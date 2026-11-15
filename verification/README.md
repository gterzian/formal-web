# verification crate

The verification crate owns trace recording, TLA+ validation, and the shutdown workflow that ties them together.

- The main process starts the trace monitor, shares senders with local workers, and sends the same sender to the content and net processes after IPC bootstrap.
- Trace specs live under `verification/tla_specs/`, and recorded NDJSON logs plus TLC working files live in temporary directories that are removed after validation.
- `VerificationRun::finish()` requires all top-level `TraceSender` clones to be dropped so shutdown can observe channel closure and complete.
- Verification uses the local TLA+ Toolbox jar at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar` by default.
- `./verification/verify-specs.sh` is the canonical end-to-end verification: it starts the embedder headless with TLA+ tracing, runs a WebDriver session through the trace pages under `verification/` (a page with a nested iframe, the port workflow page, a page whose canvas commits outrun a blocked content event loop, a page playing a video, a page with a pending animation frame callback, and a page whose worker canvas outruns a blocked document), collects trace events, and validates all specs (Navigation, RenderingOpportunity, …) against their models. Each page is settled before the next navigation, since a page reached by the next navigation stops recording its step.
- Both verification scripts rebuild every helper they spawn (embedder, content, net, graphics); keep that list complete, since a stale helper can leave the embedder blocked in shutdown (see `AGENTS.md`, "IPC wire format consistency").
- `webdriver-screenshot-check` decodes WebDriver screenshots with the `image`
  crate, which no other target in this crate uses. It is therefore behind the
  `screenshot-check` feature (`cargo run -p verification --features
  screenshot-check --bin webdriver-screenshot-check`), so the graphics process
  — which reaches this crate through `ipc_messages` — does not compile an image
  decoder on its macOS fast path.
- `./verification/verify-navigation.sh` is the hyperlink-navigation flow: it drives one link click (`StartupExample.html` → `navigated.html`) and treats successful `Navigation` TLA+ validation as the acceptance criterion. On failure, the human-readable report includes the failing NDJSON entry, preceding context entries, and the full TLC counterexample trace.

## Specs

| Spec | Events traced (producer) | Validates |
|---|---|---|
| `Navigation` | UA `CreateNavigable`/`CreateChildNavigable`/`RemoveNavigable`/`CreateNavigation`/`StartNavigating`/`ContinueNavigation`, content `RunBeforeUnload` | navigable/navigation state machine |
| `RenderingOpportunity` | UA `CreateFrame` (the navigable-creation part of the navigation path, traced from `CreateNavigable`/`CreateChildNavigable`) /`NoteUiStateUpdate` (including the next cycle's notes an animating composition makes)/`RenderingOpportunity`, content `UpdateTheRendering`, graphics `FrameStored` (the top-level frame was stored, which is what satisfies the render cycle's deadline) / `GraphicsComputed`/`Deadline`/`OutOfBandComputed` (the event name is the composition kind, traced immediately before the frame is sent) and `FrameDropped` (a frame from a superseded cycle), UA `MissedFrame` | the render cycle documented in `user_agent/README.md` ("Rendering pipeline"): the rendering-opportunity gate, update the rendering, the three composition kinds, and the depth of the updates the user agent holds |
| `MessagePort` | content `NewChannel`/`PostMessage`/`ReceiveMessage`/`Transfer`/`TransferReceive`, UA `RouteMessage`, content `RunTask` (the last two are the implementation-level routing/task events, skipped by the coarse trace consumer) | the cross-process port workflow mapped to the HTML spec's channel messaging steps: channel creation, the FIFO port message queue ordering (each `ReceiveMessage` pop is checked against the abstract queue head), and the transfer/transfer-receive transitions |

### MessagePort scope

The trace consumer replays the recorded events against `MessagePort.tla`,
which maps one-to-one to the HTML spec's channel messaging steps
(`NewMessageChannel` = entangle, `PostMessage` = step 7 of the message port
post message steps, `ReceiveMessage` = the message task firing,
`Transfer`/`TransferReceive` = the transfer steps).  The fine-grained routing
queue and per-event-loop task queues of `MessagePortExtraFG.tla` (the
refinement model; `MessagePortFG.tla` is the finite-state variant) are an
implementation detail: the abstract `MessagePort` model's per-port queue
preserves the FIFO ordering the message tasks must respect, and the
`ReceiveMessage` trace carries the popped message id so the consumer checks
the pop against the queue head.

`verification/messageport-trace.html` drives the actions in the
verification session: it creates channels, delivers messages directly, runs
the multi-transfer ordering flow of
`Channel_postMessage_with_transfer_incoming_messages.any.js` (messages
posted at each transfer step are received in post order), and delivers
through a transferred port.  The content processes trace
`NewChannel`/`PostMessage`/`ReceiveMessage`/`Transfer`/`TransferReceive`,
the user agent traces `RouteMessage`, and `RunTask` (the physical task
handling) is traced by the content processes; the coarse consumer skips the
latter two event kinds.

### RenderingOpportunity scope

The model tracks the frame-production cycle per **top-level traversable** and
its **child frames** (iframes). `RenderingOpportunity` and the composition
actions (`GraphicsComputed`, `Deadline`, `OutOfBandComputed`) only ever fire for
top-levels (the embedder sends `frame_needed` for webviews; the graphics process
traces the composition action its kind names before it sends the frame), while
`NoteUiStateUpdate`/`UpdateTheRendering` are traced for every frame. The
pipeline itself is documented in `user_agent/README.md` ("Rendering pipeline");
this section covers what the trace model records.

Frames are created dynamically by the `CreateFrame` action: the initial
top-level traversable (parent `NoParent`) and iframes (parent is an existing frame)
can be born at any time. The navigation path's graphics-relevant part —
`CreateNavigable`/`CreateChildNavigable` — is therefore traced for this spec
too as `CreateFrame`, so the model's frame set grows from the trace alone.
The verification session (`verify-specs.sh`) navigates through
`verification/iframe-trace-page.html`, which embeds a cross-site `file://`
iframe containing a leaf iframe, so the child rendering path and the removal of
a subtree are actually traced and validated; its `deadline-trace-page.html` and
`outofband-trace-page.html` record the deadline and out-of-band composition
kinds, and `animating-trace-page.html` keeps an animation frame callback
pending so every composed frame notes the next cycle (the only way animated
content reaches the model).

