# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Prerequisites

- **Rust toolchain**: `rustup toolchain install 1.94.0`
- **GStreamer** (for the GStreamer media backend — default):
  see [gstreamer docs](https://docs.rs/gstreamer/latest/gstreamer/) for
  platform-specific installation.  On macOS:
  ```bash
  brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly
  ```

## Commands

### Default (GStreamer media backend)

```bash
# Check all    (type-check every package without producing binaries)
rustup run 1.94.0 cargo check

# Build all    (root + embedder + content + net + media)
rustup run 1.94.0 cargo build --release

# Run all      (launches the embedder, which spawns content/net/media)
rustup run 1.94.0 cargo run --release
```

### AVFoundation media backend (macOS, no GStreamer required)

```bash
# 1. Build the media binary with AVFoundation
rustup run 1.94.0 cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-avfoundation

# 2. Build the root binary (media features aren't linked into root)
rustup run 1.94.0 cargo build --release

# 3. Run — the embedder spawns the AVFoundation-based media process
rustup run 1.94.0 cargo run --release
```

> `cargo run --release` does **not** rebuild the media binary.  Always run
> step 1 (`cargo build -p media --bin formal-web-media …`) before step 3.

### Without media (no video playback)

```bash
rustup run 1.94.0 cargo build --release --no-default-features
rustup run 1.94.0 cargo run --release
```

### Verify which backend is active

```bash
RUST_LOG=info cargo run --release 2>&1 | grep "creating pipeline"
# GStreamer:   "[media] creating pipeline id=…"
# AVFoundation: same prefix, but the media process log also shows "[avf] …"
```

## Project architecture

A multiprocess approach is chosen by default, because the goal is to match [Apple's guidelines for an independent browser engine](https://developer.apple.com/documentation/BrowserEngineKit/designing-your-browser-architecture). 

The following procesess are used:

- Main: running the `embedder`, `webview`, and `user_agent` crates. The process is started in `src/main.rs`.
- Content: running the `content` crate, and started in `user_agent/src/event_loop.rs`, because each process is running what is essentially a window event loop. In the future it will also run dedicated worker event loops. Service workers will likely run in their own process, and for shared worker the issue hasn't been decided yet (it seems there is a move towards isolating them per top-level sites). There is one process per [similar origin window agent](https://html.spec.whatwg.org/#similar-origin-window-agent); this is the only type of process of which there can be more than one.
- Net: running the `net` crate. That process is owned by the fetch worker in `user_agent/src/fetch.rs`, and the code in the process will essentially be the part of the fetch standard that starts at https://fetch.spec.whatwg.org/#http-network-or-cache-fetch.
- Media: running the `media` crate, started and owned by the media worker in `user_agent/src/media.rs`. The media binary uses one of two backends, selected at compile time via Cargo features:
  - `backend-gstreamer` (default) — GStreamer pipeline (uridecodebin → videoconvert → appsink). Requires GStreamer libraries at build time.
  - `backend-avfoundation` — AVFoundation (AVPlayer + AVPlayerItemVideoOutput). macOS only, no external dependencies.
  See [`media/README.md`](./media/README.md) for backend-specific details.

## Project structure

| Directory | Description |
|-----------|-------------|
| [`embedder/`](./embedder/README.md) | Application lifecycle, window management, browser chrome, redraw loop |
| [`user_agent/`](./user_agent/README.md) | Navigables, session history, event loops, timers, fetch workers |
| [`content/`](./content/README.md) | DOM, HTML algorithms, Boa JS integration, Web IDL bridges |
| [`media/`](./media/README.md) | Media pipeline: GStreamer or AVFoundation backend, frame extraction, IPC |
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
