# content crate

The content crate owns the content process: DOM and HTML algorithms, document
parsing and lifecycle, generic JavaScript engine integration via the
`js_engine` trait, Streams and Web IDL bridges, and the typed IPC boundary
back to the embedder and user agent.

## Design philosophy

Content code follows the same call chains the web standards define.  When a
spec algorithm calls Web IDL (e.g. type conversion, promise manipulation),
content code routes through `content/src/webidl/`.  When a spec algorithm
calls ECMA-262 directly (e.g. realm creation, script evaluation), content
code calls the `js_engine` trait directly.  The exception that routes
through `content/src/webidl/` anyway: HTML algorithms that read JS state in
place of a Web IDL step (e.g. the `self` getter's "relevant
realm.[[GlobalEnv]].[[GlobalThisValue]]" read, implemented in
`content/src/webidl/realm.rs`) — webidl hosts those direct-JS-call quirks
so the domain getter implements the spec steps and the bindings stay thin.
See `content/src/js/bindings/README.md`.  No Boa-specific APIs appear
above `js_engine/src/boa/`.  See `js_engine/README.md` for the full
design philosophy and `content/src/generic_js_test.rs` for validated
patterns.

## GcCell borrow discipline

Domain code must **never call an engine method (any `ec` operation) while a
`GcCell` borrow guard (`borrow` or `borrow_mut`) is live**.  An engine call
may allocate, and on the V8 backend an allocation can trigger a cppgc trace
that reads the cell while the borrow is live. A *mutable* borrow being traced
is an aliasing violation (undefined behavior); a *shared* borrow being traced
is legal aliasing, but the rule still forbids it so content code never has to
know which engine operations allocate (and a shared-borrow site can silently
become a mutable-borrow site later).

On the V8 backend the guard's lifetime is tied to the `ec` borrow
(`GcCell::borrow<'a>(&'a self, ec: &'a dyn ExecutionContext<…>)`), so the
compiler rejects an `ec` call while any guard is alive and the rule needs no
reviewer attention.  The Boa backend's guard holds no `ec` borrow (its cells
are `Gc<GcRefCell<T>>`), so there the rule is on the author.  The approved
patterns are:

- **Clone out, write back** — `let mut value = cell.get(ec);` … use the owned
  value (mutably, across `ec` calls) … `cell.set(value, ec);`.  `get` returns
  the value with its edges rooted, so the copy stays valid while engine code
  runs.
- **Scope the borrow** — hold the guard only for the section that touches the
  cell, and drop it before any `ec` call.  Typically that means cloning the
  needed fields into locals (`let listeners = cell.borrow(ec).clone();`) and
  running the engine calls on the locals.

Writing a rooted handle through a mutable guard is safe: dropping the guard
runs `Trace::store` over the cell's contents, converting those handles into
cppgc edges before the borrow ends.

Platform data reaches the engine through the same discipline:
`with_object_any`/`with_object_any_mut` return a guard that holds the
backend's borrow of the platform object (a `GcRef`/`GcRefMut` on Boa, a
reader count and writer flag on V8) for as long as the data is borrowed, and
it borrows both the object and the context.  Clone what is needed into an
owned value inside the closure/locals and write it back through
`with_object_any_mut` instead of holding a platform-data reference across an
engine call.

**Removing a value from a cell.**  A value that leaves a cell — a queued
request dequeued, a slot taken, a resolver pair consumed — must be removed
through `GcCell::get`/`take`/`take_all`/`pop_front`/`pop_back`/`remove`,
never by cloning the guard and then clearing the cell separately.  The
`get`/`take` family clones the value while the cell still traces it, converts
the clone's cppgc edges into `v8::Global` handles (`Trace::make_strong`), and
only then removes it, so the value stays valid while Rust holds it across
engine calls.  A `borrow_mut(ec).pop_front()` / `.take()` / `std::mem::take`
leaves the value as an untraced edge that the next collection can free.

**Cloning the cell itself.**  A cloned `GcCell` (for example the `state`
captured by a queued job closure) keeps the heap cell and its JS contents
alive for as long as the clone lives; writing the clone back into traced
storage converts it back to a traced edge (`Trace::store`).  Clone the cell
itself only when the clone must outlive every traced owner; to read a value
without extending the cell's lifetime, use `borrow`/`get` instead.

Platform-object plain fields follow the same rule: a field read out and then
cleared is cloned and rooted with `js_engine::gc::make_strong_traced` before
the slot is cleared, and a field mutated to a fresh JS value and written back
gets `js_engine::gc::store_traced` from `with_cloned_platform_mut` (see
`content/src/js/downcast.rs`).

