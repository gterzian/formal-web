# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Quick start

```bash
rustup toolchain install 1.92.0
rustup run 1.92.0 cargo run --release
```

This builds and runs the default windowed embedder for local development.

Media support (GStreamer-based video decoding) is enabled by default. See the
[gstreamer crate documentation](https://docs.rs/gstreamer/latest/gstreamer/)
for platform-specific installation instructions.

To build entirely without media support (no GStreamer dependency):

```bash
cargo run --release --no-default-features
```

This excludes the `media` Cargo feature from compilation. `HTMLMediaElement`
and `HTMLVideoElement` constructors throw `NotSupportedError` at runtime.


## Project architecture

A multiprocess approach is chosen by default, because that is the gold standard for web engines. 

The following procesess are used:

- Main: running the `embedder`, `webview`, and `user_agent` crates. The process is started in `src/main.rs`.
- Content: running the `content` crate, and started in `user_agent/src/event_loop.rs`, because each process is running what is essentially a window event loop. In the future it will also run dedicated worker event loops. Service workers will likely run in their own process, and for shared worker the issue hasn't been decided yet (it seems there is a move towards isolating them per top-level sites). There is one process per [similar origin window agent](https://html.spec.whatwg.org/#similar-origin-window-agent); this is the only process of which there can be more than one.
- Net: running the `net` crate. That process is owned by the fetch worker in `user_agent/src/fetch.rs`, and the code in the process will essentially be the part of the fetch standard that starts at https://fetch.spec.whatwg.org/#http-network-or-cache-fetch.
- Media: running the `media` crate, which runs gstreamer, which is started and owned by the media worker in `user_agent/src/media.rs`. This is an optional feature as expalined above under Quick Start.


The goal is to follow [Apple's guidelines for an independent browser engine](https://developer.apple.com/documentation/
BrowserEngineKit/designing-your-browser-architecture).


## Project structure

| Directory | Description |
|-----------|-------------|
| [`embedder/`](./embedder/README.md) | Default embedding of the engine: top-level application lifecycle, window management, browser chrome, and redraw loop |
| [`user_agent/`](./user_agent/README.md) | All global coordination: navigables and traversables, session history, event loops, timers, fetch workers, and incoming requests from the embedder and webview layers |
| [`content/`](./content/README.md) | DOM and JS execution: HTML algorithm implementations, Boa JS integration, Web IDL bridges, typed IPC — but not coordination with other components |
| [`media/`](./media/README.md) | GStreamer-based video decoding pipeline, media process entrypoint, and IPC transport for video frames |
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
