# user_agent crate

The `user_agent` crate owns browser-global coordination: navigables and traversables, navigation and session history, event loops, timers, fetch workers, sidecar lifecycle, and requests coming from the embedder and webview layers.

- `user_agent.rs` owns the top-level user-agent state and command loop.
- `event_loop.rs` owns content event loops and content sidecars.
- `timer.rs` owns the timer worker.
- `fetch.rs` owns the fetch worker and the net sidecar boundary.
- Model long-running workers as stateful structs with explicit `run` loops.
- Key cross-worker ownership with UUID newtypes such as `EventLoopId`, `NavigableId`, and related ids from `ipc_messages`.
- Keep spec-facing algorithms and continuations as named worker methods on the owning type instead of as transport-oriented helper functions.
- Route browser, embedder, automation, and webview requests through this crate instead of through synchronous cross-thread bridges.