To examine a subset of a cell's contents before an engine call, collect the
needed values under a shared borrow and root them with
`make_strong_traced`, instead of cloning the whole container with `get`
(`EventTarget::add_event_listener`/`remove_event_listener_entry` do this to
avoid rooting the entire listener list on every call).

Platform-object native data follows the same clone-out/write-back rule:
`with_object_any_mut` ties its `&mut dyn Any` to `&mut ec`, so an operation
that must call the engine while mutating a platform object clones it out,
runs the engine calls on the owned clone, then writes it back
(`content/src/js/downcast.rs::with_cloned_platform_mut` is the shared
helper).

**Known limits.**  The shared-borrow guard cannot enforce rooting:
`cell.borrow(ec).clone()` compiles and yields an unrooted copy, so the rules
above are the enforcement.  The executable case is
`generic_js_test::tests::edge_clone_taken_with_borrow_survives_collection_after_the_cell_is_cleared`;
the API fix (a non-cloneable guard or rooted clones) is described in
`js_engine/src/v8/README.md`, “An `Edge` clone that leaves traced storage is
not rooted”.

**Remaining GC sweep.**  `try_with_event_target_mut` and its named binding
test are fixed.  A per-platform-type sweep — create each platform object
through its production binding, mutate it once, and run a debug-build
collection — is still to be run to close any other `with_object_any_mut`
write-back site.

Do not write code that hands `ec` to a closure while a cell borrow is live.
The V8 backend enforces the rule as a backstop: `HeapCell::trace` aborts if
marking visits a mutably-borrowed cell (a Rust panic there would unwind
across the C++ marking visitor, so the failure is a hard abort with a log
line), and there is deliberately no `Trace` impl for bare
`std::cell::RefCell` — a `#[gc_struct]` field that needs interior mutability
must use `GcCell`, or be marked `#[ignore_trace]` when it holds no cppgc
edges.

## Task queue and commands

The HTML event loop's task queue lives in `content/src/html/event_loop.rs`
and runs on the content process main thread (`run_content_message_loop` in
`content/src/main.rs`); dedicated worker agents run their own worker event
loops on their own threads (see `content/src/html/workers/`).  Guidance for
adding work to the loop:

- Content-initiated work that the spec models as a task must be **queued as a
  `Task`**, never run by calling its handler directly: an inline call skips
  the bookkeeping the model requires (marking the task's document dirty, the
  microtask checkpoint after the task's steps).  When the step runs inside
  content and the spec says "queue an element task", content sends itself a
  `Command` (`RunImageElementTask`, `RunImageEnvironmentUpdate`) instead of
  adding a private queue; `handle_command` runs the task's steps and the
  checkpoint follows.
- An `ipc_messages::content::Command` is a message to this process.  Where a
  command only triggers a spec task (the target half of `postMessage`, a
  routed port message, `update the rendering`, beforeunload, WebDriver/CDP
  script evaluation and event dispatch), handling the command queues the
  matching `Task`, so it runs through the same processing-model steps as a
  task queued here.  A command that *is* the queued task (the image element
  tasks) runs its steps directly; the cluster-level commands (viewport,
  document lifecycle, navigation fetch completion, shutdown) run directly as
  well.  A `Command::PortTask` arrives here only for ports of the window
  event loop; ports owned by a dedicated worker agent's event loop are
  delivered over that agent's own user-agent command channel (see
  `dedicated_worker_agent.rs`).
- **Processing-model state is not a task.**  Work a spec step does not queue
  (e.g. `ImageRegistry::pending_presentation`, which waits because a DOM hook
  runs while blitz holds the document mutably borrowed) is state applied by
  the content process, and is named as state, never as a task.
- **The command channel is only read while the task queue is empty.**  A
  command's steps run as soon as it is read, so reading one with tasks
  already queued runs those steps ahead of them.  The rendering pipeline
  depends on this: a cross-origin child navigable's `update the rendering`
  task must run before the top-level traversable's frame composes (their
  frames compose together), and letting a command overtake it makes the
  `RenderingOpportunity` TLA+ trace validation fail intermittently
  (`verification/verify-specs.sh`, roughly one run in five).  The pipeline
  itself is documented in `user_agent/README.md` ("Rendering pipeline").

## Fetch

`content/src/fetch.rs` holds the event-loop side of the Fetch spec, including
`request_header_list`, the conversion from a blitz `Request` to the header
list that travels on the wire.

`fetch` is the minimal start of the Fetch algorithm: it registers the response
continuation, implements the `data:` branch of scheme fetch, and routes the
other schemes to the embedder or net. The per-step notes in `fetch`,
`main_fetch`, and `scheme_fetch` mark what is still missing (request client
population, default headers, preloading, CORS, redirects, timing, and response
handover).

