# formal-web IPC Architecture

## Overview

The IPC (Inter-Process Communication) system connects the browser main process
(embedder / user agent) with its three helper processes:

| Process | Role | Binary |
|---|---|---|
| `formal-web-embedder` | Browser main process (window, chrome, routing) | `embedder/src/main.rs` |
| `formal-web-content` | One per webview — HTML rendering, JS, DOM | `content/src/bin/content_process.rs` |
| `formal-web-net` | Singleton — HTTP networking | `net/src/bin/net_process.rs` |
| `formal-web-graphics` | Singleton — scene composition + video/audio playback | `graphics/src/bin/graphics_process.rs` |

The media backend (AVFoundation / GStreamer from the `media` crate) runs inside
the `formal-web-graphics` process — there is no separate media process.

## Two Backend Architecture

The crate `ipc/` provides an abstract IPC layer with two selectable backends.
**`ipc-channel` is the default** (`default = ["ipc-channel-backend"]` in
`ipc/Cargo.toml`); **`bek`** (BrowserEngineKit) is the alternative. The two
flags are independent and non-exclusive; with neither enabled, `ipc` fails to
compile with a `compile_error` in `backend.rs`.

### 1. `ipc-channel` backend (default, works everywhere)

Uses Servo's [`ipc-channel`](https://crates.io/crates/ipc-channel) crate for:

- **Bootstrap**: `IpcOneShotServer<T>` — parent creates a named Mach port (macOS)
  or Unix domain socket (Linux), passes the name to the child via
  `--<name>-token <uuid>` argv, child connects back.
- **Transport**: Typed channels (`IpcSender<T>` / `IpcReceiver<T>`) with serde
  serialization. Mach port rights (macOS) and file descriptors (Linux) are
  transferred natively by ipc-channel.
- **Shared memory**: `IpcSharedMemory` regions carried as `HashMap<usize, IpcSharedMemory>`
  alongside each message, enabling zero-copy bulk data transport for paint scenes
  and video frames.
- **Routing**: `RouterProxy` / `ROUTER` to bridge ipc-channel receivers to crossbeam
  channels on both parent and child sides.

**Selection**: Enabled by `default = ["ipc-channel-backend"]` in `ipc/Cargo.toml`.
All extensions (content, net, graphics) use ipc-channel by default. It is the
only backend that runs on macOS — `ExtensionManifest::spawn` starts the helper
binary from the embedder.

### 2. `bek` backend (BrowserEngineKit — iOS/iPadOS only)

Uses Apple's [BrowserEngineKit](https://developer.apple.com/documentation/browserenginekit)
framework, which launches a browser engine's helper processes as **extensions**
(`NetworkingProcess` / `WebContentProcess` / `RenderingProcess`) under the OS —
no launchd plists, no `std::process::Command`:

- **Launch**: The OS starts the extension process. `ExtensionManifest::spawn`
  is never called; `ExtensionManifest::bek_target` maps the manifest to a BEK
  extension category plus the bundle ID of the entitled extension target.
  `Singleton` and `MultiInstance` manifests take the same path — BEK's
  `WebContentProcess` model is inherently multi-instance.
- **Transport**: The host calls `makeLibXPCConnection()` on the process object
  and gets a raw `xpc_connection_t`. Everything downstream of that reuses the
  libxpc layer (`xpc-sys`) identically to any other XPC connection:
  postcard-serialized payloads carried as `_p` data fields in XPC dictionaries.
- **Lifecycle**: `ExtensionHandle::invalidate` asks BEK to stop the process
  (`bek_sys::invalidate`). `grant_capability`/`CapabilityGrant` request
  per-task scheduling grants (`ProcessCapability` → BEK `ProcessCapability`)
  nested inside a still-alive extension handle.

**Selection**: Enabled with `--features bek` (or `--no-default-features
--features bek` to build it without `ipc-channel`). The backend is per-crate
compile-time today: when both features are on, the `ipc-channel` dispatch arm
wins and the `bek` module is compiled but inert — runtime backend choice per
manifest is future work.

**Platform reality**: BrowserEngineKit exists only in the iPhoneOS and
iPhoneSimulator SDKs. `bek-sys`'s Swift shim compiles only for iOS-family
targets; off-iOS its functions return transport errors so a macOS build that
enables `bek` by mistake still compiles (but cannot launch anything). Adopting
`bek` is a "port formal-web to iOS/iPadOS" project; see the design milestones
in `ipc/bek-sys`'s documentation for what is validated versus what still needs
entitlements, extension targets, and on-device integration.

