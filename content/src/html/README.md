# content/src/html

`content/src/html` owns HTML parser integration, document lifecycle work, navigation helpers, and HTML global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) such as `Window` and `GlobalScope`.

- `dom_trees.rs` implements the HTML Standard's DOM tree steps from [`#dom-trees`](https://html.spec.whatwg.org/#dom-trees) as `blitz_dom::MutationHooks` methods.  Each general step dispatches to the HTML element steps the element's local name defines; those are free functions in the element's module (the `<img>` ones are in `images_processing_model.rs`).  Add an element by extending the dispatch in the general step and writing its functions beside the element.
- `document_providers.rs` is blitz plumbing, not a spec algorithm: it holds `DocumentProviders`, its `ResourceHooks` impl (which reports blitz's subresource requests), the `AssetRequest` those fetches carry, and `install_resource`, which installs a fetched response into the document.  A function carrying a spec anchor belongs in the module that owns that spec section, not here.
- `html_dom_tree.rs` holds the document-level iframe post-connection and removing entry points run at document load completion and document destruction; the iframe's HTML element steps do not run from the `MutationHooks` hooks.
- Keep iframe bindings and iframe processing algorithms together in `content/src/html/html_iframe_element.rs` as free functions over content-process state (`ContentProcess`).
- Keep helper names aligned with the corresponding HTML algorithm anchors, and prefer explicit error returns or `debug_assert!` plus safe early returns over sentinel ids.
- Iframe work is driven by the DOM mutation hooks in `dom_trees.rs`: a
  connected, changed or removed iframe records its HTML element steps, and the
  content process runs them at the end of the current operation.  The
  document-load scan remains the synchronous path for parser insertions.
- Use the `web-standards` skill with `https://html.spec.whatwg.org/` to read the HTML spec.

## Common microsyntaxes (`common_microsyntaxes/`)

The micro-parsers for the data types HTML content attributes accept (the
[common microsyntaxes](https://html.spec.whatwg.org/#common-microsyntaxes))
live in `common_microsyntaxes/`, one module per spec subsection
(`signed_integers.rs`, `non_negative_integers.rs`, …).  Each parser is a free
function named for its spec algorithm (`rules_for_parsing_non_negative_integers`)
carrying that algorithm's anchor and verbatim step comments.  Never inline an
attribute's parsing rules into the element module that needs them, and never
fold a sub-algorithm the spec calls into (`rules for parsing integers`) into
its caller.

## Structured clone (`structured_data/`)

The safe-passing algorithms live in `structured_data/`, split between the
generic algorithms and the per-platform-object parts, with the serialization
pitfalls to avoid; see its `README.md`.

## Iframes

Every iframe's content is a real document in `ContentProcess::documents`,
created by the ordinary navigation path (`create_loaded_document`), and every
iframe composes as an `EmbedSite::Frame` (`build_frame_composition_metadata`
walks every `NavigableContainerState`). A child document is never painted into
the parent's blitz scene: each iframe is its own document-table entry and its
own frame.

- `about:blank` and `srcdoc` navigate like any other URL. The content process
  carries the `srcdoc` body on `NavigateRequest::srcdoc_html`; the user agent
  answers the `about:srcdoc` navigation with it in place of a fetch. Both URLs
  inherit the parent document's origin and base URL, so their document lands in
  the parent's content process.
- The user agent decides the process from the destination origin
  (`user_agent/README.md`, "Content-process placement for navigations"), so a
  same-origin URL child also reuses the parent's event loop. Process placement
  is the only difference between a same-origin and a cross-origin iframe.
- Post-connection, `src`/`srcdoc` attribute-change and removing steps are
  driven by the DOM mutation hooks in `dom_trees.rs`.  Those hooks run while
  blitz holds the document mutably borrowed and cannot reach the navigable
  state, so they record the step and
  `ContentProcess::drain_pending_iframe_steps` runs it at the end of the current
  operation.  The `<img>` steps in the same hooks do not need this: they only
  touch the document's providers and run synchronously.

Remaining gaps:

- The iframe DOM mutation steps run when the content process has the document
  and realm back, at the end of the current operation, not synchronously inside
  the DOM mutation the way the algorithms specify.  A child realm cannot be
  created while the running script holds the realm's engine, so script that
  reads `iframe.contentWindow` immediately after inserting the element sees
  null.  Making the steps strictly synchronous needs a realm-creation handle
  that does not borrow the parent realm's engine, and the iframe-to-navigable
  mapping moved out of the realm's `GcCell` so a hook can reach it.
- `contentDocument` is unimplemented: the getter returns null even for a
  same-origin child navigable whose document lives in this content process
  (`contentWindow` resolves and the iframe `load` event does fire).  Named
  property access to child navigables on the parent Window (`window.myframe`)
  is not implemented either.
- The sandboxing directive is parsed but not stored on the element, and the
  iframe load in-progress/mute flags are not modeled (an initial `about:blank`
  replaced by a `src` navigation can fire an extra `load`).
- Iframe lazy loading (`loading=lazy`) is not implemented: `process the iframe
  attributes` runs its "navigate" steps immediately.

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

- The `SameContentProcess` backing holds the Window's JS object as an
  ordinary traced `JsObject` field; `WindowProxy::backing` reads it through
  `GcCell::get`, which roots the cloned backing's edges before returning it.
  That matters because the returned clone must stay usable after navigation
  commit re-points or replaces the shared cell: an unrooted edge read back
  from the cell would lose its referent to the navigation-commit garbage
  collection on the V8 backend.  Do not replace the `get` with a
  `borrow().clone()`.
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
Implementation").  The WindowOrWorkerGlobalScope mixin members follow the
general rule in `content/src/js/bindings/README.md`, "Web IDL mixins".

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

## Images

`images_processing_model.rs` implements <https://html.spec.whatwg.org/#images-processing-model>.
The `<img>` HTML element steps are free functions there; `dom_trees.rs`
dispatches to them.  Do not run the algorithm from the IDL setters or from the
render path.

Availability lives in `ImageRegistry`, not in blitz's
`SpecialElementData::Image`: content installs a zero-pixel `RasterImageData`
only to give layout the intrinsic size, while the graphics process decodes and
owns the pixels.  `naturalWidth`, `naturalHeight`, `complete`, `currentSrc`,
and `decode` read the registry through the realm's `GlobalScope`; reading
`raster_image_data` is not a substitute.

`decode()` promises live on the realm's `GlobalScope` (a traced `GcCell`, so
the resolving functions survive to settlement; the image registry cannot hold
them) and are settled by the step that moves the element's request to a new
state.  Never settle a promise while holding a `GcCell` borrow.

A "queue an element task" step is a command the content process sends to
itself, so it runs later on the same task queue as every other task; the
model's state changes that do not queue a task are applied where the algorithm
runs.  A DOM hook cannot write blitz state (blitz holds the document mutably
borrowed), so "prepare an image for presentation" waits in
`ImageRegistry::pending_presentation` until the content process has the
document again.

Known gaps in the processing model:

- The net process delivers the whole response body in one
  `CompleteDocumentFetch` message, so one networking task runs step 27's "last
  task" and the per-chunk presentation updates of the earlier networking tasks
  are not modeled.  Implementing streaming fetch requires the response handler
  to become incremental.

- `update-src-complete.html` is disabled.  It asserts `img.complete` is false
  synchronously after setting `src`, but steps 10-18 (including installing
  the new request as the pending request) run in the step 8 microtask, so
  `complete` (true while the current request is completely available and the
  pending request is null) stays true until the microtask runs.  The
  synchronous false result is the on-demand behavior in "when to obtain
  images" (the current request's state returns to unavailable), not the
  immediate-obtaining behavior implemented here.

- `srcset`/`sizes` selection picks the first `srcset` candidate and ignores
  the descriptors and `sizes`, so the current pixel density is always 1.0 and
  the natural dimensions are the decoded pixel dimensions; a parent
  `<picture>` element and its `<source>` siblings are not tracked, so
  `picture`/`source` mutations never re-run the algorithm.  "Reacting to
  environment changes" runs on every viewport change but uses the same
  selection, so a `sizes`-driven choice never changes the image.  The
  insertion hook receives no parent, so the `img` insertion steps cannot
  check for a `<picture>` parent and update the image data on every
  insertion; the HTML element moving steps are not observed because blitz
  exposes no moving hook.
- Decoded images are retained for the webview lifetime until their document is
  destroyed; there is no per-image eviction for images still referenced by a
  live document.
- Lazy loading (`loading=lazy`), animation restart, EXIF density
  correction, CORS enforcement, and progressive/multipart image decoding
  are not modeled.
- "Fetching the image must delay the load event" is not modeled: the document
  load event ignores `<img>` fetches.  Implementing it as written deadlocks
  on any page whose script clears the image's `src` to release the load
  event: the engine runs parser scripts at load completion (see
  `continue_document_load`), so the clearing script would itself wait for the
  load event, and the fetch it is meant to abort never finishes.  The spec's
  ordering needs parser scripts to run during parsing first.
- The `img` adopting steps are not observed, so adopting an element into
  another document does not re-run the algorithm.
- Subresource URLs resolve against the document's URL only: a `<base href>`
  change is not reflected in the base URL captured by `DocumentProviders`.
- The `Image(width, height)` legacy factory function is not implemented, so
  `new Image()` throws; `available-images-onload.html`,
  `Image-constructor.html` and WPT's `the-img-element/decode/` tests (which
  construct their elements with it) are blocked on it, which leaves `decode()`
  covered only indirectly by the enabled `the-img-element` tests.
- The graphics process is the only image decoder, so the content process never
  reads image bytes itself: it ships each response body to graphics and runs
  step 27 of <https://html.spec.whatwg.org/#update-the-image-data> when
  `GraphicsEvent::ImageDecoded` reports the decoded dimensions and format
  (`complete_image_decode`).  The accepted set is PNG,
  JPEG, GIF and WebP — formats graphics reports as `ImageFormat::Other` take
  step 27's "not in a supported file format" branch, and an SVG `src` is
  reported as an error. Which formats actually decode is the running OS's
  Image I/O support on macOS (see `graphics/README.md`).
- Nothing decodes for blitz: the document's layout-time image loader is
  answered with a failed load, so CSS `background-image`/`mask-image` render
  nothing.  Those `AssetRequest::DecodedImage` requests are still fetched
  before the load is failed (skipping the fetch is remaining work).  They stay
  unsupported until graphics grows an image layer kind positioned by the
  document's paint order instead of an element box.  `<img>` elements are not
  affected: every document, including one in a same-origin iframe, runs the
  processing model and draws its images in its own frame.

## Canvas

Both 2D rendering context interfaces live in `content/src/html/canvas/`:
`CanvasRenderingContext2D` (the element's `getContext("2d")`) and
`OffscreenCanvasRenderingContext2D` (the transferred `OffscreenCanvas`).
Each interface struct owns one `RenderingContext2D` (the output bitmap and
drawing state), which is where the member algorithms of the mixins both
interfaces include live.  The binding layer defines each mixin's members
once in `bindings/html/canvas_context_2d_mixins.rs` for both interfaces:
each member resolves the receiver to its `RenderingContext2D` and calls the
method there.  To add a mixin: put its algorithms on `RenderingContext2D`,
add its members to a `define_canvas_*_members` function, and call that from
both interfaces' bindings (only `CanvasRenderingContext2D` for
`CanvasUserInterface`).  The rule that governs this — and every other Web IDL
mixin — is in `content/src/js/bindings/README.md`, "Web IDL mixins".

Both `getContext("2d")` and `transferControlToOffscreen()` register the
canvas — a `CanvasId` in `GlobalScope::canvas_registry` and
`GraphicsCommand::RegisterCanvas` to graphics — and mark the document dirty,
so the element's layer composites the committed scene (a placeholder layer
for `transferControlToOffscreen()`).  A drawing member serializes the
accumulated anyrender scene as `GraphicsCommand::CanvasPaint` through the
current realm's graphics sender, which is why the same commit path serves a
window canvas and a worker `OffscreenCanvas`.  Registration must happen
before the next update-the-rendering can include the embed site.  Both
`getContext` methods record the context mode (a shared cell) and cache the
context object, so a second call returns the same object; the mode also
makes `transferControlToOffscreen()` after a 2D context throw, and makes a
transferred `OffscreenCanvas` throw from `getContext`.

