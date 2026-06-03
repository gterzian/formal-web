# user_agent crate

The `user_agent` crate owns all browser-global coordination: navigables and traversables, navigation and session history, event loops, timers, fetch workers, content-process lifecycle, and requests coming from the embedder and webview layers.

- `user_agent.rs` owns the top-level user-agent state and command loop.
- `event_loop.rs` owns content event loops and manages the content process.
- `timer.rs` owns the timer worker.
- `fetch.rs` owns the fetch worker and the net process boundary.
- Model long-running workers as stateful structs with explicit `run` loops.
- Key cross-worker ownership with UUID newtypes such as `EventLoopId`, `NavigableId`, and related ids from `ipc_messages`.
- Keep spec-facing algorithms and continuations as named worker methods on the owning type instead of as transport-oriented helper functions.
- Route browser, embedder, automation, and webview requests through this crate instead of through synchronous cross-thread bridges.

## Window.open flow

`window.open()` goes through the shared `navigate` path. The content process
resolves the easy cases (`_self`) directly and sends a `NavigateRequest` IPC
with `features_json` set and an optional `chosen_navigable_id`. The user agent:

1. When `chosen_navigable_id` is `Some`, uses it directly.
2. When `chosen_navigable_id` is `None`, runs the remaining rules-for-choosing
   steps: find-by-target-name (cross-process), or create a new top-level
   traversable.
3. Notifies the embedder to open a new tab for new top-level traversables.
4. Sets up the opener relationship (`opener_browsing_context`) for
   `"new and unrestricted"` window types (step 15.3 of window-open-steps).
5. Navigates the target navigable.

WindowProxy return value is a null placeholder on the content side — the
user agent only performs the navigation and does not need to maintain
a reference for the caller.