## Crate Structure

```
ipc/                          # Abstract IPC API
├── Cargo.toml                # default = ["ipc-channel-backend"]; bek = ["dep:bek-sys"]
├── src/
│   ├── lib.rs                # Re-exports
│   ├── types.rs              # IpcSender, IpcIncoming, IpcSharedRegion,
│   │                         # ExtensionClient/Server, ExtensionManifest, etc.
│   ├── error.rs              # IpcError
│   ├── serialize.rs          # IpcSerialize/IpcDeserialize (serde aliases)
│   ├── backend.rs            # Feature-gated backend selection
│   └── backend/
│       ├── ipc_channel.rs    # ipc-channel backend: IpcOneShotServer bootstrap
│       └── bek.rs            # bek backend: BEK launch + libxpc transport,
│                             # anonymous-endpoint primitives

xpc-sys/                      # Minimal XPC FFI bindings (Apple targets)
├── Cargo.toml
├── build.rs                  # cc-based C wrapper compilation
├── src/
│   ├── lib.rs                # Conditional: re-exports apple.rs or compile_error
│   ├── apple.rs              # XpcObject, XpcDictionary, XpcEndpoint,
│   │                         # XpcConnection, XpcSharedMemory, callbacks
│   └── xpc_wrapper.c         # C shim: block-based XPC → callback-based FFI

bek-sys/                      # BrowserEngineKit FFI (Swift shim, iOS targets)
├── Cargo.toml
├── build.rs                  # swiftc against the real framework for iOS targets
├── src/
│   ├── lib.rs                # re-exports shim::* (real or off-iOS stub)
│   └── shim.rs               # extern "C" decls + wrappers (launch_process, ...)
└── swift-shim/
    └── BekShim.swift         # @_cdecl wrappers over BrowserEngineKit's
                              # Swift-only, async API
```

`xpc-sys` does not know or care how a connection came to exist: the launchd
Mach-service constructors in `apple.rs`/`xpc_wrapper.c` are macOS-only (those
APIs are unavailable on iOS), while `XpcConnection::from_raw`, the anonymous
listener/endpoint helpers, and the peer-entitlement requirement methods are
shared. Under `bek`, the connection object arrives from
`bek_process_make_xpc_connection` and is wrapped with `from_raw` unchanged.

## Public API

```rust
// Parent side: launch a helper process from its manifest
let (handle, connection) =
    ipc::ExtensionHandle::launch::<NetManifest, Request, Response>(&manifest)?;
connection.sender.send(Request::Fetch { .. })?;        // send to child
let response = connection.receiver.recv()?.payload;     // receive from child
handle.invalidate();                                    // stop the child

// Child side: connect to the parent's bootstrap token and run
ipc::run_extension::<Request, Response>(&token, move |server| {
    // The child owns one end of the bootstrap connection; bridge its
    // receiver to a crossbeam channel for select!-driven event loops.
    let sender = server.connection.sender;
    let incoming = ipc::crossbeam_proxy(server.connection.receiver);
    loop {
        match incoming.recv() {
            Ok(incoming) => { let request = incoming.payload; /* ... */ }
            Err(_) => break,
        }
    }
    Ok(())
})
```

Extension manifests (`user_agent/src/ipc_manifest.rs`) implement
`ExtensionManifest`: `endpoint()` names the topology, `spawn()` starts the
binary under `ipc-channel`, and `bek_target()` maps net → `Networking`,
graphics → `Rendering`, content → `WebContent` with the extension bundle IDs
under the `bek` backend.

## Feature Selection

The `ipc-channel-backend` and `bek` features are defined in `ipc/Cargo.toml`.
`ipc-channel-backend` is inherited transitively by crates that depend on `ipc`
(graphics, media, user_agent forward it); `bek` adds `bek-sys` as a
dependency.

```bash
# Default (ipc-channel everywhere — works on all platforms):
cargo build --release
cargo run --release

# iOS/iPadOS target validation build (bek backend, no entitlement needed —
# compiles and links the real BrowserEngineKit Swift shim):
rustup target add aarch64-apple-ios-sim
cargo build -p ipc -p bek-sys --target aarch64-apple-ios-sim \
  --no-default-features --features bek
```

| Crate | Default backend | Alternative |
|---|---|---|
| All (content, net, graphics) | ipc-channel (`ipc-channel-backend` feature enabled) | bek (iOS only, `--features bek`)| 

## Message Types

IPC message types live in `ipc_messages/src/`:

