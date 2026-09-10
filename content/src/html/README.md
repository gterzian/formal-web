# content/src/html

`content/src/html` owns HTML parser integration, document lifecycle work, navigation helpers, and HTML global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) such as `Window` and `GlobalScope`.

- Keep DOM-tree entry points under `content/src/html/html_dom_tree.rs`, and route per-element hooks from there into element modules.
- Keep iframe bindings and iframe processing algorithms together in `content/src/html/html_iframe_element.rs` as free functions over content-process state (`ContentProcess`).
  `contentDocument` is unimplemented: the getter returns null even for a
  same-origin child navigable whose document lives in this content process
  (`contentWindow` resolves and the iframe `load` event does fire).
- Keep helper names aligned with the corresponding HTML algorithm anchors, and prefer explicit error returns or `debug_assert!` plus safe early returns over sentinel ids.
- Trigger parser-discovered iframe work from document-load parsing completion.
- Use the `web_standards` extension (`spec_lookup`) with `https://html.spec.whatwg.org/` to read the HTML spec.

## Structured clone (`structured_data/`)

The safe-passing algorithms live in `structured_data/`, split between the
generic algorithms and the per-platform-object parts, with the serialization
pitfalls to avoid; see its `README.md`.

## Algorithm split: content process vs user agent

Many HTML algorithms (navigation, `window.open`, iframe creation) span the
content process (which runs JS and owns DOM state) and the user agent
(which owns the navigable tree, browsing contexts, and event-loop
dispatch).  The side that hits its limit sends an IPC message and the
other side continues; the IPC ordering guarantee (per content process,
messages arrive in order) makes this safe.  Which steps of a given
algorithm run on which side is carried by the per-step comments and notes
of its two halves — do not summarize the split in the README.

The concrete split for the algorithms this crate owns: the content side of
navigation, `window.open`, and document creation lives in `content/src/html.rs`
(`navigate`, `the_rules_for_choosing_a_navigable`, `ChosenNavigable`),
`window.rs` (`Window::open`, `window_open_steps`), and
`global_scope.rs` (`create_auxiliary_context_document`,
`set_navigable_hierarchy`); the user-agent side lives in
`user_agent/src/user_agent.rs` (see "Related documentation" below).

### Opener tracking for auxiliary browsing contexts

The content process does not track opener relationships — those are purely
UA-side state (`BrowsingContext.opener_browsing_context`, set by
`setup_opener_for_window_open`). The opener is only used for:
- Navigation policy (e.g., `target=_blank` with `rel=opener`)
- `window.opener` JS property (not yet implemented)
- Popup blocking

## Window IDL members (`window.rs`)

