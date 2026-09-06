# formal-web

formal-web is a Rust web-engine prototype with a modular architecture and support for formal verification.

## Getting Started

The project has only been run on macOS; all build commands assume macOS. The
Rust toolchain is pinned to 1.94.0 (`rustup toolchain install 1.94.0`); if it
is not your default toolchain, prefix the commands below with `rustup run 1.94.0`.

Just build and run:

```bash
cargo build --release
cargo run --release
```

This is the default configuration: **V8** as the JS engine and **AVFoundation**
(via the `media` feature) for video/audio playback. The WPT suite runs with
`cargo run --release -- wpt`.

For other configurations, the feature flags configure:

- **JS engine** — exactly one of `v8` (default), `boa`, or `jsc` must be
  enabled; enabling none (`--no-default-features` alone) or more than one
  fails the build.
- **Media** — the `media` feature (on by default) enables video/audio
  playback through a platform media backend; drop it to build without media.
- **WebAssembly** — the `wasm` feature is the Wasmtime-based WebAssembly
  implementation for the Boa engine (which has no native WebAssembly).
  V8 and JSC implement WebAssembly natively, so no feature is needed
  there.

### No media (no video/audio)

```bash
cargo build --release --no-default-features --features v8
cargo run --release --no-default-features --features v8
```

### Boa engine

```bash
cargo build --release --no-default-features --features boa,media
cargo run --release --no-default-features --features boa,media
```

### Boa + WebAssembly

```bash
cargo build --release --no-default-features --features boa,wasm,media
```

Wasmtime-based WebAssembly for the Boa engine (V8 and JSC have native
WebAssembly — no feature needed).

### JSC engine (experimental, macOS only)

```bash
cargo build --release --no-default-features --features jsc,media
cargo run --release --no-default-features --features jsc,media
```

### GStreamer media backend instead of AVFoundation

```bash
cargo build --release --features backend-gstreamer
```

The media backend is independent of the JS engine. AVFoundation (the macOS
default) keeps decoded video frames on the GPU; GStreamer delivers CPU bytes
and is the only backend on non-Apple platforms. To pair GStreamer with a
different engine, add the engine flags, e.g.
`cargo build --release --no-default-features --features boa,media,backend-gstreamer`.

### Windowed embedder (browser chrome)

The headed app's window and browser chrome come from one of two independent
embedder crates, selected at compile time:

- **macOS**: the AppKit backend (`mac-embedder`) is the default. It runs an
  `NSApplication` with native chrome (menu bar, toolbar, address field, tab
  strip) and zero-copy IOSurface presentation, and has no winit, Blitz, or
  GPU dependencies.
- **Other platforms**: the winit backend (`winit-embedder`, winit windows
  with a Blitz-rendered chrome) is the only option.

The winit windowed backend is **not compiled on macOS by default**; pass the
`winit_embedder` feature to build and select it there:

```bash
cargo build --release --features winit_embedder
cargo run --release --features winit_embedder
```

The `winit-embedder` crate also provides the **headless** app (no window, no
chrome) used by WebDriver/CDP/WPT; on macOS it builds headless-only by
default, so the AppKit app never pulls winit graphics code. The headless
build has no graphics dependencies at all — WPT and the automation servers
compile without wgpu/Blitz. See `embedder/README.md` for the crate layout.

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