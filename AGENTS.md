When learning something general about the project, update this file with the lesson.

When the user gives general project or documentation instructions, record them here proactively even
if they may later be split into more modular instruction files.

This file is for durable project-wide lessons and guidance only, not task-by-task work logs or progress notes.

- Lean library modules live under `FormalWeb/` and are re-exported from `FormalWeb.lean`.
- Keep top-level navigation orchestration and the LTS in `FormalWeb/UserAgent.lean`; move shared document, session-history, navigation, and traversable models/helpers into broader companion modules such as `Document.lean`, `SessionHistory.lean`, `Navigation.lean`, and `Traversable.lean` instead of many tiny files.
- Use CamelCase filenames such as `FormalWeb/UserAgent.lean` for modules like `FormalWeb.UserAgent`.
- Keep structure docstrings tied to the spec only when the type itself has a direct spec concept; put spec links on individual structure fields when the spec concept belongs to the member.
- Prefer single-line doc comments for spec-only documentation such as `/-- https://html.spec.whatwg.org/multipage/#anchor -/`.
- Keep docs minimal: default to just the spec link when a precise spec anchor exists, and add prose only when it carries real modeling information.
- Document structure fields with spec-slot links wherever the standard exposes a corresponding slot; if a field is model-local, say so explicitly and link the closest relevant spec concept.
- For spec algorithms, document the Lean function with the spec link and annotate the body with `Step n: ...` comments using verbatim spec prose.
- For spec algorithms, keep `Step n:` comments for spec prose only; add separate `Notes:` comments for concrete modeling or implementation details.
- Model shared spec algorithms at the least-specific spec type that the standard uses; for example, `initialize-the-navigable` should take a `Navigable`, not a `TraversableNavigable`, since the spec reuses it for child navigables.
- For partially implemented algorithm steps in Lean, put a `-- TODO:` comment immediately below the corresponding step comment.
- When a spec algorithm calls another algorithm, model that callee as a separate Lean function.
- In Lean code, prefer `do`-notation and monadic binds or pattern lets such as `let x <- ...` / `let some x := ... | ...` over nested `match` expressions when a function already returns `Option`, `IO`, or another monad.
- For small state-threading helpers, prefer `Option.map`, `Option.getD`, and local helper bindings over repeating the same inline `match` expressions when that keeps the spec-facing control flow unchanged.
- For pure state-transition helpers that thread state and return it, it is acceptable to use `Id.run do` so early exits can be expressed with pattern lets instead of nested `match` expressions.
- The author is not a Lean expert, so make an effort to translate user instruction freely into equivalent idiomatic Lean constructs. 
- Web standards are in a local-only folder name web_standards. Search these files to document your work as noted above.
- Prefer a custom Lake `target` plus `moreLinkObjs` for repo-local Rust static libraries; reserve `extern_lib` for cases that truly need it.
- Keep upstream Rust checkouts in `scratchpad/` for reference only; if the build depends on local Blitz crates, copy the required subset under `ffi/vendor/` and depend on that vendored copy instead of `scratchpad/` paths.
- For Lean FFI callbacks from Rust, prefer exporting a Lean function and calling it from Rust through a tiny C shim that includes `lean/lean.h`; Rust alone cannot directly use Lean's many inline runtime helpers such as `lean_dec`, `lean_string_cstr`, and `lean_io_result_*`.
- When searching the local HTML standard, search for the exact spec anchor string first, such as `creating-a-new-top-level-traversable`.
- For concurrent spec algorithms such as fetch-and-wait navigation steps, prefer modeling the pause point as explicit pending state on `UserAgent` plus a separate resume transition before introducing real runtime tasks or I/O.
- When a spec algorithm pauses and later resumes after a wait point, model the resumed portion as an explicit continuation helper instead of re-entering the top-level algorithm at a later argument state.
- When a spec wait has multiple wakeup conditions, model each wakeup reason explicitly in the LTS; if one branch produces no result and just returns, represent that as its own continuation path instead of folding it into the response-arrival case.
- Introduce a small LTS action type above navigation helpers so spec-visible concurrent steps such as "begin navigation" and "fetch response arrives" are explicit labels, while helper functions remain implementation detail under those labels.
- It is acceptable for the LTS layer to factor a convenience spec helper into multiple explicit labels, such as separating top-level traversable creation from the later begin-navigation and fetch-completion steps.
- If a spec convenience helper only bundles multiple LTS-visible steps, prefer modeling those steps directly in the action system and omit the convenience helper unless it still carries independent explanatory value.
- If a spec algorithm is only a top-level entry point and is not referenced by other spec algorithms, it does not need to be preserved as a separate helper in the model when the LTS already captures its intended behavior.
- A guide on Lean FFI can be found in `/scratchpad/ffi_guide/md`.
- On the current Lean 4.29 toolchain, use `Std.Data.TreeMap` for standard ordered maps; the verified API here is `Std.TreeMap.empty`, `map.insert`, and `map.get?`.
- Use `Std.Channel` (from `Std.Sync.Channel`) for multi-producer multi-consumer channels in Lean runtime code; `Channel.new`, `Channel.send`, `Channel.recv` are the core async API.
- `Std.Channel.forAsync` runs a `BaseIO` callback, so if a runtime worker needs `IO` effects such as `IO.println`, prefer an `IO.asTask` loop that awaits `Channel.recv` / `Channel.send` with `IO.wait`.
- For main-thread callbacks such as winit handlers, prefer non-blocking `Channel.trySend` on unbounded channels over waiting on `Channel.send`, to avoid stalling UI event delivery.
- For Rust-to-Lean UI callbacks, keep the callback body minimal and offload follow-up work with `IO.asTask` so the host event loop can return promptly.
- If a Lean background worker must stop when `main` exits, use `Std.CloseableChannel` rather than `Std.Channel`, close it after the host event loop returns, and wait for the worker task to finish.
- Validated runtime pattern: start background Lean workers with `IO.asTask`, feed them from UI callbacks via non-blocking `trySend`, have the worker loop terminate on `CloseableChannel.recv = none`, and in `main` clear shared refs, close the channel, and `IO.wait` the worker after `runWinitEventLoop` returns.
- Rendering-opportunity semantics: `user_agent_note_rendering_opportunity` is triggered when `request_redraw` is called, so the rendering-opportunity worker should run while the main winit loop is waiting for `WindowEvent::RedrawRequested`; the `RedrawRequested` handler should only consume a ready paint payload if one is available.
- Route supported winit input events through the Rust-side Blitz `UiEvent` translation first, then send a `DispatchEvent|<serialized event>` user-agent task message to Lean, and let Lean apply that event to the active document via FFI.
- The current runtime bootstrap uses a checked-in `file://` artifact URL, so the model's temporary fetch-scheme gate must treat `file://` as fetchable until startup moves to a served `http(s)` URL or a dedicated non-fetch branch.
- `RustBaseDocumentPointer` (in `FFI.lean`) is the opaque Lean handle for a Rust `BaseDocument*`; distinct from `RustDocumentPointer` which holds the full `HtmlDocument*`.
- Validated render fix for the current host path: the Blitz logo SVG painted successfully only after both conditions were met together: the root `<svg>` had explicit `width`/`height`, and the `formalwebffi` crate enabled the vendored Blitz `svg` features needed to parse and paint inline SVG.
- For refinement proofs about the executable runtime in `Main.lean`, factor the message-handling logic into a pure runtime model and prove each handler step is a stutter or a short trace of allowed `FetchAction`/`UserAgentAction` transitions.
- In user-agent refinement proofs, do not use `TransitionTrace.nil` as a fallback for spec-visible messages; empty traces should be reserved for explicitly silent runtime messages or explicit error/rejection cases, so adding a new visible `UserAgentTaskMessage` forces a corresponding `UserAgentAction`/`step` case.
- For direct startup-trace proofs over `createNewTopLevelTraversable`, a workable pattern is to prove the returned traversable is present in `topLevelTraversableSet` by unfolding `createNewTopLevelTraversableImpl` and simplifying with `TopLevelTraversableSet.appendFresh`, `TopLevelTraversableSet.replace`, and `TopLevelTraversableSet.nextId`.
- For deterministic runtime traces over the pure `RuntimeState` machine, lift total `runtimeStep` into `TransitionTrace` with `runtimeTraceStep := fun s a => some (runtimeStep s a)` and prove the multi-step execution trace with a `runtimeExec` fold over the action list.
- For list-level refinement over runtime action sequences, first prove a generic `TransitionTrace.append` lemma, then combine one-step projection refinements by induction over `runtimeExec`.
- Keep `Main.lean` aligned with the proof model by serializing all runtime work through one `RuntimeAction` queue and threading `RuntimeState` with `runtimeExec state [action]` in the worker loop.
- When a runtime worker wraps a pure `handle...Pure` transition that already returns the next state plus side-effect intents, reuse `result.state` in the IO wrapper instead of recomputing the same single-message step separately.
- If fetch work is spawned directly as an external side effect of handling runtime messages, keep the runtime queue typed by `RuntimeMessage`, update `RuntimeState.fetch` inside `handleRuntimeMessagePure`, and have background fetch tasks report completion with a `fetchCompleted controllerId ...` runtime message.
- Keep subsystem runtime plumbing and local refinement proofs with the subsystem model: `runUserAgent` and user-agent-only proofs live in `FormalWeb/UserAgent.lean`, while `runFetch` and fetch-only proofs live in `FormalWeb/Fetch.lean`; any cross-subsystem consistency argument can sit separately as propositional glue rather than as another monolithic LTS.
- It is acceptable to keep `Main.lean` on separate runtime workers while introducing a proof-only combined runtime module under `FormalWeb/` that serializes both subsystems into one pure queue for cross-subsystem interaction theorems.
- Keep cross-subsystem runtime interaction proofs in `FormalWeb/RuntimeProof.lean` so `FormalWeb/UserAgent.lean` stays focused on the executable user-agent runtime plus user-agent-local proofs.
- Use `lean-lsp-mcp` when working in Lean, it is documented at `/scratchpad/lean-lsp-mcp/README.md`. Read the documentation before attempting to prove anything in Lean.