# XPC Service Configuration for formal-web

The `ipc/` crate provides an abstract IPC layer with two backends:

- **`ipc-channel-backend`** (default, works reliably on all platforms)
- **Native XPC** (macOS only, experimental, requires additional setup)

The native XPC backend is **disabled by default** — enable it with
`--no-default-features --features media`.

## Prerequisites for Native XPC

The native XPC backend requires each helper process to be registered as a launchd
XPC service. The content process always uses ipc-channel even in XPC mode because
macOS AMFI rejects ad-hoc-signed embedded XPC services.

## Setup (for native XPC development)

```bash
# 1. Build all binaries
cargo build --release --no-default-features --features media

# 2. Install XPC service plists with correct binary paths
./xpc-services/install.sh $(pwd)/target/release

# 3. Load services into launchd (content plist is unused — content uses ipc-channel)
launchctl load ~/Library/LaunchAgents/formal-web.net.plist
launchctl load ~/Library/LaunchAgents/formal-web.media.plist

# 4. Run with native XPC backend
cargo run --release --no-default-features --features media
```

## Why Content Can't Use XPC

macOS **AMFI (Apple Mobile File Integrity)** rejects ad-hoc-signed binaries in
embedded XPC services with error:

```
amfid: not valid: Error Code=-423
"The file is adhoc signed or signed by an unknown certificate chain"
```

This happens even with Developer Mode enabled (`developerMode: 1`). Embedded
XPC services (inside an `.app` bundle's `XPCServices/` directory) require a
**paid Apple Developer certificate** for code signing — ad-hoc signing is
insufficient.

### Diagnostic journey

1. **Identifier mismatch**: `launchd: failed lookup: name = com.formal-web.content,
   error = 3: No such process` — the XPC service bundle identifier was not
   prefixed with the app's identifier. Fixed by renaming from
   `com.formal-web.content` to `com.formal-web.app.content`.

2. **SIGTRAP from API misuse**: Child crashed with `_xpc_api_misuse` → SIGTRAP
   in `xpc_listener_callback` — macOS 26 calls `xpc_dictionary_get_string` on
   non-dictionary objects (connections). Fixed by adding `xpc_get_type()` checks
   in both the C wrapper blocks and Rust callbacks.

3. **Garbage pointers from Mach cancel events**: Cancel events delivered with
   invalid/freed object pointers after connection close. Fixed by:
   (a) early pointer-range check in Rust callbacks,
   (b) `alive` flag in `SharedContext`,
   (c) leaking `SharedContext` to prevent use-after-free.

4. **Final blocker**: AMFI rejection of ad-hoc signed binary. Cannot be
   worked around without a paid Apple Developer certificate.

## Known Issues

- Content process cannot use XPC (macOS AMFI rejects ad-hoc-signed embedded
  XPC services). Content always uses ipc-channel even in mixed mode.
- Service plists must be updated whenever binary paths change.

## Architecture (XPC mode)

| Service Name | Type | Binary | Backend |
|---|---|---|---|
| `formal-web.net` | Singleton (Application) | `formal-web-net` | XPC |
| `formal-web.media` | Singleton (Application) | `formal-web-media` | XPC |
| `formal-web.content` | MultipleInstances | `formal-web-content` | ipc-channel (always) |