Subresource fetches leave this process by one of two routes, chosen by the
URL's scheme. A scheme named in `ContentBootstrap::embedder_schemes` is one
the embedder serves itself: the fetch goes to the user agent as
`Event::EmbedderSchemeFetchRequested`, naming the navigable that asked, and
comes back as `Command::CompleteDocumentFetch` like any other. Every other
scheme goes straight to the net process. Both routes register the same
pending handler first, so the response side does not know which was taken.
Filtering here rather than in net is what keeps net a place where networking
happens rather than a fetch coordinator; see `user_agent/README.md`.

Every subresource the document needs is reported through the
`blitz_dom::ResourceHooks` impl of `DocumentProviders` in
`content/src/html/document_providers.rs` and the DOM tree steps in
`content/src/html/dom_trees.rs`: stylesheets,
`@import` rules, `@font-face` fonts, and CSS background/mask images.  Each
request takes the scheme routing above and comes back as
`Command::CompleteDocumentFetch`; content then installs the result into the
document with `BaseDocument::handle_message` (a `Resource`, or
`complete_stylesheet_import` for `@import`).  `data:` URLs are resolved in
content and delivered through the same command.  No image is decoded here: the
graphics process is the only image decoder (`AGENTS.md`, "Process
Boundaries"), and the layout-time image loader's requests are answered as
failed so blitz paints nothing for them (see below).

The `<img>` element follows its own processing model in
`content/src/html/images_processing_model.rs`; keep its fetch on that path (see
`content/src/html/README.md`, "Images").  The completed image is registered
with the graphics process once per `ImageIdentifier`; `DestroyDocument`
releases the document's registered images with
`GraphicsCommand::UnregisterImages`.  Iframe documents run the same
processing model as a top-level document: every iframe is a child frame, and
its `<img>` layers draw inside that frame (see `content/src/html/README.md`,
"Iframes").

## Clipboard

The content process never accesses the system clipboard: a paste is answered
from text the embedder prefetched onto the same UI event, and a copy is
forwarded to the embedder. Do not add a platform clipboard dependency back to
this crate.

## Render invalidation

`ContentDocument`'s `PaintDirty` carries three answers, and
`update_the_rendering` treats them differently:

- **Paint** — content changed something the resolve pass cannot report: a
  stylesheet or font loaded, an offscreen canvas or video embed site appeared,
  the viewport changed, or blitz made a change that is only visible in paint
  (text selection, IME). `ShellProvider::request_redraw` and
  `GlobalScope::mark_document_dirty` set it, as does every DOM mutation: the
  document tree hooks (attribute changes other than an `<img>`'s image
  attributes, insertions, removals) and the text-node data setter
  (`Node::run_set_text_content`). It repaints whatever the resolve pass says.
- **Restyle** — blitz reported that a computed style may have changed:
  `ShellProvider::request_restyle` (a `:hover` state change, from
  `set_hover_to`/`clear_hover`). The document is resolved and repainted **only
  if `BaseDocument::resolve` reports that the pass produced restyle damage**.
- **Composition** — the document's embed sites changed while its own recorded
  scene did not: an `<img>` layer switched to a newly decoded image
  (`apply_pending_image_presentation`). The frame is re-sent with a freshly
  built composition and `scene_unchanged` set, so graphics recomputes every
  embed site's placement from it and re-presents only the layers that changed
  (`graphics/README.md`, "Surface delivery invariants").

Nothing marks a document merely because script ran. Event listeners, timers and
message handlers reach the document only through blitz's mutator (whose hooks
mark it, see above) or through content's own mutation sites, so a handler that
changes nothing the document draws — the demo page's "Swap sources", which only
sets an `<img>`'s `src` — re-composes without re-recording the page's scene. A
new mutation path must mark the document itself.

An `<img>`'s image attributes (`src`, `srcset`, `sizes`, `crossorigin`,
`referrerpolicy`) are the exception among attribute changes: the hook records the
node instead of marking a repaint, and the content process drops the restyle
damage blitz inserted for it once the operation has run
(`ContentProcess::clear_embed_only_attribute_damage`). The image processing model
then decides at presentation time: if the element already had an embed site and
its computed `width` and `height` are both specified, the new intrinsic size
cannot move anything the document draws, so only the damage that one call added
is dropped and the document is re-composed; otherwise the document repaints
(the image became available, so the alt text it painted goes away, or its box
moved).

An animating document also resolves every cycle: blitz reports CSS animation
damage only when the resolve pass runs, and it does not request a redraw per
animation tick. Painting still depends on the pass reporting a change.

A playing video is not an animating document: its frames are decoded in the
graphics process, which composes the video's layer without a content render
cycle (`graphics/README.md`, "Video frames drive their own composition"). A page
whose only moving content is a video therefore runs no render cycle at all
between input events, so scrolling during playback competes with nothing.

