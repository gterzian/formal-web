# formal-web

formal-web is a Rust web-engine prototype in alpha status, with an embedding API and an optional TLA+ verification layer.

## Quick start

```bash
rustup toolchain install 1.92.0
rustup run 1.92.0 cargo run --release
```

This builds and runs the default windowed embedder for local development.

## Project structure

| Directory | Description |
|-----------|-------------|
| [`embedder/`](./embedder/README.md) | Top-level application lifecycle, window management, browser chrome, and redraw loop |
| [`user_agent/`](./user_agent/README.md) | Browser-global coordination: navigables, session history, event loops, timers, fetch workers, sidecar lifecycle |
| [`content/`](./content/README.md) | Content process: DOM and HTML algorithms, Boa JS integration, Web IDL bridges, typed IPC |
| [`net/`](./net/README.md) | Net sidecar: HTTP and file fetch execution |
| [`webview/`](./webview/README.md) | Per-webview compositor state, hit testing, and redraw signaling |
| [`automation/`](./automation/README.md) | WebDriver and CDP wire-protocol servers |
| [`verification/`](./verification/README.md) | Trace recording, TLA+ validation, and shutdown workflow |
| `ipc_messages/` | Shared IPC message types between sidecars |
| `src/` | Workspace entrypoint (`formal-web` binary) |
| [`tests/`](./tests/formal/README.md) | Formal tests and WPT runner |
| `artifacts/` | Shared-memory transport artifacts |

## Extensions

The project ships three pi coding-agent extensions for repository development:

- [**`pi-share-hf`**](.pi/extensions/pi-share-hf/README.md) — Archives pi coding sessions to `.pi/collected-sessions/` on shutdown. Includes a `collect_session` tool for mid-task checkpoints.
- [**`browser`**](.pi/extensions/browser/README.md) — Wraps formal-web's CDP server into agent-callable tools (`browser_navigate`, `browser_click`, `browser_screenshot`, etc.) for live interactive debugging.
- [**`rust-analyzer`**](.pi/extensions/rust-analyzer/README.md) — Spawns `rust-analyzer` as a child process and exposes 15 tools (`ra_hover`, `ra_diagnostics`, `ra_references`, `ra_rename`, etc.) for Rust code analysis, navigation, and refactoring.

## Pi session archiving

Pi coding sessions are archived on shutdown to `.pi/collected-sessions/`. To upload collected sessions to the Hugging Face dataset:

```bash
./sync-hf-sessions.sh
```

Prerequisites: the `hf` CLI must be installed and authenticated with write access to the [target dataset](https://huggingface.co/datasets/formal-web/pi-coding-sessions).
