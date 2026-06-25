# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Prerequisites

- **Rust toolchain**: `rustup toolchain install 1.94.0`
- **macOS**: No additional system libraries required.  AVFoundation is the
  default media backend (system framework, always available).
- **Linux**: GStreamer libraries for the GStreamer media backend:
  see [gstreamer docs](https://docs.rs/gstreamer/latest/gstreamer/) for
  platform-specific installation.  On Debian/Ubuntu:
  ```bash
  apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
  ```

## Commands

### macOS (AVFoundation — default, no GStreamer required)

AVFoundation is used automatically.  No extra build steps needed.

```bash
# Check all    (type-check every package without producing binaries)
rustup run 1.94.0 cargo check

# Build all    (root + embedder + content + net + media with AVFoundation)
rustup run 1.94.0 cargo build --release

# Run all      (launches the embedder, which spawns content/net/media)
rustup run 1.94.0 cargo run --release
```

> `cargo run --release` only rebuilds the root binary — it does **not**
> rebuild the `formal-web-media` binary.  When switching between backends,
> rebuild the media binary explicitly with the desired feature flags.

### macOS (GStreamer — opt-in)

GStreamer is available on macOS for users who prefer it over AVFoundation.
Requires GStreamer libraries:
```bash
brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly
```

Build the media binary with the GStreamer backend:
```bash
cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-gstreamer
```

### Linux (GStreamer)

On Linux, GStreamer is the only available backend.

```bash
# Check all
rustup run 1.94.0 cargo check

# Build all
rustup run 1.94.0 cargo build --release

# Run all
rustup run 1.94.0 cargo run --release
```

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
- Media: running the `media` crate, started and owned by the media worker in `user_agent/src/media.rs`. The media binary uses one of two backends, selected at compile time. Backend selection is platform-dependent:
  - **macOS/iOS**: AVFoundation (AVPlayer + AVPlayerItemVideoOutput) by default.
    GStreamer available opt-in via the `backend-gstreamer` feature.
  - **Linux**: GStreamer (uridecodebin → videoconvert → appsink).
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
