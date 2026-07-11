# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Prerequisites

- **Rust toolchain**: `rustup toolchain install 1.94.0`
- **macOS**: No additional system libraries required.  AVFoundation is the
  default media backend (system framework, always available).

## Commands

### macOS (AVFoundation — default)

AVFoundation is used automatically.  No extra build steps needed.

```bash
# Check all    (type-check every package without producing binaries)
rustup run 1.94.0 cargo check

# Build all    (root + embedder + content + net + media with AVFoundation)
rustup run 1.94.0 cargo build --release

# Run all      (launches the embedder, which spawns content/net/media)
rustup run 1.94.0 cargo run --release
```

### JS engine backend

Two JS engine backends are available.  Feature flags propagate from the
root `Cargo.toml` through the workspace to control which engine the
`content` crate links against.

| Backend | Default | Platform | Description |
|---|---|---|---|
| **Boa** | ✅ yes | All platforms | Rust-native JS engine with Wasmtime for WebAssembly. Production-ready for everyday development. |
| **JSC** | — | macOS only | Uses JavaScriptCore via the system framework. Experimental — content crate compiles on both backends but runtime is less stable. |

#### Feature flag mechanics

The root `Cargo.toml` defines two mutually-exclusive feature sets:

- **`default = ["media", "boa"]`** — Enables the Boa backend and media support.
- **`jsc`** — Switches to the JSC backend (`--no-default-features --features jsc,media`).

The `--no-default-features` flag disables `boa` (and thus the
`boa_engine`/`wasmtime` dependencies).  `--features jsc,media` enables
JSC and media support.

#### Build

```bash
# Boa (default) — no special flags needed
rustup run 1.94.0 cargo build --release

# JSC (macOS only) — disable default Boa, enable JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc,media

# Without any JS engine (no content process, useful for embedder-only work)
rustup run 1.94.0 cargo build --release --no-default-features
```

#### Run

```bash
# Boa (default)
rustup run 1.94.0 cargo run --release

# JSC (macOS only)
rustup run 1.94.0 cargo run --release --no-default-features --features jsc,media
```

#### Unit tests

```bash
# Boa
rustup run 1.94.0 cargo test -p content generic_js_test

# JSC
rustup run 1.94.0 cargo test --no-default-features --features jsc -p content generic_js_test
```

#### WPT (Web Platform Tests)

The WPT runner (`cargo run --release -- wpt`) builds the `formal-web`
entrypoint binary with the active feature set.  The runner itself does
not depend on the JS engine — it drives the embedder via WebDriver —
but the embedder needs the content crate with the correct engine.

```bash
# Boa (default)
rustup run 1.94.0 cargo run --release -- wpt

# JSC (macOS only)
rustup run 1.94.0 cargo run --release --no-default-features --features jsc,media -- wpt
```

**Current JSC status:** The content crate compiles on JSC but
`run_content_process` returns an error at runtime, so the JSC backend
path is not yet functional for full WPT runs.  Boa remains the
production development backend.

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

### Without media (no video playback)

```bash
rustup run 1.94.0 cargo build --release --no-default-features
rustup run 1.94.0 cargo run --release
```

### Verify which engine backend is active

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

- [**`browser`**](.pi/extensions/browser/README.md) — Wraps CDP server into agent-callable dev tools
- [**`web_standards`**](.pi/extensions/web_standards/README.md) — Lazily loaded web spec content for interactive reading
