# content/src/dom

`content/src/dom` stores the native [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) for the JavaScript-visible DOM interfaces and the DOM Standard algorithms that operate on them.

- The blitz `BaseDocument` is the authoritative DOM tree; `Document` and `Element` compose `Node`, so shared tree algorithms live on `Node` while type-specific Web IDL behavior stays on the owning [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object).
- HTML-owned global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) (`GlobalScope`, `Window`) live in `content/src/html`; DOM dispatch code here depends on them when the DOM Standard talks about window-backed targets.
- Native UI-event to DOM-dispatch bridging lives in `content/src/html/ui_events.rs`; activation-target selection stays in `dispatch.rs`.
- `content/src/js/bindings` delegates DOM algorithms here — bindings never embed DOM logic.
- Use the `web_standards` extension (`spec_lookup`) with `https://dom.spec.whatwg.org/` to read the DOM spec.  For single-sentence spec definitions, quote the defining sentence instead of inventing `Step N:` comments.

## Event dispatch

The dispatch algorithm and its data types live in `dispatch.rs` and `event.rs`, each function and field carrying its spec anchor and verbatim `// Step N:` comments; the step-by-step mapping lives there, not here.  Two module conventions are not visible from the code alone:

- **The path is built by the caller, and the dispatch loop never sees a `JsObject`.**  `dispatch_with_path` takes a pre-built `&[EventPathItem]`; callers resolve JsObjects to `EventTarget`s and build the path (`build_path_for_target` for domain callers, `build_event_path` in `html/ui_events.rs` and `build_path_from_target_js_object` in `js/platform_objects.rs` for JS-driven dispatch).  Domain dispatch operates on `EventTarget` values only — see `AGENTS.md`, "Event path building is the caller's responsibility".
- **Anchor activation behavior runs in the JS layer.**  `dispatch_event` selects the activation target from `EventPathItem.has_activation_behavior` (dispatch step 6.9.6.1), but running the anchor's activation behavior needs the realm and its navigation context, so step 12 calls `js/platform_objects.rs::run_activation_behavior_for_path` — the one place the dispatch loop leaves the domain.

Mutable dispatch state (`event_listener_list`, `Event.target`/`currentTarget`, flags) lives in `GcCell`/`Cell` fields behind `&self` methods, so cloning an `EventTarget` or `Event` shares the underlying state — mutate through the clone, never sync back.

### What NOT to do

- Do not use `&mut self` on `Event` or `EventTarget` methods — use `GcCell` fields with `&self`.
- Do not clone an `Event`/`EventTarget`, mutate the clone, and sync the result back — `GcCell` shares data across clones.
- Do not put JsObject-only helpers (like `event_target_from_object`) in `dispatch.rs` — keep domain dispatch pure.  Such helpers belong in `js/platform_objects.rs` or `html/ui_events.rs`.
