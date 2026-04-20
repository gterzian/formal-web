`content/src/dom` stores the native data carried by JavaScript-visible DOM objects.

- `BaseDocument` remains the authoritative DOM tree. The Rust carrier structs in this directory keep references, cached event-listener state, and algorithm entry points for JavaScript-visible behavior.

- HTML global-object carriers such as `GlobalScope` and `Window` live in `content/src/html`; DOM dispatch and event-target code in this directory can depend on those HTML-owned carriers when the DOM Standard talks about Window objects.

- `Document` and `Element` compose `Node` so shared node algorithms live on `Node`, while document-specific and element-specific Web IDL behavior lives on their respective carrier types.

- ParentNode mixin algorithms such as `querySelector()` and `querySelectorAll()` belong on both `Document` and `Element`; detached element subtrees still need selector queries during pure-JS DOM construction.

- When a Web IDL attribute or algorithm naturally belongs to one of these DOM types, implement it here with spec-linked methods and keep `content/src/boa/bindings` as a thin conversion layer.

- Blitz UI-event integration that turns native `UiEvent` values into DOM dispatch belongs in `content/src/dom`, not in `content/src/boa`.

- Activation-triggering native events should flow through `content/src/dom/dispatch.rs`. Compute the activation target during DOM dispatch and invoke HTML-defined activation behavior from that algorithm; do not special-case anchor navigation in the UI-event bridge.

- Keep `activationTarget` selection in `dispatch.rs` aligned with the DOM dispatch algorithm's target-first assignment and bubbling-parent fallback, rather than treating it as a post-dispatch HTML lookup.

- Use the checked-in standards under `/web_standards` for DOM, HTML, and UI Events anchors and verbatim `Step N:` comments. Never quote specs from memory: only use the local sources.

- "Missing Feature:" comments identify major missing features in the code. Only address those if given a clear implementation plan.

- "TODO:" comments identify minor missing fixes or features. Those can be addressed in batches when asked to do so.