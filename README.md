# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Quick start

```bash
rustup toolchain install 1.92.0
rustup run 1.92.0 cargo run --release
```

This builds and runs the default windowed embedder for local development.

For a build without media support (no GStreamer dependency, no media process):

```bash
cargo run --release -- --no-media
```

This disables the media worker and prevents `HTMLMediaElement`/`HTMLVideoElement`
construction (the constructors throw `NotSupportedError`). The `--no-media` flag
is a runtime switch — media support is compiled in by default.

## Build configuration

### `media` feature

Media support (GStreamer-based video decoding) is enabled by default via the
`media` Cargo feature. To build entirely without media support:

```bash
cargo build --no-default-features
```

This removes the GStreamer dependency entirely (no need to install GStreamer
development libraries) and stubs out `HTMLMediaElement`/`HTMLVideoElement` at
the compile level. Any JS constructor call to these interfaces throws
`NotSupportedError`.

### GStreamer dependency

When the `media` feature is enabled (the default), the `media` crate depends on
[GStreamer](https://gstreamer.freedesktop.org/) for video decoding. Install the
required development packages:

**macOS (Homebrew):**
```bash
brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly
```

**Debian/Ubuntu:**
```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
     libgstreamer-plugins-bad1.0-dev gstreamer1.0-plugins-base \
     gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly
```

See the [gstreamer crate documentation](https://docs.rs/gstreamer/latest/gstreamer/)
for platform-specific setup details.

## Project structure

| Directory | Description |
|-----------|-------------|
| [`embedder/`](./embedder/README.md) | Default embedding of the engine: top-level application lifecycle, window management, browser chrome, and redraw loop |
| [`user_agent/`](./user_agent/README.md) | All global coordination: navigables and traversables, session history, event loops, timers, fetch workers, and incoming requests from the embedder and webview layers |
| [`content/`](./content/README.md) | DOM and JS execution: HTML algorithm implementations, Boa JS integration, Web IDL bridges, typed IPC — but not coordination with other components |
| [`net/`](./net/README.md) | Networking and HTTP cache (future): executes HTTP and file fetch when the Fetch spec reaches the network or cache layer |
| [`webview/`](./webview/README.md) | Public API for embedders: per-webview compositor state, hit testing, and redraw signaling |
| [`automation/`](./automation/README.md) | WebDriver and CDP wire-protocol servers |
| [`verification/`](./verification/README.md) | Trace recording, TLA+ validation, and shutdown workflow |
| `ipc_messages/` | Shared IPC message types between components |
| `src/` | Workspace entrypoint (`formal-web` binary) |
| [`tests/`](./tests/formal/README.md) | Formal tests and WPT runner |
| `artifacts/` | Default startup pages used for testing |

## Extensions

The project ships four pi coding-agent extensions for repository development:

- [**`pi-share-hf`**](.pi/extensions/pi-share-hf/README.md) — Archives pi coding sessions to `.pi/collected-sessions/` on shutdown. Includes a `collect_session` tool for mid-task checkpoints.
- [**`browser`**](.pi/extensions/browser/README.md) — Wraps formal-web's CDP server into agent-callable tools (`browser_navigate`, `browser_click`, `browser_screenshot`, etc.) for live interactive debugging.
- [**`web_standards`**](.pi/extensions/web_standards/README.md) — Lazily loads and caches web standards documents (WHATWG, W3C, etc.) and provides agent-callable tools (`spec_lookup`, `spec_select`, `spec_html`) for reading spec content interactively during development.

## Pi session archiving

Pi coding sessions are archived on shutdown to `.pi/collected-sessions/`. To upload collected sessions to the Hugging Face dataset:

```bash
./sync-hf-sessions.sh
```

Prerequisites: the `hf` CLI must be installed and authenticated with write access to the [target dataset](https://huggingface.co/datasets/formal-web/pi-coding-sessions).
