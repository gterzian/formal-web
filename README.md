# formal-web

formal-web is a Rust web-engine prototype with a modular architecture and support for formal verification.

## Getting Started

The project has only been run on macOS; all build commands assume macOS. The
Rust toolchain is pinned to 1.94.0 (`rustup toolchain install 1.94.0`); if it
is not your default toolchain, prefix the commands below with `rustup run 1.94.0`.

### JS engine

Exactly one of the engines below is enabled at a time; enabling none or more
than one fails the build.

```bash
# V8 (default)
cargo build --release
cargo run --release

# Boa
cargo build --release --no-default-features --features boa,media
cargo run --release --no-default-features --features boa,media

# Boa + WebAssembly (`wasm` is Boa-only: V8 and JSC implement WebAssembly natively)
cargo build --release --no-default-features --features boa,wasm,media
cargo run --release --no-default-features --features boa,wasm,media

# JSC (experimental, macOS only)
cargo build --release --no-default-features --features jsc,media
cargo run --release --no-default-features --features jsc,media
```

Run the WPT suite:

```bash
cargo run --release -- wpt
```

### Media

Video/audio playback is provided by a platform media backend that runs inside
the graphics process, so backend selection is a feature of the `graphics`
build.

```bash
# AVFoundation media backend (macOS default)
cargo build --release
cargo run --release

# GStreamer media backend (macOS opt-in; on other platforms GStreamer is
# the only media backend)
cargo build --release -p graphics --features backend-gstreamer,cpu_readback
cargo run --release

# Without media
cargo build --release --no-default-features --features v8
cargo run --release --no-default-features --features v8
```

### Graphics

The surface backend is selected on the `graphics` build: the embedder spawns
the `formal-web-graphics` binary it finds next to its own executable, so
rebuild that sidecar with the chosen backend and then run normally.

```bash
# Zero-copy IOSurface (macOS default)
cargo build --release
cargo run --release

# CPU readback (macOS opt-in; on other platforms the only backend)
cargo build --release -p graphics --features cpu_readback
cargo run --release
```

### Embedder

The headed app's window and browser chrome come from one of two independent
embedder crates, selected at compile time.

```bash
# macOS: AppKit embedder (default; native chrome, zero-copy IOSurface
# presentation, no winit/Blitz/GPU dependencies)
cargo build --release
cargo run --release

# macOS: winit windowed embedder (Blitz-rendered chrome)
cargo build --release --features winit_embedder
cargo run --release --features winit_embedder

# Other platforms: the winit windowed embedder is the only option
cargo build --release
cargo run --release
```

The `winit-embedder` crate also provides the **headless** app (no window, no
chrome) used by WebDriver/CDP/WPT; on macOS it builds headless-only by
default, so the AppKit app never pulls winit graphics code. See
`embedder/README.md` for the crate layout.

## Project architecture

A multiprocess approach is chosen by default, with the goal of having the possibility to meet [Apple's guidelines for an independent browser engine](https://developer.apple.com/documentation/BrowserEngineKit/designing-your-browser-architecture). 

Besides this, a modular approach is followed by making the following components generic with swappable implementations:

- The JS engine: Boa, V8, or JSC. 
- The media engine: Gstreamer or AvFoundation.
- The IPC layer: ipc-channel or Xpc/BrowserKit.
- The networking layer (planned, for now tokio only).
- The graphics process, with two independent backends: scene delivery
  (zero-copy IOSurface on macOS by default, CPU readback via
  `-p graphics --features cpu_readback` on all platforms) and video frames
  (GPU buffers with AVFoundation, CPU bytes with GStreamer).

The following processes are used:

- **Main** (`src/main.rs`): runs the `embedder`, `webview`, and `user_agent` crates.
- **Content** (`user_agent/src/event_loops.rs`): runs the `content` crate. Multiple processes: one per [similar origin window agent](https://html.spec.whatwg.org/#similar-origin-window-agent).
- **Graphics** (`graphics/src/bin/graphics_process.rs`): runs the `graphics` and `media` crates.
- **Net** (`user_agent/src/fetch.rs`): runs the `net` crate.

## Formal verification

A set of core algorithms will be formalized using TLA+, and their Rust implementation model-checked against those formal specification using the tracing approach described in [Validating Traces of Distributed Programs Against TLA+ Specifications](https://arxiv.org/abs/2404.16075). For further details, see [the verification folder](verification/README.md).

## Pi coding agent extensions

The project is build using the Pi agent, and comes with a a few extensions to it.

Pi automatically discovers extensions in `.pi/extensions/` (one level deep, each
directory containing an `index.ts` or a `package.json` with a `pi.extensions`
field). The extensions are plain TypeScript with their own npm dependencies.

### Setup

`node_modules/` is git-ignored, so a fresh checkout must install the npm
dependencies for each extension before pi can load it. From the repository root:

```bash
cd .pi/extensions/browser && npm ci && cd ../../..
cd .pi/extensions/web_standards && npm ci && cd ../../..
```

After this, restart pi in the repository directory (or reload extensions if you
are already in a session) and the tools and commands below become available.
If an extension fails to load with `Cannot find module 'ws'` or
`Cannot find module 'cheerio'`, the npm install step above was skipped.

### Extensions

- [**`browser`**](.pi/extensions/browser/README.md) — browser automation for
  testing. Depends on [`ws`](https://www.npmjs.com/package/ws) for its WebSocket
  CDP client. Connect it to formal-web's CDP server (`/browser-connect <port>`)
  to drive live debugging sessions; the extension also works with standard
  Chrome/Chromium instances.
- [**`web_standards`**](.pi/extensions/web_standards/README.md) — interactive
  spec reading (`spec_lookup`, `spec_ref_links`, `spec_search_id`). Depends on
  [`cheerio`](https://www.npmjs.com/package/cheerio) for server-side HTML
  parsing and traversal of WHATWG/W3C spec documents.
- [**`readme-chain`**](.pi/extensions/readme-chain/README.md) — walks the
  AGENTS.md/README.md documentation chain for a path; no npm dependencies.