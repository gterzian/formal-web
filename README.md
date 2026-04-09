# formal-web

formal-web keeps the embedder and content processes in Rust and the engine-wide coordination logic in Lean. The Rust entry point starts the Lean kernel, spawns the content process, and drives the windowed runtime.

## Requirements

- `elan`
- `rustup`
- Rust toolchain `1.92.0`: `rustup toolchain install 1.92.0`
- `curl`
- On macOS, Xcode and the macOS SDK at the path referenced in `lakefile.lean`

## Commands

```bash
rustup run 1.92.0 cargo check
rustup run 1.92.0 cargo build --release
rustup run 1.92.0 cargo run --release
rustup run 1.92.0 cargo run --release -- test-wpt html/interaction/focus/focus-01.html
```

`cargo check` builds the Rust workspace and the Lean runtime artifacts needed by the FFI layer.

`cargo run --release` starts the Rust embedder, initializes the Lean runtime, starts the Lean kernel, and loads `artifacts/StartupExample.html`.

The repository is still a normal Lake project. `lake build`, Lean LSP, and `lean-lsp-mcp` continue to work against the same sources, including the proof files under `FormalWeb/Proofs`.

## WPT

`cargo run --release -- test-wpt PATH` opens a single WPT file in the engine window. HTML files load directly. JavaScript test files load through a generated harness page. Close the window manually when you are done.