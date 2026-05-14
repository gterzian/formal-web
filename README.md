# formal-web

formal-web is a Rust browser prototype with explicit user-agent, event-loop, timer, fetch, content, and net components. The executable browser and its coordination logic live entirely in Rust, and the long-term verification direction is TLA+ trace checking over those Rust state transitions.

---

![formal-web architecture](formal-web-diagram.svg)

---

## The problem

Some of the hardest bugs in browsers are in the coordination. Navigation races, session history corruption, fetch ordering errors, and timer ordering mistakes are concurrency bugs.

## The approach

formal-web keeps navigation, session-history, timer, fetch, and event-loop coordination as explicit Rust state machines with direct links back to the relevant standards. The TLA+ models under `tla_specs/` are the remaining formal artifacts in-tree, and a trace-based verification workflow for the Rust runtime is planned on top of those models.

---

## Four pillars

### Correctness

The most complicated concurrent algorithms live in explicit Rust worker threads and state machines. The implementation is documented against the HTML, Fetch, DOM, Streams, and Web IDL standards, with local copies of those standards checked into `web_standards/`.

### Performance

Perceived performance is about latency, not throughput. formal-web keeps the main render path unblocked, and heavy browser coordination work runs in dedicated threads or sidecars.

### Modularity

The engine composes best-in-class open components:

- **Blitz** — DOM and layout
- **Vello** — GPU-accelerated rendering
- **Wasmtime** — WebAssembly runtime
- **Boa** — JavaScript engine

The Blitz + Vello + anyrender pipeline is naturally composable, supporting advanced use cases like cross-process iframes and media elements with minimal coordination overhead. Anything beyond core web standards is implemented as an explicit Rust module instead of hidden runtime wiring.

### Security

The process model is designed to meet Apple's [architectural requirements](https://developer.apple.com/documentation/browserenginekit/designing-your-browser-architecture) for an independent web engine on iOS:

- **Content processes** (one per tab) — DOM, JavaScript, display-list production
- **Network process** — fetch, TLS
- **GPU process** — Vello rendering (currently still in the main process)
- **Main process** — browser chrome, embedder, and worker bootstrap

---

## The bet

Composable Rust modules plus protocol-level TLA+ verification is a tractable path to a correct, secure, and performant web engine.

## Requirements

- `rustup`
- Rust toolchain `1.92.0`: `rustup toolchain install 1.92.0`
- `python3`
- `curl`
- On macOS, Xcode and a current macOS SDK

## Commands

```bash
rustup run 1.92.0 cargo run --release
```

`cargo check` builds the Rust crates that make up the embedder, user agent, content sidecar mode, and net sidecar mode.

`cargo run --release` starts the embedder, the user-agent thread, and the hidden content/net sidecar modes that the main executable respawns on demand, then loads `artifacts/StartupExample.html`.

```bash
rustup run 1.92.0 cargo run -- test-wpt formal/load-event-fires.html
```

`cargo run -- test-wpt` runs the current WPT runner. The parent process mounts `tests/formal/tests/` through the `/__formal__/` alias, starts a bundled `vendor/wpt/wpt serve` instance, and reuses one `formal-web webdriver` child and WebDriver session across the run in headless mode by default. The runner launches the browser child from the release build unless `--debug-build` is passed. Pass `--headed` when you want to watch the page.

The runner starts one `wpt serve` instance for the run, keeps one shared browser session for sequential test navigation, and uses `common/blank.html` between tests before loading the next test URL. If the browser session crashes, the runner recreates that session and retries the current test once while keeping the same `wpt serve` process alive. Before each run it also clears stale recorded `wpt serve` process IDs from previous interrupted runs. The runner collects `testharness.js` completion through WebDriver script execution with a rendered-summary fallback from `testharnessreport.js`.

For `.any.js` files, the runner currently serves the `.any.html` variant and executes the plain window form. Worker, shared-worker, and service-worker variants are still left out of the default runner selection.

The runner writes generated `wpt serve` config and injection files under `scratchpad/wpt-runner/runtime/` and removes them after each test. When machine-readable output is needed, point `--output` at a path under `scratchpad/wpt-runner/reports/`.

Without a path it uses both `tests/wpt/include.ini` and `tests/formal/include.ini`. With `--list` it prints the selected tests without launching the embedder. Explicit paths can point at the upstream WPT tree or at the local suite through the `formal/` prefix.

## Verification direction

The TLA+ specifications live under `tla_specs/`. The next verification step is a trace-based workflow that compares Rust runtime behavior against those protocol models.