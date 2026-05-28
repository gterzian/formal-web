# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API, and an optional verification layer that checks recorded runtime traces against the TLA+ specs.

## Commands

- `rustup toolchain install 1.92.0` installs the pinned Rust toolchain.
- `rustup run 1.92.0 cargo check` type-checks the workspace.
- `rustup run 1.92.0 cargo run --release` builds and runs the default windowed embedder for repository development.
- `rustup run 1.92.0 cargo run --release -- --headless` runs the same entrypoint in headless mode.
- `rustup run 1.92.0 cargo run --release -- --verify` runs with trace recording and shutdown-time TLA+ validation.
- `rustup run 1.92.0 cargo run --release -- webdriver --headless` runs the WebDriver server using the repository entrypoint.
- `rustup run 1.92.0 cargo run --release -- cdp --headless` runs the CDP server for CDP-native tooling.
- `rustup run 1.92.0 cargo run --release -- webdriver --headless --cdp-port 9222` runs WebDriver and CDP together on one embedder runtime.
- `./verification/run-cdp-startup-feature-check.sh` runs the Rust-native external CDP startup-artifact feature checks.
- `rustup run 1.92.0 cargo run --release -- wpt` runs the default WPT and local formal test selection from the repository entrypoint.
- `rustup run 1.92.0 cargo run --release -- wpt formal/load-event-fires.html` runs one selected WPT/formal test from the repository entrypoint.
- `./verification/verify-navigation.sh` runs the headless navigation workflow whose acceptance target is the shutdown-time TLA+ `Navigation` check.
- `./verification/verify-rendering.sh` runs the headless screenshot-based rendering workflow for the startup artifact and its cross-origin iframe.
- `rustup run 1.92.0 cargo run -- validate-tla --logs /path/to/logs --json` validates a saved trace log directory via the root validation entrypoint.
