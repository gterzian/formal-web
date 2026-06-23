# formal-web IPC Architecture

## Overview

The IPC (Inter-Process Communication) system connects the browser main process
(embedder / user agent) with its three helper processes:

| Process | Role | Binary |
|---|---|---|
| `formal-web-embedder` | Browser main process (window, chrome, routing) | `embedder/src/main.rs` |
| `formal-web-content` | One per webview — HTML rendering, JS, DOM | `content/src/bin/content_process.rs` |
| `formal-web-net` | Singleton — HTTP networking | `net/src/bin/net_process.rs` |
| `formal-web-media` | Singleton — GStreamer media playback | `media/src/bin/media_process.rs` |

## Two Backend Architecture

The crate `ipc/` provides an abstract IPC layer with two selectable backends.
**`ipc-channel` is the default** (`default = ["ipc-channel-backend"]` in
`ipc/Cargo.toml`). The native XPC backend remains available on macOS when the
feature is disabled.

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
All extensions (content, net, media) use ipc-channel by default.

### 2. Native XPC backend (macOS-only, experimental)

Uses Apple's XPC framework directly for:

- **Bootstrap**: Parent connects to a launchd-registered XPC service name.
  launchd starts the helper process and delivers the peer connection.
- **Transport**: Postcard-serialized payloads carried as `_p` data fields in XPC
  dictionaries (`xpc_dictionary_set_data` / `xpc_dictionary_get_data`).
- **Shared memory**: `xpc_shmem_create` / `xpc_shmem_map` (stub — not yet wired to
  the message pipeline).

**Selection**: Enabled on `target_vendor = "apple"` when the `ipc-channel-backend`
feature is disabled (`--no-default-features`).

**Mixed-mode fallback**: When the feature is disabled, only `net` and `media`
use XPC. The `content` process always uses ipc-channel (Unix domain sockets)
because macOS AMFI rejects ad-hoc-signed embedded XPC services — a paid Apple
Developer certificate would be required.

## Crate Structure

```
ipc/                          # Abstract IPC API
├── Cargo.toml                # default = ["ipc-channel-backend"]
├── src/
│   ├── lib.rs                # Re-exports
│   ├── types.rs              # IpcSender, IpcIncoming, IpcSharedRegion,
│   │                         # ExtensionClient/Server, ExtensionManifest, etc.
│   ├── error.rs              # IpcError
│   ├── serialize.rs          # IpcSerialize/IpcDeserialize (serde aliases)
│   ├── backend.rs            # Feature-gated backend selection
│   └── backend/
│       ├── ipc_channel.rs    # ipc-channel backend: IpcOneShotServer bootstrap
│       └── xpc.rs            # XPC backend: launchd listener + postcard

xpc-sys/                      # Minimal XPC FFI bindings (Apple only)
├── Cargo.toml
├── build.rs                  # cc-based C wrapper compilation
├── src/
│   ├── lib.rs                # Conditional: re-exports apple.rs or compile_error
│   ├── apple.rs              # XpcObject, XpcDictionary, XpcConnection,
│   │                         # XpcSharedMemory, callback wrappers
│   └── xpc_wrapper.c         # C shim: block-based XPC → callback-based FFI

xpc-services/                 # Launchd XPC service configuration (XPC backend only)
├── formal-web.net.plist
├── formal-web.media.plist
├── formal-web.content.plist
├── install.sh
└── README.md
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

The `ipc-channel-backend` feature is defined in `ipc/Cargo.toml` and inherited
transitively by all crates that depend on `ipc`.

```bash
# Default (ipc-channel everywhere — works on all platforms):
cargo build --release
cargo run --release

# Mixed: XPC for net/media, ipc-channel for content (macOS only):
cargo build --release --no-default-features --features media
# Requires XPC service setup first:
./ipc/xpc-services/install.sh $(pwd)/target/release
launchctl load ~/Library/LaunchAgents/formal-web.net.plist
launchctl load ~/Library/LaunchAgents/formal-web.media.plist
cargo run --release --no-default-features --features media
```

| Crate | Default backend | Alternative |
|---|---|---|
| All (content, net, media) | ipc-channel (`ipc-channel-backend` feature enabled) | XPC (macOS only, `--no-default-features`)| 

## Message Types

IPC message types live in `ipc_messages/src/`:

- `content.rs` — `Command` and `Event` enums for content-process communication
- `network.rs` — `Request` and `Response` for net-process HTTP fetching
- `media.rs` — `MediaCommand` and `MediaEvent` for media-process playback

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

## XPC Backend — Status

The XPC backend is experimental and requires additional setup (launchd plists,
ad-hoc code signing). It is disabled by default.

### Requirements (XPC mode only)

1. Disable the default feature: `--no-default-features --features media`
2. Build all helper binaries: `cargo build --release --no-default-features --features media`
3. Install XPC service plists: `./ipc/xpc-services/install.sh $(pwd)/target/release`
4. Load services: `launchctl load ~/Library/LaunchAgents/formal-web.net.plist`
                               `~/Library/LaunchAgents/formal-web.media.plist`
5. Run: `cargo run --release --no-default-features --features media`

Check `launchctl list | grep formal-web` for exit codes:
- `0` = clean exit ✅
- Non-zero = investigate `log show --predicate 'process == "formal-web-*"'`

### Known limitations

- Content process cannot use XPC (macOS AMFI rejects ad-hoc-signed embedded
  XPC services). Content always uses ipc-channel even in mixed mode.
- Shared memory via `xpc_shmem_create` / `xpc_shmem_map` is not yet wired to
  the message pipeline.
- The anonymous XPC endpoint approach (`xpc_connection_create(NULL, queue)` +
  `xpc_endpoint_create`) was explored for content multi-instance mode but
  abandoned for the same AMFI reason. The `start_multi_instance`/`run_multi_instance`
  code was removed in favour of always using ipc-channel for content.

### macOS 26 XPC quirks

Developers debugging the native XPC backend should be aware of these platform
behaviours observed on macOS 26:

- **Listener events are `XPC_TYPE_CONNECTION` directly**, not dictionaries.
  The peer connection is the event object itself — use `fw_xpc_peer_from_event()`
  and never call `xpc_dictionary_get_string` on a connection object (that
  triggers `_xpc_api_misuse` → SIGTRAP).
- **Mach cancel events deliver garbage pointers** (e.g., `0x10d8`, `0x1a0c`)
  after the connection is closed. Rust callbacks must check pointer validity
  before dereferencing. An `alive` flag in `SharedContext` prevents
  use-after-free.
- **`xpc_main` requires the main thread** — it calls `dispatch_main()`
  internally and never returns. A C wrapper (`fw_xpc_run_service`) exists
  but is currently unused; the XPC backend uses `listen()` + `resume()`
  directly on a dedicated dispatch queue instead.
- **Error events must never be swallowed.** XPC delivers `XPC_TYPE_ERROR`
  events on connection invalidation. If these are not forwarded to Rust,
  the parent process deadlocks waiting for a response from a dead helper.
  The C wrapper and Rust callbacks both forward all event types.

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
