# user_agent crate

The `user_agent` crate owns all browser-global coordination: navigables and traversables, navigation and session history, the agents and agent clusters of the HTML agent formalism, content-process lifecycle, and requests coming from the embedder and webview layers.

- `user_agent.rs` owns the top-level user-agent state and command loop.
- `agent.rs` defines the HTML agent records: `Agent` — the similar-origin window agent and dedicated worker agent kinds, recorded together in `UserAgentState::agents` and each keyed by the event loop it owns — plus `AgentCluster`/`AgentClusterKey`, the per-group agent cluster records.
- `event_loops.rs` defines the user-agent-side handles of the event loops the agents own — `WindowEventLoop` and `WorkerEventLoop` — and `spawn_window_event_loop`, which launches the content process that is a new window agent's cluster and bootstraps its window event loop inside it.
- `fetch.rs` provides `NetConnection` — owns the IPC connection to the net extension,
  tracks pending navigation fetches, and routes responses back to the user agent.
- `ui_event.rs` provides UI event serialization for routing across process boundaries.
- The UA and content processes send requests directly to the net and graphics extensions;
  there are no intermediary worker threads. Media playback commands go from content to
  the graphics extension; the media backend (AVFoundation/GStreamer) runs inside the
  graphics process, not in a separate process.
- Task queues and window timers belong to the content process's event loop
  (`content/src/html/event_loop.rs`), not to this crate.
- Key cross-worker ownership with UUID newtypes such as `EventLoopId`, `NavigableId`, and related ids from `ipc_messages`.
- Keep spec-facing algorithms and continuations as named worker methods on the owning type instead of as transport-oriented helper functions.
- Route browser, embedder, automation, and webview requests through this crate instead of through synchronous cross-thread bridges.

## IPC blocking and deadlock

Sending on an IPC channel can block when the system buffer is full, and the
user-agent thread sends content commands directly.  A content process blocked
sending an event to the user agent while the user agent blocks sending a command
to that same content process, with both channel buffers full, would deadlock the
two threads.

This is **not** a risk in practice: every content process re-routes its
incoming IPC through the ipc-channel router proxy (`ipc::crossbeam_proxy`) into
a crossbeam channel.  That proxy runs on a thread that is always ready to drain
content's command channel (the ipc-channel ROUTER thread), so a user-agent send
to content never blocks regardless of what the content process is doing.  The
content process can therefore block on its own crossbeam receive without
creating a feedback loop back to the user-agent thread.

## Embedder-served URL schemes

`EmbedderConfig::embedder_schemes` names the URL schemes the embedder serves
itself. A fetch for one of them is filtered out at the source that starts it —
here for a navigation, in the content process for a subresource — so it never
reaches the net process, whose job is networking and caching rather than
coordinating fetches. Filtering at the source is what keeps a navigation to
such a scheme from making a round trip through net to come straight back.

Both sources converge on `start_an_embedder_scheme_fetch`, which records a
`PendingEmbedderSchemeFetch` and calls `Embedder::embedder_scheme_fetch`. The
pending record names the recipient the way net's `ResponseRecipient` does:
`UserAgent` for a navigation fetch, resumed by fetch id, or `ContentProcess`
for a subresource, answered with `Command::CompleteDocumentFetch` on the event
loop that asked. The embedder answers whenever it has the response, from any
thread, through `WebviewProvider::complete_embedder_scheme_fetch` or
`fail_embedder_scheme_fetch`; a fetch that is never answered stays pending.

A content process names the navigable its document belongs to, the way
`NavigateRequest` names its source navigable, so the webview handed to the
embedder is that navigable's top-level traversable rather than a guess from
the event loop, which several traversables can share.

## Embedder user scripts

The scripts a document runs before it is populated travel with the command
that creates the document (`CreateEmptyDocument`, `CreateLoadedDocument`),
resolved by `user_scripts_for_navigable`. A traversable the embedder asked for
by name carries the scripts named in that request, recorded in
`UserAgentState::user_scripts` before the traversable is created so its first
document already runs them; one a script opened carries
`EmbedderConfig::default_user_scripts`. A child navigable drops the
main-frame-only scripts.

## Graphics process routing

The content processes send each traversable's `PaintFrame` directly to the
`formal-web-graphics` process, which composes the webview's scene (iframe
embed sites + video frames) and returns `GraphicsEvent::PixelFrameReady`.
The UA stores the accompanying `FrameHitInfo` in `UserAgentState::frame_hit_info`
(keyed by webview id) for UI event routing, and forwards the layers to the
host via `Embedder::new_web_content_layers`.

Gotcha: during a cross-origin navigation the traversable's event loop (and
content process) switches before the UA-side active document does — the
active document only changes at finalization, so in the migration window
`traversable_handles` and `active_documents_by_traversable` disagree.
Commands pairing those two maps (e.g. `UpdateTheRendering`) must verify the
active document is owned by the traversable's current event loop and skip
otherwise — a stale send fails in the new content process and, because no
paint frame is produced, leaves the render loop's pending flag stuck.
