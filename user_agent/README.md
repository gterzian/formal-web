# user_agent crate

The `user_agent` crate owns all browser-global coordination: navigables and traversables, navigation and session history, event loops, timers, content-process lifecycle, and requests coming from the embedder and webview layers.

- `user_agent.rs` owns the top-level user-agent state and command loop (uses `select!` to also process net, graphics, and media responses directly).
- `event_loop.rs` owns content event loops and manages the content process.
- `timer.rs` owns the timer worker.
- `fetch.rs` provides `NetConnection` — owns the IPC connection to the net extension,
  tracks pending navigation fetches, and routes responses back to the user agent.
- `ui_event.rs` provides UI event serialization for routing across process boundaries.
- The UA and content processes send requests directly to the net, graphics, and media extensions;
  there are no intermediary fetch or media worker threads.
- Key cross-worker ownership with UUID newtypes such as `EventLoopId`, `NavigableId`, and related ids from `ipc_messages`.
- Keep spec-facing algorithms and continuations as named worker methods on the owning type instead of as transport-oriented helper functions.
- Route browser, embedder, automation, and webview requests through this crate instead of through synchronous cross-thread bridges.

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
