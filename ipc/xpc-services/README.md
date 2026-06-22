# XPC Service Configuration for formal-web

The `ipc/` crate provides an abstract IPC layer with two backends:

- **`ipc-channel-backend`** (default, works reliably on all platforms)
- **Native XPC** (macOS only, requires additional setup)

## Prerequisites for Native XPC

The native XPC backend requires each helper process to be registered as a launchd
XPC service. This is currently experimental — the content process has a known
lifecycle issue (crashes during initialization when launched as an XPC service).

## Setup (for native XPC development)

```bash
# 1. Build all binaries
cargo build --release -p net --bin formal-web-net
cargo build --release -p media --bin formal-web-media
cargo build --release -p content --bin formal-web-content

# 2. Install XPC service plists with correct binary paths
./xpc-services/install.sh $(pwd)/target/release

# 3. Load services into launchd
launchctl load ~/Library/LaunchAgents/formal-web.net.plist
launchctl load ~/Library/LaunchAgents/formal-web.media.plist
launchctl load ~/Library/LaunchAgents/formal-web.content.plist

# 4. Run with native XPC backend
cargo run --release --no-default-features --features media
```

## Known Issues

- Content process crashes with SIGTRAP during XPC service initialization.
  The ipc-channel backend works reliably.
- Service plists must be updated whenever binary paths change (e.g., after
  switching between debug/release builds).

## Architecture

| Service Name | Type | Binary |
|---|---|---|
| `formal-web.net` | Singleton (Application) | `formal-web-net` |
| `formal-web.media` | Singleton (Application) | `formal-web-media` |
| `formal-web.content` | MultipleInstances | `formal-web-content` |