Wiring a canvas that draws on a worker:

- The `CanvasId` is registered with the graphics process by the owner content
  process when the canvas is bound, so the worker's `CanvasPaint` — which
  carries only the id — routes to the owning webview.
- The worker realm gets the content process's `graphics_sender` through
  `WorkerRealmWiring`, so its `OffscreenCanvasRenderingContext2D` commits can
  send scenes directly to graphics.
- A worker `requestAnimationFrame` sends
  `ContentEvent::WorkerAnimationFrameRequested`; the user agent records the
  request against the worker's owner navigable. When that navigable's
  update the rendering is queued, the user agent sends
  `Command::RunAnimationFrameCallbacks` to the worker's own event loop (the
  worker's update-the-rendering counterpart).
  The dispatch must be gated on that frame cadence, never performed when
  the request arrives: a worker `requestAnimationFrame` loop re-registers
  on every run, so dispatching immediately spins at the worker event
  loop's speed rather than the display refresh rate. Because the dispatch
  rides the owner navigable's cycle, the user agent also arms the graphics
  process's `RenderStarted` deadline, so a canvas commit keeps compositing
  against the last committed root when the window's top-level frame is
  late (see `graphics/README.md`). The render cycle itself is documented in
  `user_agent/README.md` ("Rendering pipeline").

