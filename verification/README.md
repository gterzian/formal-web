# verification crate

The verification crate owns trace recording, TLA+ validation, and the shutdown workflow that ties them together.

- The main process starts the trace monitor, shares senders with local workers, and sends the same sender to the content and net processes after IPC bootstrap.
- Trace specs live under `verification/tla_specs/`, and recorded NDJSON logs plus TLC working files live in temporary directories that are removed after validation.
- `VerificationRun::finish()` requires all top-level `TraceSender` clones to be dropped so shutdown can observe channel closure and complete.
- Verification uses the local TLA+ Toolbox jar at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar` by default.
- `./verification/verify-specs.sh` is the canonical end-of-task verification: it starts the embedder headless with TLA+ tracing, runs a minimal WebDriver session against `verification/iframe-trace-page.html`, collects trace events, and validates all specs (Navigation, RenderingOpportunity, …) against their models.
- Both verification scripts rebuild every helper they spawn (embedder, content, net, graphics); keep that list complete, since a stale helper can leave the embedder blocked in shutdown (see `AGENTS.md`, "IPC wire format consistency").
- `./verification/verify-navigation.sh` is the hyperlink-navigation flow: it drives one link click (`StartupExample.html` → `navigated.html`) and treats successful `Navigation` TLA+ validation as the acceptance criterion. On failure, the human-readable report includes the failing NDJSON entry, preceding context entries, and the full TLC counterexample trace.

## Specs

| Spec | Events traced (producer) | Validates |
|---|---|---|
| `Navigation` | UA navigation lifecycle | navigable/navigation state machine |
| `RenderingOpportunity` | UA `CreateFrame` (the navigable-creation part of the navigation path, traced from `CreateNavigable`/`CreateChildNavigable`) /`NoteRenderingOpportunity`/`FrameNeeded`, content `UpdateTheRendering`, graphics `GraphicsComputed` (traced when the pixels are actually sent) | FrameNeeded-gated render cycle with double buffering: a render starts only when the embedder needs a frame (paced by vsync) AND a rendering opportunity was noted; the paint consumes the composed frames, and the pipeline never holds more than `BufferCount` (2) renders in flight (one displayed, one being rendered) |
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
its **child frames** (iframes). `FrameNeeded` and `GraphicsComputed` only ever
fire for top-levels (the embedder sends `frame_needed` for webviews; graphics
only computes when the root frame arrives), while
`NoteRenderingOpportunity`/`UpdateTheRendering` are traced for every frame. A
top-level paint services the batched opportunities of its whole hierarchy, and
a child's update also starts the top-level's update (their frames compose
together), mirroring `queue_update_the_rendering`/`queue_update_the_rendering_for_navigables`
in the user agent.

Frames are created dynamically by the `CreateFrame` action: the initial
top-level traversable (parent `NoParent`) and iframes (parent is an existing frame)
can be born at any time. The navigation path's graphics-relevant part —
`CreateNavigable`/`CreateChildNavigable` — is therefore traced for this spec
too as `CreateFrame`, so the model's frame set grows from the trace alone.
The verification session (`verify-specs.sh`) navigates to
`verification/iframe-trace-page.html`, which embeds a cross-site `file://`
iframe, so the child rendering path is actually traced and validated.

### RenderingOpportunity remaining work

The model abstracts all opportunity notes into `NoteRenderingOpportunity` and
relies on `FrameNeeded` fairness to eventually service a batched opportunity,
so it does not distinguish a cross-origin content change arriving out of band
(the graphics-side `CompositionChanged` that re-notes a non-top-level frame)
from any other note. That is fine for the physical-rendering simplification,
but it means the model cannot express the recovery for the stranded-opportunity
race: the UA's `queue_update_the_rendering_for_navigables` used to bail the
whole hierarchy batch when the top-level already had an update in flight,
leaving a static child's change un-drained until an unrelated input event.
The code now drains each navigable independently (so a child is serviced even
when the top-level is in flight); expressing that recovery in the model would
need a distinct `CompositionChanged`-style note event and a `request_redraw`
recovery action, and is deferred.

### Adding a spec

1. Write `{Name}.tla` (the model) and `{Name}Trace.tla` (the trace consumer) plus `{Name}.cfg` / `{Name}Trace.cfg` in `verification/tla_specs/`. Discovery is automatic from the flat `.tla` files.
2. Emit events from code with `verification::tla_log!(tracer, -> "{Name}", "Event", args...)`. Producers holding a `TraceSender`/`TLATracer` are the UA, the graphics process, and the embedder apps.
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