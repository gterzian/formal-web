# net crate

Fetch sidecar entrypoints and request execution for the user-agent fetch worker.

## Responsibility

The `net` crate owns:
- `--net-token` sidecar startup for the dedicated `formal-web-net` executable
- File and HTTP fetch execution for fetch worker requests
- IPC bootstrap and typed request/response handling for fetch completions

## Design Notes

- The top-level package builds `formal-web`, `formal-web-content`, and `formal-web-net` in one Cargo build, and the fetch worker launches `formal-web-net` directly.
- The fetch worker still treats networking as a separate process boundary; the `net` crate provides the sidecar entrypoint and request loop for that boundary.