Remaining gaps:

- Only the `CanvasState`, `CanvasFillStrokeStyles` (`fillStyle`) and
  `CanvasRect` (`fillRect`, `clearRect`) mixins are implemented.
  `CanvasSettings`, `CanvasTransform`, `CanvasCompositing`,
  `CanvasImageSmoothing`, `CanvasShadowStyles`, `CanvasFilters`,
  `CanvasDrawPath`, `CanvasUserInterface`, `CanvasText`, `CanvasDrawImage`,
  `CanvasImageData`, `CanvasPathDrawingStyles`, `CanvasTextDrawingStyles`
  and `CanvasPath` are not, so there are no paths, transforms, images,
  gradients, or text.
- The `CanvasRenderingContext2D.canvas` attribute works; the
  `OffscreenCanvasRenderingContext2D.canvas` attribute is not exposed.
- The canvas element's `width`/`height` IDL attributes reflect the content
  attributes (the content-attribute parsing rules and the `placeholder`
  `InvalidStateError` are implemented), but changing them does not run "set
  bitmap dimensions", so an existing 2D context is neither reset nor resized.
- `reset()` clears the accumulated scene and resets the tracked drawing
  state, but the default path is not tracked and context loss is never
  signaled.
- `OffscreenCanvas.width`/`height` are read-only (no resize), and the
  `OffscreenCanvas` constructor does not model the inherited
  language/direction.
