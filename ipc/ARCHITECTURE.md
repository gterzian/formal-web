# formal-web IPC Architecture

## Overview

The IPC (Inter-Process Communication) system connects the main `formal-web` process
with its three helper processes:

| Process | Role | Binary |
|---|---|---|
| `formal-web` | Browser main process (embedder, window, chrome) | `src/main.rs` |
| `formal-web-content` | One per webview — HTML rendering, JS, DOM | `content/src/bin/content_process.rs` |
| `formal-web-net` | Singleton — HTTP networking | `net/src/bin/net_process.rs` |
| `formal-web-media` | Singleton — GStreamer media playback | `media/src/bin/media_process.rs` |

## Two Backend Architecture

The crate `ipc/` provides an abstract IPC layer with two selectable backends.
On macOS, **native XPC is the default**. The ipc-channel backend remains
available via the `ipc-channel-backend` Cargo feature (used by WPT and
verification tooling on all platforms).

### 1. Native XPC backend (macOS default)

Uses Apple's XPC framework directly for:

- **Bootstrap**: Parent connects to a launchd-registered XPC service name.
  launchd starts the helper process and delivers the peer connection.
- **Transport**: Postcard-serialized payloads carried as `_p` data fields in XPC
  dictionaries (`xpc_dictionary_set_data` / `xpc_dictionary_get_data`).
- **Shared memory**: `xpc_shmem_create` / `xpc_shmem_map` (stub — not yet wired to
  the message pipeline).

**Selection**: Default on `target_vendor = "apple"` when `ipc-channel-backend`
feature is not enabled.

### 2. `ipc-channel` backend (testing, verification)

Uses Servo's [`ipc-channel`](https://crates.io/crates/ipc-channel) crate for:

- **Bootstrap**: `IpcOneShotServer<T>` — parent creates a named Mach port, passes the
  name to the child via `--<name>-token <uuid>` argv, child connects back.
- **Transport**: Typed channels (`IpcSender<T>` / `IpcReceiver<T>`) with serde+postcard
  serialization. Shared memory via `IpcSharedMemory`.
- **Routing**: `RouterProxy` / `ROUTER` to bridge ipc-channel receivers to crossbeam
  channels on both parent and child sides.

**Selection**: `ipc-channel-backend` Cargo feature on individual crates.
`cargo build --release -p content --features ipc-channel-backend` etc.

## Crate Structure

```
ipc/                          # Abstract IPC API
├── Cargo.toml                # Feature: ipc-channel-backend
├── src/
│   ├── lib.rs                # Re-exports
│   ├── types.rs              # IpcSender, IpcIncoming, IpcSharedRegion,
│   │                         # ExtensionClient/Server, ExtensionManifest, etc.
│   ├── error.rs              # IpcError
│   ├── serialize.rs          # IpcSerialize/IpcDeserialize (serde aliases)
│   ├── backend.rs            # Feature-gated backend selection
│   └── backend/
│       ├── ipc_channel.rs    # ipc-channel backend: IpcOneShotServer bootstrap
│       └── native.rs         # XPC backend: unique bootstrap listener + postcard

xpc-sys/                      # Minimal XPC FFI bindings (Apple only)
├── Cargo.toml
├── build.rs                  # cc-based C wrapper compilation
├── src/
│   ├── lib.rs                # Conditional: re-exports apple.rs or compile_error
│   ├── apple.rs              # XpcObject, XpcDictionary, XpcConnection,
│   │                         # XpcSharedMemory, callback wrappers
│   └── xpc_wrapper.c         # C shim: block-based XPC → callback-based FFI

xpc-services/                 # Launchd XPC service configuration
├── formal-web.net.plist      # Singleton XPC service for net helper
├── formal-web.media.plist    # Singleton XPC service for media helper
├── formal-web.content.plist  # MultipleInstances XPC service for content helper
├── install.sh                # Installs plists with correct binary paths
└── README.md                 # XPC setup instructions
```

## Public API

```rust
// Parent side: start a helper process
let client = ipc::start_extension::<NetManifest, NetRequest, NetResponse>(&manifest)?;
client.tx.send(NetRequest::Fetch { ... })?;       // send to child
let response = client.rx.recv()?.payload;          // receive from child
let child: Option<std::process::Child> = client.child;  // process handle

// Child side: run as a helper process
let server = ipc::run_extension::<NetManifest, NetRequest, NetResponse>(
    &manifest, token, service_name)?;
let request = server.rx.recv()?.payload;           // receive from parent
server.tx.send(NetResponse { ... })?;              // send to parent
```

## Feature Selection

```bash
# Default (XPC on macOS, requires launchd plists):
cargo build --release
./ipc/xpc-services/install.sh $(pwd)/target/release
launchctl load ~/Library/LaunchAgents/formal-web.net.plist
launchctl load ~/Library/LaunchAgents/formal-web.media.plist
launchctl load ~/Library/LaunchAgents/formal-web.content.plist
cargo run --release

# ipc-channel backend (testing, WPT, verification):
cargo build --release -p content --features ipc-channel-backend
cargo build --release -p net --features ipc-channel-backend
cargo build --release -p media --features ipc-channel-backend
cargo run --release --features ipc-channel-backend
```

