# User Agent

The browser user-agent lives in this crate.

Keep ownership and cross-thread coordination here, split by responsibility:
- `user_agent.rs` owns the top-level user-agent state, handles, and command loop.
- `event_loop.rs` owns content event-loop threads, content sidecar processes, and task routing.
- `timer.rs` owns the timer worker.
- `fetch.rs` owns the fetch worker and the net sidecar process boundary.

Model long-running workers as stateful structs with `run(&mut self)` so thread-local state stays on the owning component instead of being spread across helper functions.

Implement each `UserAgentCommand` branch as a dedicated `handle_*` method on `UserAgentWorker`, and keep spec-facing algorithms such as `create_agent`, `create_new_top_level_traversable`, `create_navigation_params_by_fetching`, and `finalize_cross_document_navigation` as named worker methods instead of free helper functions.

Inline single-use sidecar spawn setup at the `Command::new(...)` site and give first-party user-agent threads and sidecar processes stable names where they are created.

Route browser, embedder, automation, and webview requests through this crate so traversable ownership, navigation state, viewport updates, and rendering opportunities stay in one place.

Keep traversable target-name bookkeeping in this crate so iframe-host cleanup can stop the correct event loop without pushing that state into the embedder or content side.

Keep the spec-facing browser-global concepts in `UserAgentState` itself: the browsing-context-group set, the top-level traversable set, allocator state, and the pending navigation/fetch continuations. Treat helper hash maps in that file as model-local indices derived from those concepts rather than as replacements for them.

Keep pending navigation state as explicit spec-facing request, snapshot, history-entry, and history-handling records so finalization can follow HTML's `push` versus `replace` session-history steps without flattening those concepts into ad hoc fields.