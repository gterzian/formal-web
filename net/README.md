# net crate

The `net` crate owns the `formal-web-net` entrypoint and executes fetch requests on behalf of the user-agent fetch worker.

- Launches the dedicated net process and performs typed IPC bootstrap.
- Executes file and HTTP fetches and returns typed responses.
- Keeps network work behind a separate process boundary.
- Will host HTTP cache logic when the Fetch spec reaches that layer.

## Modular network backends

Remaining work: the URLSession backend never completes a fetch of an `ftp:`
URL (observed on macOS with `workers/Worker_cross_origin_security_err.htm`,
disabled in `tests/wpt/meta/workers/`), where the fetch should fail.

Network work is hidden behind the `NetworkBackend` trait
(`net/src/backend/`), whose single method, `http_network_or_cache_fetch`,
maps coarsely to the fetch spec's
[HTTP-network-or-cache fetch](https://fetch.spec.whatwg.org/#http-network-or-cache-fetch).
Each build feature provides one concrete backend:

| Feature | Backend | Notes |
|---|---|---|
| `url_session` | `UrlSessionBackend` (`backend/url_session.rs`) | Apple URLSession, macOS/iOS only; wins over `tokio` when both are enabled |
| `tokio` | `TokioBackend` (`backend/tokio.rs`) | reqwest blocking client |

On Apple platforms the URLSession backend is the default: `build.rs` emits
the `url_session_default` cfg when no backend feature is enabled, so a
plain `cargo build` compiles no reqwest/tokio stack there. On non-Apple
platforms the tokio backend is the only option and is always compiled. The
tokio backend on Apple is opt-in via `--features tokio` (the URLSession
feature flag itself is a marker — the `url_session_sys` crate is always
compiled on Apple).

The root build prebuilds `formal-web-net` with the platform default backend
(see `build.rs`).

### Session partitioning by event loop

Each backend keeps one session per
[network partition key](https://fetch.spec.whatwg.org/#network-partition-key),
which is modelled as the event loop id that travels on every fetch message:
the tokio backend stores one reqwest client per key, the URLSession backend
stores one session (with no shared cache) per key. Sessions are created on
first use and reused.

### Response routing lives in the main loop

The backends deliver fetch outcomes on a plain crossbeam reply channel: each
`http_network_or_cache_fetch` call takes a clone of the process-wide
`FetchReplySender` (`backend/mod.rs`) and sends `(request_id, result)` on it.
The main loop (`main.rs`) stores the `reply_to` recipient of every in-flight
request in a pending map, `select!`s over the request receiver and the reply
receiver, and routes each delivered outcome to its recipient: the one-off
content command channel embedded in `ResponseRecipient::ContentProcess`, or
the persistent net→UA channel — the sender end of the net process's own
bootstrap connection — for `ResponseRecipient::UserAgent`. Because routing
lives in the main loop, a backend may deliver a reply at any time after its
fetch method returns: the tokio backend sends it before returning, the
URLSession backend sends it from the data task's completion handler (a
background queue).

## url_session_sys

`net/url_session_sys` is the Apple URLSession client: a safe Rust API (one
session with no shared cache, fetch completions delivered on a background
queue) over a private Objective-C FFI layer. The raw FFI declarations
(`src/ffi.rs`) and the C/ObjC wrapper (`src/url_session_wrapper.m`, compiled
via `cc`, exposing the small C API `fw_url_session_create/fetch/release`)
are crate-private; the public surface — `UrlSession::new`, `UrlSession::fetch`,
and the `FetchResponse` type — is safe. Apple targets only.

## WebSocket connections (`websocket.rs`)

One thread per connection over tungstenite, polling the socket with a short
read timeout so content's send and close requests interleave with the peer's
messages. Remaining work: the opening handshake has none of the fetch
integration the WebSockets Standard gives it (no CSP, no cookies, no proxy),
and a `wss:` server is trusted through the platform certificate store only.
