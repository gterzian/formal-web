# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Prerequisites

- **Rust toolchain**: `rustup toolchain install 1.94.0`
- **GStreamer** (for media): see [gstreamer docs](https://docs.rs/gstreamer/latest/gstreamer/) for platform-specific installation

## Commands

```bash
# Check all    (type-check every package without producing binaries)
rustup run 1.94.0 cargo check

# Build all    (root + embedder + content + net + media)
rustup run 1.94.0 cargo build --release

# Run all      (launches the embedder, which spawns content/net/media)
rustup run 1.94.0 cargo run --release
```

Without media:

```bash
rustup run 1.94.0 cargo build --release --no-default-features
rustup run 1.94.0 cargo run --release -- --no-default-features
```

## Project structure

| Directory | Description |
|-----------|-------------|
| [`embedder/`](./embedder/README.md) | Application lifecycle, window management, browser chrome, redraw loop |
| [`user_agent/`](./user_agent/README.md) | Navigables, session history, event loops, timers, fetch workers |
| [`content/`](./content/README.md) | DOM, HTML algorithms, Boa JS integration, Web IDL bridges |
| [`media/`](./media/README.md) | GStreamer video decoding pipeline |
| [`net/`](./net/README.md) | HTTP and file fetch |
| [`webview/`](./webview/README.md) | Embedder-facing compositor and redraw API |
| [`automation/`](./automation/README.md) | WebDriver and CDP wire-protocol servers |
| [`verification/`](./verification/README.md) | Trace recording, TLA+ validation |
| `ipc_messages/` | Shared IPC message types |
| [`tests/`](./tests/formal/README.md) | Formal tests and WPT runner |
| `artifacts/` | Default startup pages for testing |

## Extensions

- [**`pi-share-hf`**](.pi/extensions/pi-share-hf/README.md) — Archives pi coding sessions to `.pi/collected-sessions/`
- [**`browser`**](.pi/extensions/browser/README.md) — Wraps CDP server into agent-callable dev tools
- [**`web_standards`**](.pi/extensions/web_standards/README.md) — Lazily loaded web spec content for interactive reading

## Session archiving

Sessions are archived to `.pi/collected-sessions/` on shutdown.  Upload with:

```bash
./sync-hf-sessions.sh
```
