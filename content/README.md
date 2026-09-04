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
`GcCell` borrow guard (`borrow`/`borrow_mut`) is live** — shared or mutable.
The rule is engine-independent: an engine call may allocate, and on the V8
backend an allocation can trigger a cppgc trace that reads the cell while the
borrow is live. A *mutable* borrow being traced is an aliasing violation
(undefined behavior); a *shared* borrow being traced is legal aliasing, but
the rule still forbids it so content code never has to know which engine
operations allocate (and a shared-borrow site can silently become a
mutable-borrow site later). The approved patterns are:

- **Clone out, write back** — `let mut value = cell.borrow(ec).clone();` …
  use the owned value (mutably, across `ec` calls) …
  `cell.set(value, ec);`.
- **Scope the borrow** — hold the guard only for the section that touches
the cell, and drop it before any `ec` call (an explicit `drop(guard)` where
the control flow is not obvious).

Do not write code that hands `ec` to a closure while a cell borrow is live
(e.g. `with_..._mut(|data, ec| ...)` patterns). The V8 backend enforces the
rule as a backstop: `HeapCell::trace` aborts if marking visits a
mutably-borrowed cell (a Rust panic there would unwind across the C++
marking visitor, so the failure is a hard abort with a log line), and there
is deliberately no `Trace` impl for bare `std::cell::RefCell` — a
`#[gc_struct]` field that needs interior mutability must use `GcCell`, or be
marked `#[ignore_trace]` when it holds no cppgc edges. The one remaining
exception class is the `with_object_any_mut_with` platform-object closure
pattern (it hands `&mut dyn Any` and `ec` to the operation; see
`js_engine/src/v8/README.md`, "Remaining work").

## GcCell interior mutability refactor candidates

Types that currently use `&mut self` for mutation but could use `GcCell` + `&self`:

- **Node** (`content/src/dom`) — `child_node_ids` is read-only; other mutating methods could use GcCell.
- **Window** (`content/src/html`) — `setTimeout` callbacks stored behind GcCell.
- **Streams** (`content/src/streams`) — writable/readable stream state machines use `Cell<bool>`/`RefCell`; some could use GcCell for GC-traced callback fields.
- **AbortSignal** (`content/src/dom`) — already uses `GcCell<AbortSignalState>` at the top level; internal fields like `onabort` could move to GcCell if needed.

## The event loop's task queue

The window agent's HTML event loop runs on the content process main thread
(`run_content_message_loop` in `content/src/main.rs`).  Every task source feeds
one queue of `Task` (`content/src/html/event_loop.rs`), and
`ContentProcess::run_task` is the only place that runs one: it takes the oldest
task, performs its steps, and performs the microtask checkpoint that ends them.

Each dedicated worker agent runs its own event loop on its own native thread
nested to the content process (see `content/src/html/workers/dedicated_worker_agent.rs`): its
own task queue, timer map, and realm, driven from the main thread over a
crossbeam command channel and from the net process over its own IPC channel,
both joined into one select like the window loop's.
Content-initiated work (window timer expiry, port message tasks) must therefore
be **queued as a `Task`**, never run by calling its handler directly: an inline
call skips the bookkeeping the model requires (marking the task's document
dirty, the microtask checkpoint after the task's steps).

An `ipc_messages::content::Command` is an ad hoc message the user agent (or the
net process) sends this process — not a task.  Where the spec step behind a
command says to queue a task on this event loop (the target half of
`postMessage`, a routed port message, `update the rendering`, beforeunload,
WebDriver/CDP script evaluation and event dispatch), handling the command
queues the matching `Task`, so it runs through the same processing-model steps
as a task queued here.  The rest (viewport, document lifecycle, navigation
fetch completion, shutdown) `ContentProcess::handle_command` runs directly.

The loop waits for the oldest task on the queue, a command, a WebAssembly
result, or the earliest expiry time in the map of active timers
(`MapOfActiveTimers` in `content/src/html/timers.rs`, reached through
`EventLoopTaskSources`).  It runs one task per iteration, so a task that queues
another task — a message event handler posting a message, a timer callback
scheduling a timer — does not starve the other inputs.  The timer channel is
only a wake-up: expiry queues the expired timers' tasks, it does not run them.

**The command channel is only read while the task queue is empty.**  A
command's steps run as soon as it is read, so reading one with tasks already
queued runs those steps ahead of them.  The rendering pipeline depends on this:
a child navigable's `update the rendering` task must run before the top-level
traversable's frame composes (their frames compose together), and letting a
command overtake it makes the `RenderingOpportunity` TLA+ trace validation fail
intermittently (`verification/verify-specs.sh`, roughly one run in five).

## Known issues

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