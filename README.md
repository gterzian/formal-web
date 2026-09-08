# formal-web

formal-web is a Rust web-engine prototype with a modular architecture and support for formal verification.

The modularity is goal orientated: it can be used to either support shipping on a platform with specific constraints (like using extensions for [BrowserEngineKit](https://developer.apple.com/documentation/browserenginekit)), support 
platform specific performance (like integrating with Core Animation based compositing on Mac), introduce flexibility (like the ability to choose a JS engine), or simply to reduce binary size and build time by re-using what is already on the system.

## Getting Started

The project has only been run on macOS; all build commands assume macOS. The
Rust toolchain is pinned to 1.94.0 (`rustup toolchain install 1.94.0`); if it
is not your default toolchain, prefix the commands below with `rustup run 1.94.0`.

### Build and run on Mac OS

```bash
# Default: V8, media on, AppKit embedder, AVFoundation media
# backend and zero-copy IOSurface graphics.
cargo build --release
cargo run --release
```

### JS engine

Exactly one of the engines below is enabled at a time; enabling none or more
than one fails the build. V8 is the default and needs no feature flags; the
others replace it:

```bash
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

### Media and graphics backends (selected on the `graphics` build)

```bash
# Defaults (macOS): AVFoundation media backend and zero-copy IOSurface
# graphics — the default build above

# GStreamer media backend + CPU readback graphics (macOS opt-in; on other
# platforms GStreamer and CPU readback are the only backends)
cargo build --release -p graphics --features backend-gstreamer,cpu_readback
cargo run --release

# CPU readback graphics (macOS opt-in), with the default AVFoundation media
# backend
cargo build --release -p graphics --features cpu_readback
cargo run --release

# Without media
cargo build --release --no-default-features --features v8
cargo run --release --no-default-features --features v8
```

### Embedder

```bash
# macOS: AppKit embedder — the default build above

# macOS: winit windowed embedder (Blitz-rendered chrome)
cargo build --release --features winit_embedder
cargo run --release --features winit_embedder
```

## Project architecture

The following components, mapping to processes or extensions, are used:

- **Main** (`src/main.rs`): runs the `embedder`, `webview`, and `user_agent` crates.
- **Content** (`user_agent/src/event_loops.rs`): runs the `content` crate; one per [similar origin window agent](https://html.spec.whatwg.org/#similar-origin-window-agent).
- **Graphics** (`graphics/src/bin/graphics_process.rs`): runs the `graphics` and `media` crates.
- **Net** (`user_agent/src/fetch.rs`): runs the `net` crate.

## Formal verification

A set of core algorithms will be formalized using TLA+, and their Rust implementation model-checked against those formal specification using the tracing approach described in [Validating Traces of Distributed Programs Against TLA+ Specifications](https://arxiv.org/abs/2404.16075). For further details, see [the verification folder](verification/README.md).
