# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API, and an optional verification layer that checks recorded execution traces against the TLA+ specs.

## Commands

- `rustup toolchain install 1.92.0` installs the pinned Rust toolchain.
- `rustup run 1.92.0 cargo check` type-checks the workspace.
- `rustup run 1.92.0 cargo run --release` builds and runs the default windowed embedder for repository development.
- `rustup run 1.92.0 cargo run --release -- --headless` runs the same entrypoint in headless mode.
- `rustup run 1.92.0 cargo run --release -- --verify` runs with trace recording and shutdown-time TLA+ validation.
- `rustup run 1.92.0 cargo run --release -- webdriver --headless` runs the WebDriver server using the repository entrypoint.
- `rustup run 1.92.0 cargo run --release -- cdp --headless` runs the CDP server for CDP-native tooling.
- `rustup run 1.92.0 cargo run --release -- webdriver --headless --cdp-port 9222` runs WebDriver and CDP together in one embedder process.
- `rustup run 1.92.0 cargo run --release -- wpt` runs the default WPT and local formal test selection from the repository entrypoint.
- `rustup run 1.92.0 cargo run --release -- wpt formal/load-event-fires.html` runs one selected WPT/formal test from the repository entrypoint.
- `./verification/verify-navigation.sh` runs the headless navigation workflow whose acceptance target is the shutdown-time TLA+ `Navigation` check.
- `./verification/verify-rendering.sh` runs the headless screenshot-based rendering workflow for the startup artifact and its cross-origin iframe.
- `rustup run 1.92.0 cargo run -- validate-tla --logs /path/to/logs --json` validates a saved trace log directory via the root validation entrypoint.

## Pi Coding Sessions

Pi (the coding agent used to develop this repository) can archive session traces
for reproducibility and review. The local infrastructure consists of three parts:

- **[pi-share-hf extension](.pi/extensions/pi-share-hf/)** — A pi extension that
automatically collects session data to `.pi/collected-sessions/` on shutdown.
It also exposes a `collect_session` tool and `/collect-session` command for
mid-task checkpoints.

- **[`sync-hf-sessions.sh`](./sync-hf-sessions.sh)** — Uploads collected
sessions to the Hugging Face dataset and clears the local directory. Run it
whenever you want to push accumulated sessions upstream:

  ```bash
  ./sync-hf-sessions.sh
  ```

  Prerequisites: the `hf` CLI must be installed and authenticated, and you need
  write access to the target dataset.

- **[Hugging Face dataset](https://huggingface.co/datasets/formal-web/pi-coding-sessions)**
— Remote destination for archived sessions. Pull requests to this dataset are
created automatically by `sync-hf-sessions.sh`.
