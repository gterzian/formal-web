# User Agent

The browser user-agent lives in this crate.

Keep ownership and cross-thread coordination here, split by responsibility:
- `user_agent.rs` owns the top-level user-agent state, event-loop ownership indices, and command loop.
- `event_loop.rs` owns content event-loop threads, content sidecar processes, and task routing.
- `timer.rs` owns the timer worker.
- `fetch.rs` owns the fetch worker and the net sidecar process boundary.

Model long-running workers as stateful structs with `run(&mut self)` so thread-local state stays on the owning component instead of being spread across helper functions.

Key event-loop ownership directly by `EventLoopId` and keep traversable-to-owner indices in terms of those UUID ids; avoid process-local integer handle allocators for cross-worker routing.

Implement each `UserAgentCommand` branch as a dedicated `handle_*` method on `UserAgentWorker`, and keep spec-facing algorithms such as `create_agent`, `create_new_top_level_traversable`, `create_navigation_params_by_fetching`, and `finalize_cross_document_navigation` as named worker methods instead of free helper functions.

Inline single-use sidecar spawn setup at the `Command::new(...)` site and give first-party user-agent threads and sidecar processes stable names where they are created.

Route browser, embedder, automation, and webview requests through this crate so traversable ownership, navigation state, viewport updates, and rendering opportunities stay in one place.

When a cross-document navigation runs `beforeunload`, fan it out across the traversable's inclusive descendant tree instead of stopping at the active document, and queue a rendering opportunity after navigation commit so history traversal repaints without relying on incidental input.

Keep traversable target-name bookkeeping in this crate so iframe-host cleanup can stop the correct event loop without pushing that state into the embedder or content side.

When creating an iframe child traversable placeholder, emit the embedder registration signal for that traversable target name so the webview compositor can map child paints back into the parent iframe host instead of treating the child traversable as an unrelated root webview.

When retiring an iframe traversable, remove that traversable from shared event-loop ownership first and stop the event loop only when it no longer owns any traversables; stopping a shared parent/child event loop from within iframe-removal continuations can deadlock navigation teardown.

When removing a child traversable, delete that child browsing context from the browsing-context group without dropping the top-level browsing-context-group index for the parent traversable; that top-level mapping stays owned by the actual top-level browsing context.

Keep the spec-facing browser-global concepts in `UserAgentState` itself: the browsing-context-group set, the top-level traversable set, allocator state, and the pending navigation/fetch continuations. Treat helper hash maps in that file as model-local indices derived from those concepts rather than as replacements for them.

Keep pending navigation state as explicit spec-facing request, snapshot, history-entry, and history-handling records so finalization can follow HTML's `push` versus `replace` session-history steps without flattening those concepts into ad hoc fields.

Use distributed `RuntimeId` UUIDs for fetch controllers, document-fetch handlers, and timer keys so worker-owned components can allocate those ids locally without blocking on user-agent refill traffic. Keep `FrameId(u64)` as the compositor-facing iframe host identity, and reuse that frame id as the synthetic iframe traversable id when the user agent materializes `_iframe|...` helper traversables.

When `user_agent` code implements or continues a standards algorithm, follow the documentation conventions from `content/README.md`: anchor-only top doc-comments for the algorithm, verbatim `Step n:` comments for mapped steps, and separate `Notes:` comments for reduced-model or runtime-bridge explanation.

For spec-facing worker methods and continuations, give each helper its own anchor-only doc comment for the algorithm it continues, keep `Step n:` comments as verbatim standard prose, and move reduced-model or runtime-bridge explanation into separate `Notes:` comments.

Avoid introducing synchronous user-agent command bridges that block on content replies unless the corresponding standard algorithm has an explicit wait point; prefer queueing work and resuming through existing continuation events.

When content resolves the local `_self` / `_parent` / `_top` prefix of `the-rules-for-choosing-a-navigable`, keep the remaining user-agent work as an explicit continuation helper for shared target-name lookup and new-top-level creation, and reject unresolved synthetic iframe target names there instead of letting them fall through into top-level traversable creation.

For `create-a-new-child-navigable`, let content perform its local iframe/container steps first, continue the user-agent-owned stable child-navigable allocation asynchronously, and notify content with a continuation command instead of replying over a blocking request channel.

When response-driven navigation initializes a new document, keep same-site top-level navigations on the current browsing context and event loop, but explicitly run the browsing-context selection step for cross-site top-level responses instead of silently reusing the active top-level context.

Keep the initial top-level about:blank shell on the event loop that created it until the first navigation commits; the startup artifact should not force a new content process merely because the destination is cross-site.

Name user-agent command variants and worker methods after the standard algorithm or continuation they trigger (for example `Navigate`, `CompleteBeforeUnload`, and `FinalizeCrossDocumentNavigation`) instead of transport-oriented `Queue*` names, and keep the `navigate` entrypoint typed in terms of a navigable id that resolves to a traversable when the target is traversable-backed.

Model navigables and traversables as a single `Navigable` struct in `UserAgentState::navigables`. A traversable navigable is one where `event_loop_id` is `Some`; top-level traversables additionally have `parent_navigable_id: None`. Remove any parallel `TraversableSet` or `Traversable` storage; single-source updates eliminate the redundant dual-write pattern in every state setter.