| Crate | How backend is selected |
|---|---|
| All | The `ipc-channel-backend` feature on each crate enables `ipc/ipc-channel-backend`. By default this feature is disabled → native XPC (macOS). Enable it for the ipc-channel transport.|

## Message Types

IPC message types live in `ipc_messages/src/`:

- `content.rs` — `Command` and `Event` enums for content-process communication
- `network.rs` — `Request` and `Response` for net-process HTTP fetching
- `media.rs` — `MediaCommand` and `MediaEvent` for media-process playback

Message serialization uses `serde` + `postcard` on both backends. All shared memory
was migrated from `IpcSharedMemory` to `Vec<u8>` in message types for cross-backend
compatibility.

## Clipboard

Clipboard operations no longer use blocking IPC round-trips:

- **Paste**: Content reads the system clipboard directly via `arboard::Clipboard`
  (with a prefetch cache for embedder-dispatched paste events).
- **Copy/Cut**: Fire-and-forget `ClipboardWriteRequested { text }` message — no
  reply expected.

## XPC Backend — Status

The native XPC backend is now the default on macOS. All three helper processes
(content, net, media) work correctly with exit code 0.

### Requirements

1. Build all helper binaries: `cargo build --release`
2. Install XPC service plists: `./ipc/xpc-services/install.sh $(pwd)/target/release`
3. Load services: `launchctl load ~/Library/LaunchAgents/formal-web.*.plist`
4. Run: `cargo run --release` (or `cargo run --release cdp --port 9222` for CDP)

Check `launchctl list | grep formal-web` for exit codes:
- `0` = clean exit ✅
- Non-zero = investigate `log show --predicate 'process == "formal-web-*"'`

### Previous issues resolved

| Issue | Fix |
|---|---|
| Content process crash (SIGTRAP) — hardcoded `features = ["ipc-channel-backend"]` in Cargo.toml | Made `ipc-channel-backend` an opt-in feature; default is XPC |
| C-side memory leaks in `xpc_wrapper.c` (`malloc` / never freed) | Replaced manual `malloc` with Clang block capture by value |
| Error events swallowed for client/peer connections (deadlock) | Forward all XPC events (errors + dictionaries) to Rust callbacks |
| Fat pointer segfault (thin→fat pointer cast) | Double-indirection: `Box<Box<dyn Fn(...)>>` |
| Double `munmap` on XPC shared memory | `needs_munmap` flag to track ownership |
| Listener dropped immediately in `run_extension` | Store `_listener` in `ExtensionServer` |
| Peer not configured before listener callback returns (XPC contract violation) | Configure + resume peer inside listener callback, per Apple docs |
| Closure context leaked when no invalidation fires | Shared `Arc<Mutex<Option<ContextEntry>>>` between callback and `Drop` |
| Missing `fw_xpc_cancel` in `XpcConnection::drop` | Call `cancel` before `release` |
| `Clone` for `XpcConnection` missing `context` field | Clone `Arc`, skip cleanup on clones |

## Broken: Verification & WPT

The verification tracer (`verification::tracer`) and WPT runner (`wpt_runner`) are
broken after the IPC switch. Both use `TraceSender = IpcSender<LogEntry>` embedded
in `Command::SetTraceSender`, which fails with:

```
Error in IO: Bogus destination port
```

The `IpcSender<LogEntry>` Mach port is invalidated during the bootstrap handshake.
Likely cause: the ipc-channel `IpcOneShotServer::accept()` transfers Mach port
rights that the embedded `TraceSender` depends on, and the rights are consumed
or invalidated during the new `start_extension`/`run_extension` flow.

### Symptoms

- WPT tests hang or exit with code 1 (runner starts wptserve + WebDriver but
  tests never complete)
- `verify-navigation.sh` reports `formal-web exited with a failure status`
  with many `verification trace send failed` errors
- `bogus destination port` errors from the content process

### Possible fix

The `TraceSender` in `Command::SetTraceSender(Option<TraceSender>)` is
serialized through `ipc_channel::ipc::IpcSender<Command>`, which should handle
`IpcSender<LogEntry>` port transfer natively. The issue might be in how
`send_command_inner` clones the Command before sending, or the Mach port
namespace of the child process differs from the parent's.

### To restore

1. Run a simple verification test to reproduce:
   ```bash
   target/release/formal-web --verify --headless
   ```
2. Check for `bogus destination port` errors in the output
3. Debug the Mach port transfer in `Command::SetTraceSender` — verify that
   the `IpcSender<LogEntry>` survives the round-trip through `BootstrapMessage`
   and into the child's `TLATracer`

See `verification/src/tracer.rs` and `verification/src/monitor.rs` for the
trace sender/receiver setup.