A render cycle is numbered: the user agent stamps the number on the updates it
queues, content carries it on the frames it paints, and graphics drops a frame
whose cycle a newer one superseded (`FrameDropped`). Only a composition that
presents the top-level content frame advances the number. A cycle whose update
stays outstanding past the user agent's timeout is a missed frame: the user
agent stops waiting for it, drops the cycle and starts the next one
(`MissedFrame`), so the abandoned cycle's late frames are dropped instead of
composing. Frames are accounted for as presented (`GraphicsComputed`) or
dropped (`FrameDropped`), which is what keeps the pipeline bounded.

The model keeps its state space finite with a cap (`CommandDepth`) on the
updates content was sent and has not run, and a `Done` action that stutters
once every cap is hit: the real pipeline has no such cap, so a trace that
overflows it fails validation. The base-model configuration caps the frame
universe at one top-level and one child (`RenderingOpportunity.cfg`) to keep
the invariant and liveness check fast; the trace validation uses every frame
id the trace references and a larger `TraceCommandDepth`, because a document
that blocks its event loop while out-of-band layers keep arming cycles builds
a deeper backlog than the base model's cap.

### RenderingOpportunity remaining work

The model's counters are relative to the last paint: a paint rebases them by
the compositions it consumed (the embedder presents what was composed), which
is what keeps them within `CommandDepth` across a long trace.

What graphics composes out of band depends on environment state the model does
not carry: `Deadline` needs a changed out-of-band layer (a video frame or a
worker's canvas commit) and `OutOfBandComputed` needs one to present, so
`OutOfBandComputed` is gated on the model's own condition (no content cycle in
flight) and `Deadline` carries no guard of its own: graphics decides it when a
deadline expires and delivers it once the frames it waits on are stored, which
can be after the update it was decided for was released, and the trace carries
only the delivery.

The recorded session's pages do not miss a frame, so `MissedFrame` is only
checked by the base model. A miss needs a cycle's wait to stay unserved for the
user agent's timeout with a paint after it, and the session's pages either
finish in time or have a dirty out-of-band layer: the deadline composition ends
the wait and starts the next one, so an out-of-band-animated page keeps the
pipeline alive while its content event loop is blocked and never misses.

`FrameDropped` has no session coverage yet. A page whose worker-driven
offscreen canvas keeps arming cycles while the document blocks its event loop
records drops, but its trace does not replay: content falls far enough behind
that the model's counter abstraction stops matching. The counters are relative
to the last paint (`composed` is rebased at each one) while content's backlog is
a task queue that a paint does not shrink, so a deep backlog makes the model
run out of `queued` commands that content still owes (`UpdateTheRendering` with
nothing left to render). Closing that needs the model to carry the backlog
without the paint rebase, or a content-side bound on queued commands.

The base model passes its invariants (`TypeOK`, `QueuedLeadsRendering`,
`RenderingLeadsComposed`, `DoubleBufferBound`, `PendingUpdateLeadsComposition`)
and the `UiStateUpdatesServiced` liveness property.

