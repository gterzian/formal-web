# net crate

Fetch sidecar entrypoints and request execution for the user-agent fetch worker.

## Responsibility

The `net` crate owns:
- Hidden `--net-token` sidecar startup for the shared `formal-web` executable
- File and HTTP fetch execution for fetch worker requests
- IPC bootstrap and typed request/response handling for fetch completions

## Design Notes

- The root `formal-web` executable hosts the `net` sidecar mode directly so `cargo run --release` builds one runnable binary without a staging build script.
- The fetch worker still treats networking as a separate process boundary; the `net` crate provides the sidecar entrypoint and request loop for that boundary.