- `content.rs` — `Command` and `Event` enums for content-process communication
- `network.rs` — `Request` and `Response` for net-process HTTP fetching
- `media.rs` — video types shared between content and the graphics process
  (`MediaPipelineId`, `VideoPaintId`, `VideoFrame`, `VideoEmbedData`)
- `graphics.rs` — `GraphicsCommand` and `GraphicsEvent` for the graphics process;
  playback control (`CreateMediaPipeline`, `MediaPlay`, `MediaPause`, `MediaSeek`,
  `MediaDestroy`) rides on `GraphicsCommand`

Serialization uses `serde` + `postcard` on both backends.

## Shared Memory Transport

Bulk data (paint scenes, video frames) is transferred through shared memory regions
carried alongside each message. On the ipc-channel backend, `HashMap<usize, IpcSharedMemory>`
is serialized alongside the payload — ipc-channel transfers each `IpcSharedMemory` as a
Mach port (macOS) or fd (Linux) with zero-copy semantics.

### Font deduplication

`FontTransportSender`/`FontTransportReceiver` avoid re-sending font binary data that
was already shipped in a previous paint frame. Each font is identified by a unique
`FontIdentifier`; the sender tracks which fonts have been sent and omits duplicates.
The receiver caches font data by identifier.

## `bek` Backend — Status

The `bek` backend is validated at the API/ABI level and not yet runnable:
launching a real BEK extension requires a code-signed, entitled app on a device
or Simulator plus the EU/Japan alternative-browser-engine entitlements.

### What is in place

- **Real-API validation**: `bek-sys`'s Swift shim imports the actual
  `BrowserEngineKit` module from the iPhoneSimulator SDK and wraps the real
  classes (`NetworkingProcess`/`WebContentProcess`/`RenderingProcess`,
  `makeLibXPCConnection()`, `grantCapability(_:)`, `invalidate()`). `build.rs`
  compiles it on every iOS-target build; the `examples/ffi_link` example links
  Rust's extern declarations against the compiled shim, checking the ABI.
  `cargo build -p ipc -p bek-sys --target aarch64-apple-ios-sim
  --no-default-features --features bek` is the green validation build.
- **Transport reuse**: `IpcTransport::Xpc` + `XpcConnection::from_raw` handle a
  BEK-supplied connection exactly like a self-created one; postcard-over-`_p`
  messages are unchanged.
- **Anonymous endpoints**: `bek::create_endpoint`/`accept_endpoint` + the
  `xpc-sys` dictionary helpers (`set_endpoint`/`get_endpoint`, `XpcEndpoint`)
  implement the host-relayed direct-connection pattern. Endpoints travel as
  dictionary values inside XPC messages, never as byte blobs.
- **Peer identity**: `XpcConnection::require_entitlement_value` /
  `require_team_identity` wrap the `xpc_connection_set_peer_*_requirement`
  family; `bek::launch_extension` asserts the extension-kind entitlement
  (e.g. `com.apple.developer.web-browser-engine.webcontent`) before resuming
  the connection.

### Remaining work

- The extension-side bootstrap: extension binaries receive their connection
  from BEK's `handle(xpcConnection:)` callback (a Swift shell calling into
  Rust), and reply to the host's anonymous-endpoint requests. `run_extension`
  returns a transport error under `bek` until that reverse direction exists.
- `grant_capability` call sites in `user_agent`'s dispatch (wrap sends in a
  grant under `bek`, no-op otherwise).
- Shared-memory receive side: `send_with_shmem_map`'s Xpc arm attaches
  `xpc_shmem_create` regions to the outgoing dictionary, but the receiving
  handler does not map them back into `IpcIncoming::shmem_regions` yet.
- Rendering-extension consolidation: BEK has one rendering category; the
  graphics and media process roles need merging inside it (see the design).
- JIT entitlements for the JS engine (V8/JSC) on iOS.
- An iOS host embedder shell + Xcode "iOS Generic Extension" targets + `arm64e`
  builds, all of which need the EU/Japan entitlements first.

## Verification Trace Support

The verification tracer (`verification::tracer`) sends `LogEntry` records to a
`TraceMonitor` via `TraceSender = ipc_channel::ipc::IpcSender<LogEntry>`. The sender
is embedded in `Command::SetTraceSender(Option<TraceSender>)` and forwarded to the
content and net processes on startup.

With the ipc-channel backend (default), Mach port rights for the embedded
`IpcSender<LogEntry>` are transferred natively by ipc-channel's serde
serialization — no special handling is needed.

See `verification/src/tracer.rs` and `verification/src/monitor.rs` for the
trace sender/receiver setup.