Every Window interface member is implemented as a `Window` method in
`window.rs` following the spec's getter/method steps with verbatim
`// Step N:` comments.  The getters that read realm state
(`window`/`frames`/`self` — "return this's relevant
realm.[[GlobalEnv]].[[GlobalThisValue]]") route through
`content/src/webidl/realm.rs::relevant_realm_global_this_value`, which owns
the JS-side read (see `content/src/js/bindings/README.md`, "When the spec
calls into JS directly").  Members whose state is user-agent-only (navigable
target name, opener, closed, document-tree child navigable count) return
placeholder values from the domain methods with a `// Note:`.

The bindings (`content/src/js/bindings/html/window.rs` for the Window
interface, `bindings/html/windowproxy.rs` for the WindowProxy platform
object) are thin glue: they downcast, resolve the local Window
(`local_window_domain` / `window_domain_from`), and call the domain method.
The WindowProxy member set is only reached for cross-content-process windows
(no local Window); the same member names on the Window interface shadow them
for same-content-process windows.

### `window.open` cannot reach a cross-origin destination

The navigate algorithm's "allowed by sandboxing to navigate" check is
approximated in `window_open_steps` by comparing the destination's origin
with the source document's, throwing a "SecurityError" DOMException when they
differ, so a cross-origin popup (`window.open("https://other.example/")`)
throws instead of opening.  Lifting this needs the sandboxing flag set and
the target snapshot params the check is defined over.

## WindowProxy (`windowproxy.rs`)

<https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

The file implements the WindowProxy exotic object as an ECMAScript Proxy
whose target is a [`WindowProxy`](windowproxy.rs) platform object tied to
the navigable (one per (realm, navigable), cached on the realm's
GlobalScope).  Each trap function carries its spec anchor and step
comments, and the `backing` cell's variants document the same-process vs
cross-content-process behavior — read those before extending the proxy.
Two constraints are not visible from the traps alone:

- The `SameContentProcess` backing keeps the Window's JS object handle
  **rooted** (not a cppgc-traced edge): the backing must stay usable across
  the navigation-commit garbage collection, and a traced edge read back
  from the cell after that collection is not reliably usable on the V8
  backend.  Do not convert it to a traced edge; see the field note in
  `WindowProxyBacking`.
- The `backing` cell is shared by every clone of the `WindowProxy` (the
  realm's cached copy and the platform object), so navigation commit
  re-points the cell in place — `destroy_document` in the content process
  re-points every cached WindowProxy of the navigable at the new Window, or
  switches the backing to `CrossContentProcess` when the new document was
  created in another content process.  The proxy identity never changes.
  Where the destroy lands is UA-side: the user agent routes `DestroyDocument`
  to the event loop that owns the document, not the traversable's current
  event loop (after a cross-process navigation those differ).

The mapping of agent clusters/agents to processes and realms (one content
process per similar-origin window agent, cross-origin windows always
cross-process, a realm per Window) is what decides which of the two
backings applies; the agent records and their spec anchors live in
`user_agent/src/user_agent.rs` and `agent.rs`.

### Remaining gaps

**1. Cross-cluster selective access is not wired.**  When the target
navigable lives in another agent cluster (another content process), the
WindowProxy must give selective access to the remote window: `postMessage`
already routes through the user agent, and the remaining members
(`document`, `location`, `name`, …) must be forwarded
to the target process the same way.  The domain `WindowProxy` is what makes
this possible — the proxy is a navigable id plus a backing and a forwarding
policy, so it can hand any member off to the user agent.

**2. The cross-content WindowProxy exposes a fixed member set.**  The
cross-content WindowProxy exposes the Window members the current features
need rather than delegating every property access; members not in its set
(e.g. `setTimeout`, `onmessage`, or script-defined globals on the target
window) are absent until the selective-access forwarding is wired.

**3. Child navigable properties (array-index and named).**  The spec requires
WindowProxy to expose child browsing contexts by numeric index (`window[0]`,
`window[1]`) and by name.  This requires tracking the document-tree child
navigables on the Document, which is not yet implemented.

**4. WindowProxy identity is per realm.**  `create_window_proxy` resolves
the current realm's own navigable to the realm's global object (the spec's
[[GlobalThisValue]] of a Window realm is that navigable's WindowProxy), so
`window.top === window`, `window.parent === window`, and
`window.open("", "_self") === window` hold.  Every other navigable gets a
proxy cached per (realm, navigable), which means the same navigable seen
from two realms is two objects: `iframe.contentWindow.top === window` does
not hold like in browsers.  `Window::top_value`/`parent_value` consult the
navigable hierarchy (`top_level_traversable_id`/`parent_traversable_id`) and
create the resolved navigable's WindowProxy with no local window, so the
proxy's backing is `CrossContentProcess` even when that navigable's window
lives in this process.

**5. `name`, `opener`, `closed` are stubs.**  The navigable target name is
tracked by the user agent (`traversable_target_names` in
`user_agent/src/user_agent.rs`), the opener relationship by
`BrowsingContext.opener_browsing_context`, and the is-closing flag by no
process yet; the domain methods (`Window::name_value`, `opener_value`,
`closed_value`, `close`) return placeholder values with `// Note:`
annotations until that state is sent to the content process or forwarded.

## Workers (`workers/`)

Dedicated-worker code lives under `workers/`, split across modules by the
spec section that specifies each interface or algorithm (workers.html
§10.2.1): the `Worker` platform object and its constructor steps are in
`workers/worker.rs`; the `WorkerGlobalScope` common interface — its event
target, global scope, name, url, type and closing flag, plus the
WindowOrWorkerGlobalScope mixin members — in `workers/worker_global_scope.rs`;
the `DedicatedWorkerGlobalScope` interface, which embeds that base (the
repo's subclass pattern) and carries the dedicated members (name,
postMessage, close, the inside port and its inbound queue), in
`workers/dedicated_worker_global_scope.rs`; and run-a-worker with the
agent's event loop in `workers/dedicated_worker_agent.rs`.
`WorkerLocation` and `WorkerNavigator` are in
`workers/worker_location.rs` and `workers/worker_navigator.rs`.  Add new
worker algorithms to the module that owns their spec section, quoting each
spec step verbatim in `// Step N:` comments and annotating deviations
inline at the step they diverge from (see `AGENTS.md`, "Algorithm
Implementation").

Design decisions that change how the spec's worker channel and lifecycle
are realized are annotated where they diverge — per-field on the channel
types and platform objects, per-step in the run-a-worker and
terminate-a-worker bodies — not summarized here.  Read those notes before
extending either mechanism.  The one cross-cutting deviation: the worker's
implicit outside/inside MessagePort pair is not created.  Owner and worker
communicate over two direct crossbeam channel ends, with the port role
each end replaces mapped on `Worker::outside_port`, the `WorkerBootstrap`
fields and `DedicatedWorkerAgentState`.

Known gaps:

- **importScripts only supports data: URLs.**  Fetches of other URL schemes
  are asynchronous in this architecture (through the net process), and
  importScripts is synchronous; non-data URLs throw a NotSupportedError.
- **Module workers are evaluated as classic scripts.**  "Fetch a module worker
  script graph" (run-a-worker step 12's module branch) is not implemented;
  `type: "module"` workers run their source as a classic script.
- **`error` events on the Worker object are fired directly, not as queued
  global tasks**, matching the document lifecycle commands' existing
  deviation (see `content/README.md`).
- **Worker runtime errors are conflated with fetch/parse failures, and
  ErrorEvent is missing.**  An evaluation error in the worker script (parse
  failure or a top-level exception at run-a-worker step 12.13) fires a plain
  `error` event at the Worker object; per report an exception (runtime script
  errors) it should first fire an error event at the worker global scope
  (self.onerror) and only reach the Worker object when unhandled, and the
  event must be an ErrorEvent with message/error/filename/lineno/colno.
  Runtime errors thrown inside worker *tasks* (e.g. an onmessage handler
  throwing) are not reported at all: no error event fires at the worker
  global scope and nothing reaches the Worker object — the engine swallows
  the exception.  A script parse failure should abort the worker at step
  12.4, but the engine's evaluate combines parse and run, so the two are not
  distinguished.  (`dedicated-worker-parse-error-failure.html` passes: it
  asserts the error handler receives a single plain-`Event` argument, which
  the direct event does satisfy.)
- **The default WPT run covers document-side dedicated-worker tests.**
  `tests/wpt/include.ini` selects the dedicated-worker tests under `workers/`
  that run their subtests in the document (Worker construction and
  messaging, WorkerLocation/WorkerNavigator, timers and close(), data: URL
  and nested workers, importScripts of data: URLs); tests that fail are
  selected there and disabled in `tests/wpt/meta` with the specific gap.
  Still excluded: worker-global tests (`.worker.` files and
  `fetch_tests_from_worker` need worker-side testharness, which imports
  testharness.js over http — see the importScripts gap above;
  `webmessaging/without-ports/025.html` is disabled for that reason), and
  shared-worker, module-worker, and worker tests that additionally need an
  unrelated unexposed feature (Blob, URL, fetch/XHR, canvas, SharedArrayBuffer,
  iframe/session-history navigation, redirects).
- **The worker realm does not expose `performance`.**  The
  WindowOrWorkerGlobalScope `performance` member is missing on worker
  globals, so `self.performance.now` is undefined (`WorkerPerformanceNow.html`
  is disabled for it).
- **`terminate()` does not promptly stop delivery of already-queued
  messages.**  Messages queued on the worker's outside port are still
  dispatched after `terminate()` returns, so the timing-sensitive
  `Worker_terminate_event_queue.htm` and `constructors/Worker/terminate.html`
  fail (the infinite-runner and evaluation-time terminate tests pass).
- **Worker script URLs are resolved against the document URL, not the
  document base URL**, so a worker created under a `<base href>` fails to
  load (`constructors/Worker/use-base-url.html` is disabled for it).
- **The Worker constructor and `postMessage` do not enforce their required
  arguments**: `new Worker()` and `worker.postMessage()` with no arguments
  do not throw a TypeError (`constructors/Worker/Worker-constructor.html`
  and `Worker-multi-port.html` are disabled for it).
- **Platform objects report `[object Object]` from `Object.prototype.toString`**
  (no `@@toStringTag` is exposed; affects every platform object — Worker,
  MessageEvent, Event, DOMException — on the V8 backend), so tests that
  assert class strings (`Worker_basic.htm`'s constructor subtest,
  `message-event.html`, `AbstractWorker.onerror.html`) are disabled.
- **Owner-set lifetime management is minimal.**  Terminate-on-owner-document-
destroy is wired (`destroy_document` terminates its workers, as does a
closing owner worker for its nested workers); the spec's
protected/permissible/suspendable monitoring is not.
- **Shared workers are not implemented.**  They need new-agent-cluster
  allocation (which must happen in the user agent) and the UA-side instance
  lookup by (origin, name); the dedicated-only implementation folds
  `WorkerGlobalScopeKind` into the dedicated global scope.

## Canvas (offscreen rendering)

The `canvas` element is supported only through `OffscreenCanvas`
(`content/src/html/canvas/`): it has no `getContext` member, and
`transferControlToOffscreen()` allocates a `CanvasId` and hands it to a new
`OffscreenCanvas`.  The placeholder element is composited as its own texture
layer like a cross-origin iframe.  Wiring a canvas that draws on a worker:

- The `CanvasId` is registered with the graphics process
  (`GraphicsCommand::RegisterCanvas`, sent by the owner content process when
  `transferControlToOffscreen` runs) so the worker's `CanvasPaint` — which
  carries only the id — can be routed to the owning webview.
- The worker realm gets the content process's `graphics_sender` through
  `WorkerRealmWiring`, so its `OffscreenCanvasRenderingContext2D` commits can
  send scenes directly to graphics.
- A worker `requestAnimationFrame` sends
  `ContentEvent::WorkerAnimationFrameRequested`; the user agent records the
  request against the worker's owner navigable. When that navigable's
  update the rendering is queued — i.e. when the embedder needs a frame —
  the user agent sends `Command::RunAnimationFrameCallbacks` to the
  worker's own event loop (the worker's update-the-rendering counterpart).
  The dispatch must be gated on that frame cadence, never performed when
  the request arrives: a worker `requestAnimationFrame` loop re-registers
  on every run, so dispatching immediately spins at the worker event
  loop's speed rather than the display refresh rate. Because the dispatch
  rides the owner navigable's cycle, the user agent also arms the graphics
  process's `RenderStarted` deadline, so a canvas commit keeps compositing
  against the last committed root when the window's top-level frame is
  late (see `graphics/README.md`).
- The canvas registry (`GlobalScope::canvas_registry`, shared with
  `ContentProcess`) is what lets the owner document's render path discover a
  newly transferred canvas: `transferControlToOffscreen` inserts into it and
  marks the document dirty, so the next update-the-rendering rebuilds the
  frame composition instead of reusing the cached one.

Remaining gaps:

- Only the `"2d"` context exists, with `fillStyle`, `fillRect`, and
  `clearRect` (a minimal anyrender mapping).  No paths, transforms, images,
  gradients, or text.
- `OffscreenCanvas.width`/`height` are read-only (no resize).
- Canvas embed sites surface only for a top-level document; a canvas inside a
  same-origin iframe document would not get its own layer (same-origin
  iframes are baked into their parent's scene).
- Canvas frames are never unregistered when their document is destroyed (the
  id is a UUID, so a stale frame is inert, but it leaks until the webview
  goes away).
- Worker animation is display-paced on the winit embedder only. The winit
  windowed app routes `request_redraw` through the OS, so the UA's
  frame-needed gate lands at display cadence, but the AppKit app services
  `request_redraw` by calling `frame_needed` immediately and its
  `CVDisplayLink` only runs while the surface's `animating` flag is set —
  and a worker-only animation leaves that flag false. A worker
  `requestAnimationFrame` loop therefore still runs at IPC round-trip speed
  on the AppKit browser. Closing the gap needs the worker's pending
  animation frames to mark the document animating and the UA to stop
  requesting its own redraw while the traversable is animating.

## Related documentation

- `content/src/webidl/README.md` — Web IDL bindings infrastructure, platform object pattern
- `content/src/js/README.md` — JS integration layer conventions (bindings, exotic objects)
- `content/README.md` — Content-crate overview
- `user_agent/src/user_agent.rs` — `create_new_top_level_traversable_from_content`, `create_new_top_level_traversable`, `the_rules_for_choosing_a_navigable` (UA side), `setup_opener_for_window_open`, and the agent model (`AgentCluster`, `AgentClusterKey`, `similar_origin_window_agent`)
- `ipc_messages/src/content.rs` — `NewTraversableInfo`, `CreateEmptyDocument`, `NavigateRequest`
- `content/src/html.rs` — `the_rules_for_choosing_a_navigable` (content side), `navigate`, `ChosenNavigable`
- `content/src/html/window.rs` — `Window::open`, `window_open_steps`
- `content/src/html/global_scope.rs` — `create_auxiliary_context_document`, `set_navigable_hierarchy`
