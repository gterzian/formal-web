# formal-web

formal-web is a Rust web-engine prototype with a modular architecture and support for formal verification.

The modularity is oriented to support the following goals:

- **external constraint satisfaction**: shipping on a platform with specific constraints. Example: the ipc layer defaults to Rust multiprocessing, but is also designed to in the future support extensions in the context of [BrowserEngineKit](https://developer.apple.com/documentation/browserenginekit).
- **performance optimization**: platform specific performance. Example: integrating with Core Animation based compositing on Mac.
- **engineering flexibility**: subsystem swapping. For example, one can choose a JS engine such as V8 or Boa, and with Boa, one can also choose to add wasm via Wasmtime (this wasm layer itself is not generic as of now, but could be).
- **cost reduction**: reduce binary size or build time by re-using what is already on the system. Example: choosing the url-session networking backend on Mac.

Note: the current set of implementations of generic components reflect Mac OS being the main development platform: high-performance Mac OS paths and lower-performance cross platform paths. For example, there is a relatively high-performance rendering path on Mac OS, with zero copy texture sharing and a modicum of layering using multiple Core Animation layers to minimize re-rendering, and then there is a relatively low-performance cross platform path involving reading back data to the CPU.

## Getting Started

The project has only been run on macOS; all build commands assume macOS. The
Rust toolchain is pinned to 1.94.0 (`rustup toolchain install 1.94.0`); if it
is not your default toolchain, prefix the commands below with `rustup run 1.94.0`.

### Build and run with default features on Mac OS

```bash
# Default: V8, media on, AppKit embedder, AVFoundation media
# backend and zero-copy IOSurface graphics.
cargo build --release
cargo run --release
```

### Choose a JS engine

Exactly one of the engines below is enabled at a time; enabling none or more
than one fails the build. V8 is the default and needs no feature flags; the
others replace it:

```bash
# Default (macOS): V8

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

### Choose a media and graphics backends (selected on the `graphics` build)

```bash
# Defaults (macOS): AVFoundation media backend and zero-copy IOSurface
# graphics.

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

### Choose a networking stack(selected on the `net` build)

The fetch transport is one of two backends. macOS defaults to the Apple
URLSession backend, which compiles no reqwest/tokio stack; on other
platforms the tokio/reqwest backend is the only option and is always
compiled:

```bash
# Default (macOS): Apple URLSession.

# tokio/reqwest backend
cargo build --release -p net --features tokio
cargo run --release
```

A full `cargo build --release` prebuilds `formal-web-net` with the platform
default backend and overwrites the copy it places next to the embedder
binary, so build the `net` package after it when switching backends. See
`net/README.md` for the backends themselves.

### Choose an embedder app

```bash
# Default (macOS): AppKit embedder

# winit windowed embedder (Blitz-rendered chrome)
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