- The `OffscreenCanvas` transfer steps carry the canvas id and bitmap
  dimensions but not its inherited language/direction.
- Canvas frames are never unregistered when their document is destroyed (the
  id is a UUID, so a stale frame is inert, but it leaks until the webview
  goes away).
- Worker animation is display-paced on the winit embedder only. The winit
  windowed app routes `request_next_frame` through the OS, so the UA's
  rendering-opportunity gate lands at display cadence, but the AppKit app
  services `request_next_frame` by calling `frame_needed` immediately and its
  `CVDisplayLink` only runs while the surface's `animating` flag is set —
  and a worker-only animation leaves that flag false. A worker
  `requestAnimationFrame` loop therefore still runs at IPC round-trip speed
  on the AppKit browser. Closing the gap needs the worker's pending
  animation frames to mark the document animating and the UA to stop
  requesting its own frame while the traversable is animating.

## Related documentation

- `content/src/webidl/README.md` — Web IDL bindings infrastructure, platform object pattern
- `content/src/js/README.md` — JS integration layer conventions (bindings, exotic objects)
- `content/README.md` — Content-crate overview
- `user_agent/src/user_agent.rs` — `create_new_top_level_traversable_from_content`, `create_new_top_level_traversable`, `the_rules_for_choosing_a_navigable` (UA side), `setup_opener_for_window_open`, and the agent model (`AgentCluster`, `AgentClusterKey`, `similar_origin_window_agent`)
- `ipc_messages/src/content.rs` — `NewTraversableInfo`, `CreateEmptyDocument`, `NavigateRequest`
- `content/src/html.rs` — `the_rules_for_choosing_a_navigable` (content side), `navigate`, `ChosenNavigable`
- `content/src/html/window.rs` — `Window::open`, `window_open_steps`
- `content/src/html/global_scope.rs` — `create_auxiliary_context_document`, `set_navigable_hierarchy`
