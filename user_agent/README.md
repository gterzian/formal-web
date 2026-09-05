# user_agent crate

The `user_agent` crate owns all browser-global coordination: navigables and traversables, navigation and session history, the agents and agent clusters of the HTML agent formalism, content-process lifecycle, and requests coming from the embedder and webview layers.

- `user_agent.rs` owns the top-level user-agent state and command loop.  A single thread selects over the user-agent command channel, the net channel, the graphics channel, and every content process's event channel.  Each content process is one agent cluster: its main thread runs the similar-origin window agent's window event loop, its dedicated worker agents run their own worker event loops on nested threads, and every agent of the cluster reports over the same event channel.
- `agent.rs` defines the agent records.  `Agent` is the union of the two agent kinds recorded in `UserAgentState::agents` — `SimilarOriginWindowAgent` (the window agent of one agent cluster, owning its window event loop and the traversables whose documents run on it) and `DedicatedWorkerAgent` (the record of one worker agent) — registered side by side, each keyed by the event loop it owns: its window event loop for a window agent, its own worker event loop for a worker agent, the id by which content addresses the agent.  Worker agents are flat, not nested under the window agent of their cluster, since a worker agent is created and destroyed while its host window agent lives on; a worker record carries its own worker event loop, the worker id it is reported and closed under, and the signifier of its hosting window agent (so the record is dropped when the content process exits).  `AgentCluster`/`AgentClusterKey` are the per-group agent cluster records of the "agent cluster formalism" section; the UA-side records of the event loops the agents own live in `event_loops.rs`.
- `event_loops.rs` defines the user-agent-side handles of the two event-loop kinds.  `WindowEventLoop` is the window event loop of a similar-origin window agent — the record of the window agent's content process (its agent cluster), whose main thread doubles as the cluster's command endpoint and as the window event loop: the command channel carries both cluster-level commands (bootstrap, viewport, document lifecycle, shutdown) and the window event loop's queued tasks, the event receiver carries the events of every agent of the cluster (worker agents report over it too), and the record holds the child process plus the script/click waiters keyed by request id.  `WorkerEventLoop` is the event loop of a dedicated worker agent: its own event loop id and its own user-agent command channel, so commands addressed to a worker event loop — the port tasks of `Command::PortTask` — go straight to the worker, with no main-thread forwarding.  `spawn_window_event_loop` launches the content process that is the new agent's cluster and bootstraps the agent's window event loop inside it; it is called from `obtain_similar_origin_window_agent` (`UserAgentWorker`, the `#obtain-similar-origin-window-agent` algorithm), which creates the cluster and the window agent when the browsing context group's agent cluster map has no cluster for the origin's site yet, and otherwise returns the window agent of the group's existing cluster.
- `fetch.rs` provides `NetConnection` — owns the IPC connection to the net extension,
  tracks pending navigation fetches, and routes responses back to the user agent.
- `ui_event.rs` provides UI event serialization for routing across process boundaries.
- The UA and content processes send requests directly to the net, graphics, and media extensions;
  there are no intermediary worker threads.
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

## Graphics process routing

The user agent starts the `formal-web-graphics` process alongside net and media on startup.
Paint frames from content processes are forwarded to the graphics process via
`GraphicsCommand::PaintFrame`. The graphics process composes scenes (iframe embed
sites + video frames) and sends the final composed scene back via
`GraphicsEvent::ComposedSceneReady`. The UA stores the accompanying
`FrameHitInfo` for hit-testing and forwards the scene to the embedder host
via `Embedder::new_web_content_scene`.

Hit-testing info (`FrameHitInfo`) from each composed scene is stored in
`UserAgentState::frame_hit_info`, keyed by webview id. This data enables
UI event routing without the embedder needing access to the compositor tree.

During a cross-origin navigation the traversable's event loop (and content
process) switches before the UA-side active document does: the active
document only changes at finalization, so in the migration window
`traversable_handles` and `active_documents_by_traversable` disagree.
Commands pairing those two maps (e.g. `UpdateTheRendering`) must verify the
active document is owned by the traversable's current event loop and skip
otherwise — a stale send fails in the new content process and, because no
paint frame is produced, leaves the render loop's pending flag stuck.
