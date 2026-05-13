# User Agent

The browser user-agent lives in this crate.

Keep ownership and cross-thread coordination here, split by responsibility:
- `user_agent.rs` owns the top-level user-agent state, handles, and command loop.
- `event_loop.rs` owns content event-loop threads, content sidecar processes, and task routing.
- `timer.rs` owns the timer worker.
- `fetch.rs` owns the fetch worker and the net sidecar process boundary.

Model long-running workers as stateful structs with `run(&mut self)` so thread-local state stays on the owning component instead of being spread across helper functions.

Route browser, embedder, automation, and webview requests through this crate so traversable ownership, navigation state, viewport updates, and rendering opportunities stay in one place.

Keep traversable target-name bookkeeping in this crate so iframe-host cleanup can stop the correct event loop without pushing that state into the embedder or content side.