# content/src/dom

`content/src/dom` stores the native carriers for JavaScript-visible DOM objects and the DOM Standard algorithms that operate on them.

- `BaseDocument` remains the authoritative DOM tree and document state.
- `Document` and `Element` compose `Node`, so shared tree algorithms live on `Node` while type-specific Web IDL behavior stays on the owning carrier.
- HTML-owned global-object carriers such as `GlobalScope` and `Window` live in `content/src/html`, and DOM dispatch code here depends on them when the DOM Standard talks about window-backed targets.
- `content/src/boa/bindings` should delegate DOM algorithms here instead of embedding DOM logic in the binding layer.
- Native UI-event to DOM-dispatch bridging belongs here, with activation-target selection kept in `dispatch.rs`.
- Use the `web_standards` extension (`spec_section`, `spec_algorithm`) with `https://dom.spec.whatwg.org/` to read the DOM spec, and for single-sentence spec definitions quote the defining sentence instead of inventing `Step N:` comments.