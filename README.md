# formal-web

formal-web is a Rust browser prototype with explicit user-agent, event-loop, timer, fetch, content, and net components. The `FormalWeb` Lean modules remain in-tree as reference models and proofs, while the executable runtime is now implemented in Rust.

---

![formal-web architecture](formal-web-diagram.svg)

---

## The problem

Some of the hardest bugs in browsers are in the coordination. Navigation races, session history corruption, fetch ordering errors. These are concurrency bugs.

## The approach

formal-web keeps the Rust runtime structurally close to the `FormalWeb` reference models. The goal is an idiomatic Rust implementation of the same navigation, session-history, timer, and fetch business logic, with the Lean files serving as specifications and proof artifacts rather than the live runtime.

---

## Four pillars

### Correctness

The most complicated concurrent algorithms — navigation, session history, fetch, timers, and event-loop coordination — live in explicit Rust state machines and worker threads. The Lean modules remain available as reference material for the intended business logic and invariants.

### Performance

Perceived performance is about latency, not throughput. formal-web ensures the main render path is never blocked. Even with a modest JS engine like Boa, the browser stays snappy. 

### Modularity

The engine composes best-in-class open components:

- **Blitz** — DOM and layout
- **Vello** — GPU-accelerated rendering
- **Wasmtime** — WebAssembly runtime
- **Boa** — JavaScript engine

The Blitz + Vello + anyrender pipeline is naturally composable, supporting advanced use cases like cross-process iframes and media elements with minimal coordination overhead. Anything beyond core web standards — WebNN, WebBluetooth, Web MIDI — is implemented as a plain Rust module, keeping the extension surface explicit.

### Security

The process model is designed to meet Apple's [architectural requirements](https://developer.apple.com/documentation/browserenginekit/designing-your-browser-architecture) for an independent web engine on iOS — from day one:

- **Content processes** (one per tab) — DOM, JavaScript, display list production
- **Network process** — fetch, TLS
- **GPU process** — Vello rendering (note: this happens in the main process for now)
- **Main process** — browser chrome, embedder
---

## The bet

A formally verified core plus composable Rust modules is a tractable path to a correct, secure, and performant web engine.

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

`cargo check` builds the Rust crates that make up the embedder, user agent, content sidecar mode, and net sidecar mode.

`cargo run --release` starts the Rust embedder, the user-agent thread, and the hidden content/net sidecar modes that the main executable respawns on demand, then loads `artifacts/StartupExample.html`.

```bash
rustup run 1.92.0 cargo run -- test-wpt formal/load-event-fires.html
```

`cargo run -- test-wpt` runs the current WPT runner. The parent process mounts `tests/formal/tests/` through the `/__formal__/` alias, starts a bundled `vendor/wpt/wpt serve` instance, and reuses one `formal-web webdriver` child and WebDriver session across the run in headless mode by default. The runner launches the browser child from the release build unless `--debug-build` is passed. Pass `--headed` when you want to watch the page.

The runner starts one `wpt serve` instance for the run, keeps one shared browser session for sequential test navigation, and uses `common/blank.html` between tests before loading the next test URL. If the browser session crashes, the runner recreates that session and retries the current test once while keeping the same `wpt serve` process alive. Before each run it also clears stale recorded `wpt serve` process IDs from previous interrupted runs. The runner collects `testharness.js` completion through WebDriver script execution with a rendered-summary fallback from `testharnessreport.js`.

For `.any.js` files, the runner currently serves the `.any.html` variant and executes the plain window form. Worker, shared-worker, and service-worker variants are still left out of the default runner selection.

The runner writes generated `wpt serve` config and injection files under `scratchpad/wpt-runner/runtime/` and removes them after each test. When machine-readable output is needed, point `--output` at a path under `scratchpad/wpt-runner/reports/`.

Without a path it uses both `tests/wpt/include.ini` and `tests/formal/include.ini`. With `--list` it prints the selected tests without launching the embedder. Explicit paths can point at the upstream WPT tree or at the local suite through the `formal/` prefix.

```bash
rustup run 1.92.0 cargo run -- test-wpt captured-mouse-events/captured-mouse-event-constructor.html
```

The repository is still a normal Lake project. `lake build`, Lean LSP, and `lean-lsp-mcp` continue to work against the same sources, including the proof files under `FormalWeb/Proofs`.