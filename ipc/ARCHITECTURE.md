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

The crate `ipc/` provides an abstract IPC layer with two selectable backends:

### 1. `ipc-channel` backend (CURRENT DEFAULT)

Uses Servo's [`ipc-channel`](https://crates.io/crates/ipc-channel) crate for:

- **Bootstrap**: `IpcOneShotServer<T>` — parent creates a named Mach port, passes the
  name to the child via `--<name>-token <uuid>` argv, child connects back.
- **Transport**: Typed channels (`IpcSender<T>` / `IpcReceiver<T>`) with serde+postcard
  serialization. Shared memory via `IpcSharedMemory`.
- **Routing**: `RouterProxy` / `ROUTER` to bridge ipc-channel receivers to crossbeam
  channels on both parent and child sides.

**Selection**: `ipc-channel-backend` Cargo feature on `ipc/`.

### 2. Native XPC backend (macOS only, EXPERIMENTAL)

Uses Apple's XPC framework directly for:

- **Bootstrap**: Parent creates a unique temporary XPC listener, passes the service
  name to the child via the same `--<name>-token <uuid>` argv mechanism. Child connects
  as an XPC client to the bootstrap listener.
- **Transport**: Postcard-serialized payloads carried as `_p` data fields in XPC
  dictionaries (`xpc_dictionary_set_data` / `xpc_dictionary_get_data`).
- **Shared memory**: `xpc_shmem_create` / `xpc_shmem_map` (stub — not yet wired to
  the message pipeline).

**Selection**: Compiled when `ipc-channel-backend` is NOT enabled and `target_vendor = "apple"`.

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
# Default (ipc-channel backend, works on all platforms):
cargo build --release
cargo run --release

# Native XPC backend (macOS only, experimental):
# Requires helper binaries and launchd plists (see xpc-services/README.md)
cargo build --release --no-default-features --features media
```

| Crate | How ipc-channel-backend is enabled |
|---|---|
| `user_agent` | Via default feature `ipc-channel-backend` |
| `net` | `features = ["ipc-channel-backend"]` on ipc dep |
| `content` | `features = ["ipc-channel-backend"]` on ipc dep |
| `media` | `features = ["ipc-channel-backend"]` on ipc dep |

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

## XPC Backend — Remaining Work

The native XPC backend infrastructure is complete but the **content process crashes**
during launchd XPC service initialization (exit code -5 / SIGTRAP). Likely causes:

1. **XPC service lifecycle**: When launchd starts an XPC service with
   `ServiceType = Application`, the process must signal readiness to launchd.
   Our `run_extension` creates a listener via `xpc_connection_create_mach_service`,
   which might not be the correct pattern for launchd-started services.

2. **Listener on registered name**: The child creates an XPC listener on the same
   name that launchd registered (e.g. `formal-web.content`). This might conflict
   with launchd's ownership of the Mach service name.

3. **Startup race**: The child process might need to call `xpc_connection_resume`
   on the listener before the parent's connection arrives, or launchd might deliver
   the connection through a different mechanism (e.g. `xpc_main`).

### To debug:

```bash
# Build helpers
cargo build --release

# Install and load XPC services
./xpc-services/install.sh $(pwd)/target/release
launchctl load ~/Library/LaunchAgents/formal-web.net.plist
launchctl load ~/Library/LaunchAgents/formal-web.media.plist
launchctl load ~/Library/LaunchAgents/formal-web.content.plist

# Run with native XPC backend
cargo run --release --no-default-features --features media
```

Check `launchctl list | grep formal-web` for exit codes:
- `-5` = SIGTRAP (content crash — needs investigation)
- `0` = clean exit (net/media work)
- `-11` = SIGSEGV (memory issue)

Check system logs: `log show --predicate 'process == "formal-web-content"' --last 30s`

### Known working:
- ✅ Net process XPC service (exit code 0)
- ✅ Media process XPC service (exit code 0)
- ❌ Content process XPC service (exit code -5, SIGTRAP)

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
