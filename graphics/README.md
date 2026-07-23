# Graphics Process — IOSurface Zero-Copy Pipeline

## Status: Cross-process IOSurface sharing is not working

The graphics process renders composed scenes to an IOSurface-backed wgpu::Texture
via Vello (intermediate texture → GPU blit → export texture).  The IOSurface
is allocated with correct RGBA FourCC and kept alive via CFRetained.  The
GPU-side pipeline (Vello render + blit) works correctly.

What does **not** work is sharing the IOSurface with the embedder process for
zero-copy compositing.  Three approaches were tried and all failed.

---

## Attempt 1: IOSurfaceLookup (global ID)

**What:** Graphics process allocates IOSurface, sends its global IOSurfaceID
over IPC.  Embedder calls `IOSurfaceRef::lookup(id)`.

**Result:** `IOSurfaceLookup` consistently returns NULL for IDs created in a
different process.  `IOSurfaceLookup` was deprecated in macOS 10.11 and on
modern macOS (Sequoia 15.x) it no longer resolves cross-process.

**Ruled out:** Not fixable — deprecated API.

---

## Attempt 2: Mach port transfer through ipc-channel serde

**What:** Embedder creates an `IpcSender<()>` channel pair, serializes the
sender inside a `GraphicsCommand::SetSurfaceTransport` message.  The
ipc-channel deserialization should transfer the Mach port right to the
graphics process via the existing port-descriptor mechanism.  Graphics process
extracts the raw `mach_port_t` from the opaque sender by reading the first
4 bytes of `OpaqueIpcSender` (UB — reaches into private struct layout).

Graphics process then constructs a raw `mach_msg` with the IOSurface's Mach
port (from `IOSurfaceCreateMachPort`) as a port descriptor, and sends it to
the embedder's port.  Embedder receives via raw `mach_msg` and calls
`IOSurfaceRef::lookup_from_mach_port`.

**Result:** Two problems:

1. **UB port extraction:** `OpaqueIpcSender` and `OpaqueIpcReceiver` are
   non-`#[repr(C)]` structs from ipc-channel's private implementation.
   Reading their first 4 bytes as `u32` is undefined behavior.  In practice
   the receiver's port extracted correctly (non-zero) but the sender's port
   arrived at the graphics process as 0, indicating the ipc-channel
   serialization/deserialization didn't transfer the send right.

2. **Hand-rolled Mach struct layout:** Several bugs in the custom Mach
   message types — wrong OOL descriptor constant (3 instead of 1), missing
   `MACH_RCV_TIMEOUT` flag, `msgh_reserved` field that doesn't exist in
   mach2's header struct, etc.

**Ruled out:** The UB port extraction is a correctness and security hazard.
Fixing the ipc-channel port transfer would require deep debugging of the
serde-based port-right-transfer mechanism, which is non-trivial.

---

## Attempt 3: Bootstrap register/lookup (mach2)

**What:** Embedder allocates a receive port via `mach_port_allocate` and
registers it with the bootstrap server under a unique name
(`org.fw.surface.<pid>`).  Sends the bootstrap name as a plain string to the
graphics process (via existing IPC).  Graphics process calls
`bootstrap_look_up(name)` to get a send right, then uses `mach2` types to
send a `mach_msg` with the IOSurface port as a descriptor.

**Result:** `bootstrap_look_up` returns a non-zero port name consistently.
But `mach_msg` fails with `MACH_SEND_INVALID_DEST` (0x1000000A) on every
attempted send.  The port from `bootstrap_look_up` is not a valid send right
to the registered receive port.

Most likely cause: `bootstrap_register` was deprecated in macOS 10.14 and on
Sequoia 15.x it appears to create dead-name entries or return success without
actually creating a working send right for the lookup caller.  The bootstrap
server's behavior with `bootstrap_register` of user-allocated receive rights
is not reliable on this OS version.

**Ruled out:** The bootstrap path is a dead end on modern macOS without
`bootstrap_check_in` (which requires pre-registered launchd plist services)
or root entitlements.

---

## Current state

The `ipc::mach_transport` module (gated on `#[cfg(target_os = "macos")]`)
contains working `mach2`-based primitives for sending and receiving
IOSurface port rights via raw Mach messages.  The `bootstrap_register` /
`bootstrap_look_up` pair is wired through the existing IPC as
`GraphicsCommand::SetSurfaceTransport { bootstrap_name: String }`.

These primitives could work with a DIFFERENT channel-establishment mechanism.
Options that remain unexplored:

- **Unix socket + SCM_RIGHTS:** Both processes are children of the same
  parent.  The embedder creates a socket pair, keeps one end, sends the
  other end (as a file descriptor) to the graphics process via the existing
  IPC (using `sendmsg` with `SCM_RIGHTS`).  Then the graphics process sends
  a `mach_msg` containing the IOSurface port to a well-known receive port
  established through the socket.

- **Inline the renderer:** Eliminate the cross-process GPU sharing problem
  entirely by running the compositor and Vello renderer in the embedder
  process.  The content process already sends `PaintFrame` via IPC; the
  graphics process boundary exists mainly for process isolation, not
  architectural necessity.

- **XPC IOSurface objects:** The `xpc_dictionary_set_iosurface` /
  `xpc_dictionary_copy_iosurface` APIs handle the Mach port transfer
  transparently.  This requires the XPC IPC backend (currently incomplete)
  or a standalone XPC channel just for surface transport.

- **CPU readback + shared memory:** The graphics process reads back the
  IOSurface content to a staging buffer (GPU → CPU), ships the pixels via
  `IpcSharedRegion`, and the embedder creates a Vello `ImageData` from them.
  CPU round-trip but will work immediately.  Partially implemented in an
  earlier iteration of this code.
