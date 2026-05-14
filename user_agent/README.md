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

When a cross-document navigation runs `beforeunload`, fan it out across the traversable's inclusive descendant tree instead of stopping at the active document, and queue a rendering opportunity after navigation commit so history traversal repaints without relying on incidental input.

Keep traversable target-name bookkeeping in this crate so iframe-host cleanup can stop the correct event loop without pushing that state into the embedder or content side.

Keep the spec-facing browser-global concepts in `UserAgentState` itself: the browsing-context-group set, the top-level traversable set, allocator state, and the pending navigation/fetch continuations. Treat helper hash maps in that file as model-local indices derived from those concepts rather than as replacements for them.

Keep pending navigation state as explicit spec-facing request, snapshot, history-entry, and history-handling records so finalization can follow HTML's `push` versus `replace` session-history steps without flattening those concepts into ad hoc fields.

Use distributed `RuntimeId` UUIDs for fetch controllers, document-fetch handlers, and timer keys so worker-owned components can allocate those ids locally without blocking on user-agent refill traffic. Keep `FrameId(u64)` as the compositor-facing iframe host identity, and reuse that frame id as the synthetic iframe traversable id when the user agent materializes `_iframe|...` helper traversables.

When `user_agent` code implements or continues a standards algorithm, follow the documentation conventions from `content/README.md`: anchor-only top doc-comments for the algorithm, verbatim `Step n:` comments for mapped steps, and separate `Notes:` comments for reduced-model or runtime-bridge explanation.

For spec-facing worker methods and continuations, give each helper its own anchor-only doc comment for the algorithm it continues, keep `Step n:` comments as verbatim standard prose, and move reduced-model or runtime-bridge explanation into separate `Notes:` comments.

Avoid introducing synchronous user-agent command bridges that block on content replies unless the corresponding standard algorithm has an explicit wait point; prefer queueing work and resuming through existing continuation events.

Name user-agent command variants and worker methods after the standard algorithm or continuation they trigger (for example `Navigate`, `CompleteBeforeUnload`, and `FinalizeCrossDocumentNavigation`) instead of transport-oriented `Queue*` names, and keep the `navigate` entrypoint typed in terms of a navigable id that resolves to a traversable when the target is traversable-backed.