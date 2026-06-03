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

## Window.open continuation

When the content process sends a `WindowOpenRequested` IPC event
(`user_agent/src/event_loop.rs` forwards it as `UserAgentCommand::WindowOpenRequest`),
the user agent's `handle_window_open` continues the window open algorithm
from step 13:

1. `choose_navigable` (unified rules-for-choosing that returns both the
   chosen navigable id and the windowType string).
2. Step 15: For new-traversable windowTypes, sets up the opener
   relationship (`opener_browsing_context`) on the target browsing
   context when windowType is "new and unrestricted".
3. Steps 15.4/16.1: Navigates the target navigable.

WindowProxy return value is a null placeholder on the content side — the
user agent only performs the navigation and does not need to maintain
a reference for the caller.