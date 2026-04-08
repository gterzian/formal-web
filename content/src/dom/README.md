`content/src/dom` stores the native data carried by JavaScript-visible DOM objects.

- `BaseDocument` remains the authoritative DOM tree. The Rust carrier structs in this directory keep references, cached event-listener state, and algorithm entry points for JavaScript-visible behavior.

- `GlobalScope` maps to HTML's global object concept. `Window` is one concrete global-object kind and composes `GlobalScope` rather than replacing it.

- `Document` and `Element` compose `Node` so shared node algorithms live on `Node`, while document-specific and element-specific Web IDL behavior lives on their respective carrier types.

- When a Web IDL attribute or algorithm naturally belongs to one of these DOM types, implement it here with spec-linked methods and keep `content/src/boa/bindings` as a thin conversion layer.

- Blitz UI-event integration that turns native `UiEvent` values into DOM dispatch belongs in `content/src/dom`, not in `content/src/boa`.

- Use the checked-in standards under `/web_standards` for DOM, HTML, and UI Events anchors and verbatim `Step N:` comments. Never quote specs from memory: only use the local sources.

- "Missing Feature:" comments identify major missing features in the code. Only address those if given a clear implementation plan.

- "TODO:" comments identify minor missing fixes or features. Those can be addressed in batches when asked to do so.