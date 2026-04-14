# formal-web

formal-web runs a verified Lean kernel dealing with all engine-wide coordination(navigation, session history, etc...), coupled with individual Rust modules for things like running HTML event-loop tasks, managing the DOM, a winit embedder, a window renderer, and other things interfacing with the outside world.

## Requirements

- `elan`
- `rustup`
- Rust toolchain `1.92.0`: `rustup toolchain install 1.92.0`
- `curl`
- On macOS, Xcode and the macOS SDK at the path referenced in `lakefile.lean`

## Commands

```bash
rustup run 1.92.0 cargo run --release
```

`cargo check` builds the Rust workspace and the Lean runtime artifacts needed by the FFI layer.

`cargo run --release` starts the Rust embedder, initializes the Lean runtime, starts the Lean kernel, and loads `artifacts/StartupExample.html`.

The repository is still a normal Lake project. `lake build`, Lean LSP, and `lean-lsp-mcp` continue to work against the same sources, including the proof files under `FormalWeb/Proofs`.