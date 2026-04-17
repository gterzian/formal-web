# formal-web

formal-web runs a verified Lean kernel dealing with all engine-wide coordination(navigation, session history, etc...), coupled with individual Rust modules for things like running HTML event-loop tasks, managing the DOM, a winit embedder, a window renderer, and other things interfacing with the outside world.

## Requirements

- `elan`
- `rustup`
- Rust toolchain `1.92.0`: `rustup toolchain install 1.92.0`
- `python3`
- `curl`
- On macOS, Xcode and the macOS SDK at the path referenced in `lakefile.lean`

## Commands

```bash
rustup run 1.92.0 cargo run --release
```

`cargo check` builds the Rust workspace and the Lean runtime artifacts needed by the FFI layer.

`cargo run --release` starts the Rust embedder, initializes the Lean runtime, starts the Lean kernel, and loads `artifacts/StartupExample.html`.

```bash
rustup run 1.92.0 cargo run -- test-wpt formal/load-event-fires.html
```

`cargo run -- test-wpt` runs the current WPT runner. The parent process mounts `tests/formal/tests/` through the `/__formal__/` alias, starts a fresh bundled `vendor/wpt/wpt serve` instance per test for isolation, and spawns a fresh `formal-web webdriver` child per test in headless mode by default. Pass `--headed` when you want to watch the page.

Each WebDriver child starts on the test URL instead of the startup artifact page, and the runner collects `testharness.js` completion through WebDriver script execution with a rendered-summary fallback from `testharnessreport.js`.

Without a path it uses both `tests/wpt/include.ini` and `tests/formal/include.ini`. With `--list` it prints the selected tests without launching the embedder. Explicit paths can point at the upstream WPT tree or at the local suite through the `formal/` prefix.

```bash
rustup run 1.92.0 cargo run -- test-wpt captured-mouse-events/captured-mouse-event-constructor.html
```

The repository is still a normal Lake project. `lake build`, Lean LSP, and `lean-lsp-mcp` continue to work against the same sources, including the proof files under `FormalWeb/Proofs`.