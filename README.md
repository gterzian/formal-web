# formal-web

formal-web is a Rust browser prototype with an embedder process for window chrome, a user-agent crate that owns navigation, event-loop, timer, and fetch coordination, a dedicated content sidecar for DOM, HTML, and JavaScript runtime work, a dedicated net sidecar for fetch execution, and an optional verification layer that checks recorded runtime traces against the TLA+ specs under `verification/tla_specs/`.

## Commands

- `rustup toolchain install 1.92.0` installs the pinned Rust toolchain.
- `rustup run 1.92.0 cargo check` type-checks the workspace.
- `rustup run 1.92.0 cargo run --release` builds and runs the browser with release binaries.
- `rustup run 1.92.0 cargo run --release -- --verify` runs the browser with trace recording and shutdown-time TLA+ validation.
- `./verification/verify-navigation.sh` runs the headless navigation verification flow end to end.
- `rustup run 1.92.0 cargo run -- test-wpt` runs the default WPT and local formal test selection.
- `rustup run 1.92.0 cargo run -- test-wpt formal/load-event-fires.html` runs one selected test.
- `rustup run 1.92.0 cargo run -- validate-tla --logs /path/to/logs --json` validates a saved trace log directory.