The model abstracts all UI state update notes into `NoteUiStateUpdate` and
relies on `RenderingOpportunity` fairness to eventually service a batched
update, so it does not distinguish a cross-origin content change arriving out
of band (the graphics-side `CompositionChanged` that notes a non-top-level
frame) from any other note. The model batches the whole hierarchy, while the UA
drains each navigable independently (see `user_agent/README.md`, "Rendering
pipeline"). Expressing that recovery in the model would need a distinct
`CompositionChanged`-style note event and a `request_next_frame` recovery
action, and is deferred.

### Adding a spec

1. Write `{Name}.tla` (the model) and `{Name}Trace.tla` (the trace consumer) plus `{Name}.cfg` / `{Name}Trace.cfg` in `verification/tla_specs/`. Discovery is automatic from the flat `.tla` files.
2. Emit events from code with `verification::tla_log!(tracer, -> "{Name}", "Event", args...)`. Producers holding a `TraceSender`/`TLATracer` are the UA, content processes, the graphics process, and the embedder apps. A producer that owns the state a composition decides traces it before sending the message that reports it: a stamp taken after the send can land behind the receiver's own events on the shared clock.
3. For specs whose event args need TLA+ numeric types (e.g. generation counters), extend `collect_trace_ids_for_spec` / `render_trace_data_module_for_spec` in `verification/src/validate.rs` with a custom event renderer (see `render_gpu_rendering_trace_event`, which renders generation/region args as integer literals).
4. `./verification/verify-specs.sh` picks the new spec up automatically; a `CHECK {Name} ... OK` line means the recorded trace is consistent with the model.

### Spec file style

- The `.tla` model files carry **no prose**: at most a one-line comment stating the
  reason the spec exists. The model's definitions, action names, and invariants
  are the documentation — explanatory paragraphs about the modeled system live in
  this README and the owning feature's README, not in the spec.

### TLA+/TLC gotchas (all confirmed with the bundled tla2tools.jar)

- The macOS `/usr/bin/java` stub may fail to locate an installed JVM (e.g. under a
  restrictive sandbox). The verifier resolves `$JAVA_HOME/bin/java` when `JAVA_HOME`
  is set, so point it at a JDK home (e.g. `/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home`)
  instead of relying on launch-services discovery.

- The TLC **config parser** accepts only simple constant values: numbers,
  identifiers, strings, and sets. It rejects function-constructor `|->` syntax,
  tuples `<<>>`, and a trailing `====` line. To give the model a function-valued
  configuration (e.g. a hierarchy), make the universe a plain set constant and
  derive the function as an operator in the module, or make it a variable that
  the actions grow.
- TLC's `EXCEPT` does **not** add new domain elements: `[f EXCEPT ![x] = v]`
  with `x \notin DOMAIN f` warns and leaves `f` unchanged. When an action grows
  a function's domain, rebuild the function explicitly over the new domain
  (e.g. `[g \in live' |-> IF g = f THEN ... ELSE f[g]]`).
- TLC cannot quantify **temporal formulas** over a variable set
  (`\A f \in live: WF_vars(...)` fails at parse/evaluation time). Quantify over
  a constant universe with a live guard, and keep the per-frame state total
  over that universe (default values for not-yet-created elements) — applying
  a function outside its domain is a hard TLC error, not a disabled action.
- TLC2 resolves operator definitions in order of appearance — no forward
  references (relevant when generating trace-data modules: define the
  referenced operators before the operators that use them).
- Every action must list every variable it does not assign in `UNCHANGED`,
  including ones it only reads; TLC errors on a successor state it cannot
  fully specify.
- The `f \in live` guard also protects the trace consumer: a trace event
  referencing a frame that was never created fails the validation at that
  entry (deadlock at that state) instead of crashing TLC.

### Model fidelity lessons

These hold for every action added to a trace model:

- **A variable no event can set is a hole in the model.** The render-cycle
  model carried an `animating` flag that no trace event ever set, so animated
  pages diverged from the model in a way that only surfaced as a rejected
  trace. The user agent now traces the next cycle's notes an animating
  composition queues (`NoteUiStateUpdate`), which is what the code does, and
  the variable is gone.
- **Clear state where the code clears it.** Graphics satisfies the render
  cycle's deadline when the top-level frame is *stored* (`FrameStored`), not
  when content runs the update: a deadline composition between those two points
  is real, and the model rejected it while it treated `UpdateTheRendering` as
  the end of the wait.
- **A composition can be delivered later than it was decided.** Graphics marks
  a deadline composition when the deadline expires and delivers it once the
  frames it waits on are stored, so `Deadline` carries no state guard; its
  effects are gated the way the user agent gates them (an update it released is
  what leaves batched notes to service).
- **Follow the code's order when a navigable is destroyed.** The user agent
  removes the traversable it was told about and destroys the document (and its
  descendants) from there, so `RemoveNavigable` removes the subtree and the
  descendants' own removal events are consumed as already applied.
- **A trace that overflows a model bound is the bound's problem if the code is
  right.** `TraceCommandDepth` is a replay bound, not a property of the
  pipeline: it has to exceed the deepest backlog the recorded pages reach.