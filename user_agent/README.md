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

## Rendering pipeline

The rendering pipeline is demand-driven: content is rendered only when the
embedder can present a frame and the navigable's UI state has changed since
the last render. The user agent owns that gate; content owns the document and
the painted scene; graphics owns composition and pixels; the embedder owns
presentation. The gate implements the HTML event loop's
<https://html.spec.whatwg.org/multipage/webappapis.html#rendering-opportunity>
and the Unnecessary rendering step of
<https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>.

### The two inputs

A **rendering opportunity** exists for a top-level traversable when the
embedder can present a frame. The embedder calls the public
`WebviewProvider::frame_needed` at each paint; the UA's
`note_rendering_opportunity` records it in `rendering_opportunities`. The
embedder is the only source of rendering opportunities: the UA cannot create
one itself.

A **UI state update** says the navigable's visible state may have changed:
input dispatch, script evaluation, navigation finalization, viewport changes,
automation, worker `requestAnimationFrame`, or a content- or graphics-reported
change. `note_ui_state_update` records it in `queued_ui_state_updates`, with
the epoch time that becomes the event loop's `frameTimestamp` (the HTML "last
render opportunity time"). Notes are batched: multiple notes before the next
render collapse into one, and the latest timestamp wins.

`update the rendering` is queued only when both inputs exist. A UI state
update without a rendering opportunity is batched and the UA calls
`Embedder::request_next_frame`; a rendering opportunity with no queued UI state
update does not render, and the opportunity remains recorded for the next UI
state update. That is the Unnecessary rendering step in code form.

### Queueing update the rendering

`queue_update_the_rendering` starts one cycle per navigable:

- a per-navigable guard (`pending_update_the_rendering`) prevents a second
  cycle while the first is in flight;
- the navigable's queued UI state update is drained and its timestamp is used
  as `frameTimestamp`;
- the top-level traversable's rendering opportunity is consumed;
- the active document must be owned by the navigable's current event loop (a
  cross-origin navigation moves the traversable to a new content process
  before the UA-side active document changes; a stale send would leave the
  pending flag stuck);
- pending worker animation-frame requests are dispatched;
- for a top-level navigable, graphics is sent `RenderStarted` with the cycle
  deadline;
- `ContentCommand::UpdateTheRendering` is sent to the content event loop;
- for a child navigable, the top-level traversable's update is queued too,
  because graphics composes only when the top-level frame arrives.

Content turns the command into a task so it runs in event-loop order (see
`content/README.md`); the rendering task then enters `update_the_rendering`.

### Content's part

`update_the_rendering` runs the animation-frame callbacks and applies the
Unnecessary rendering test: a clean document reuses its last scene and
composition instead of re-running blitz, while a dirty or animating document
(or one with no cached scene) resolves style/layout and paints. Either way
content sends a `PaintFrame` directly to the graphics process; that is how
graphics learns the current top-level or child frame and its `animating` flag.
Content marks a document dirty when a script or event may have mutated it, and
sends `ContentEvent::NoteUiStateUpdate` when a change happens outside a
UA-driven command.

### Graphics' part

The graphics process stores child frames in the parent's compositor and
composes only when a top-level frame arrives, waiting for the child and video
frames the top-level frame references. It returns child viewport data in
`PixelFrameReady`; the UA publishes that data to the child. A dirty child frame
or canvas is reported with `GraphicsEvent::CompositionChanged`, which the UA
turns into a UI state update for the parent traversable.

`RenderStarted` arms a per-webview composition deadline (16 ms). If the content
event loop is blocked and the top-level `PaintFrame` does not arrive, graphics
composes the last committed root with any dirty out-of-band layer (a worker
`OffscreenCanvas` or a video frame) so that content keeps animating. A normal
top-level frame clears the deadline. `PixelFrameReady` carries the composed
layers, hit-test info, child viewport data, and the animating frame ids.

### Completing the cycle

On `PixelFrameReady` the UA:

- clears the in-flight state for the top-level and composed child frames;
- forwards the layers to the embedder (`new_web_content_layers`), which
  presents them;
- records `FrameHitInfo` and the child-frame mapping for input routing;
- publishes child viewports back to content (cached, so a viewport update does
  not create a render cascade);
- if the composed scene is animating, queues UI state updates for the animating
  frames so the next rendering opportunity starts the next frame;
- if the embedder already has an outstanding rendering opportunity (a paint
  arrived while the render was in flight), queues the next update;
- otherwise, if a static UI state update was stranded during the in-flight
  render, calls `request_next_frame` so it is not deferred to the next
  unrelated input event.

The result is a closed loop for animated content: `PixelFrameReady(animating)`
queues the next UI state update, the embedder keeps supplying rendering
opportunities while the tab animates, and each opportunity starts the next
`update the rendering`, content paints, graphics composes, and the cycle
repeats. A static page leaves the loop: no animating flag, no self-sustaining
frame requests, and only an actual UI state update or an embedder event starts
the next cycle.

### Cross-origin navigation and rendering

During a cross-origin navigation the traversable's event loop (and content
process) switches before the UA-side active document does — the active document
only changes at finalization, so in the migration window `traversable_handles`
and `active_documents_by_traversable` disagree. Commands pairing those two maps
(e.g. `UpdateTheRendering`) must verify the active document is owned by the
traversable's current event loop and skip otherwise — a stale send fails in the
new content process and, because no paint frame is produced, leaves the render
loop's pending flag stuck.

### Model

The cross-process cycle is modeled by
`verification/tla_specs/RenderingOpportunity.tla`. It tracks
`rendering_opportunity`, `ui_state_update_count`, `pending`,
`rendering_updated`, `composed`, and `animating` per frame; its invariants are
`pending >= rendering_updated >= composed` and `pending <= BufferCount` (two
buffers: one displayed, one being rendered). See `verification/README.md` for
the trace-event mapping.
