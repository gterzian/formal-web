# net crate

The `net` crate owns the `formal-web-net` entrypoint and executes fetch requests on behalf of the user-agent fetch worker.

- Launches the dedicated net process and performs typed IPC bootstrap.
- Executes file and HTTP fetches and returns typed responses.
- Keeps network work behind a separate process boundary.
- Will host HTTP cache logic when the Fetch spec reaches that layer.