Media pipelines are owned by the document whose media elements created them:
`destroy_document` sends `MediaDestroy` for every pipeline recorded on the
document, so navigating away stops the outgoing page's video instead of leaving
its frames to compose against the incoming document.

Animation frame callbacks are not a repaint reason by themselves. A callback
that mutates the document marks it as any other mutation does, and the change is
painted in the same frame; a callback that changes only an `<img>` source marks
nothing, and the cycle re-composes. A document whose callbacks re-registered
themselves (a script-driven animation loop) reports `animating` on its frame,
which keeps the UA queueing rendering opportunities — that is what drives the
demo page's "Animate sources".

## Known issues

- **A physical click repaints the whole document.** blitz focuses the clicked
  element on pointer-down (`set_focus_to`) and requests a repaint for it, so
  pressing a button re-records the page's scene. Separately, a document that
  contains a `<button>` makes `BaseDocument::resolve` report restyle damage on
  every pass, with no click and nothing focused: a page holding a `div` and a
  `button` reports a change on two consecutive resolves in the same cycle,
  while the same page without the button reports none on the second, so the
  damage is inserted during the pass itself. The source of that damage was not
  investigated. Content runs no resolve pass for a listener run, so both show
  up only where blitz itself asks for a repaint or a restyle.

- **A hover-driven repaint still happens where it should not.** A resolved
  document that reports no restyle damage keeps its layer clean, but pages still
  repaint on pointer moves, including cases where no `:hover` rule seems to
  apply. blitz's own
  `blitz-html/tests/resolve_change.rs` shows the damage signal distinguishes a
  matching `:hover` rule from none, so the remaining cause is either UA
  stylesheet `:hover` rules matching, or stylo reporting damage for restyled
  nodes whose computed values are equal — that was not investigated. End-to-end
  checks over CDP could not settle it either: `Input.dispatchMouseEvent` hover
  delivery in formal-web's CDP server is unreliable.

- **Document lifecycle commands run outside the task queue.**
  `CreateEmptyDocument`, `CreateLoadedDocument`, `CompleteDocumentFetch`,
  `FailDocumentFetch` and `DestroyDocument` parse documents and run script
  (inline scripts, unload handlers) directly in `handle_command`, while the
  spec runs those steps in tasks queued on the networking and
  navigation-and-traversal task sources.  Running a script does end with a
  microtask checkpoint (`EnvironmentSettingsObject::evaluate_script`), so
  promise reactions are not deferred; what is missing is the task boundary
  itself, and with it the task-level checkpoint and the document bookkeeping
  `run_task` performs.

- **Clippy warning backlog.** The content crate has a backlog of pre-existing
  clippy warnings (e.g. "useless conversion to the same type: V8Object").

- **A dynamically inserted `<img>` ignores a fixed CSS `height`.** An `<img>`
  created with `createElement` and appended with `appendChild` lays out at its
  intrinsic aspect ratio; the stylesheet's `height` and an inline `height` on
  the element are both ignored, so changing the source reflows the document.
  Building the same figure through `innerHTML` applies the stylesheet, and the
  image keeps its fixed-height box. Images present in the initial markup are
  unaffected.

## Layout

- `content/src/main.rs` and the root modules resume embedder-driven HTML algorithms and content IPC entry points.
- `content/src/dom` holds native DOM [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) and DOM Standard algorithm implementations.
- `content/src/ui_events` holds the UI Events Standard types (UIEvent, MouseEvent) and their constructors.  Note: `MouseEvent` and `MouseEventInit` are defined by the Pointer Events spec (`https://w3c.github.io/pointerevents/`), not UI Events; only `UIEvent` and its members live in `https://w3c.github.io/uievents/`.
- `content/src/html` holds parser, document lifecycle, navigation helpers, and HTML global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object).
- `content/src/js` holds the content crate's JS integration layer: type aliases pointing to the concrete `js_engine` backend, generic platform-object resolution and downcast helpers, and JavaScript dispatch glue. The `js_engine` trait itself lives in the top-level `js_engine/` crate (see its `README.md`).
- `content/src/webidl` holds shared Web IDL callback and promise algorithms (implements Web IDL §3 JavaScript binding).
- `content/src/streams` holds native Streams [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) and Streams Standard algorithms.
- `content/src/infra` holds shared Infra Standard helpers.

## Three-layer architecture

Every Web-exposed feature follows a three-layer split (domain → Web IDL infra →
JS bindings glue).  See `content/src/js/bindings/README.md` for the definitive
description with examples and common mistakes.

## Spec Documentation

The rules for spec-annotated code — anchor-only doc comments, verbatim
`// Step N:` step comments, `// Note:` for discrepancies only — live in
`AGENTS.md` ("Algorithm Implementation"); the definitive reference with the
complete Common Mistakes table is `content/src/js/bindings/README.md`.
