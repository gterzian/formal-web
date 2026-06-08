# content/src/dom

`content/src/dom` stores the native [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) for the JavaScript-visible DOM interfaces and the DOM Standard algorithms that operate on them.

- `BaseDocument` remains the authoritative DOM tree and document state.
- `Document` and `Element` compose `Node`, so shared tree algorithms live on `Node` while type-specific Web IDL behavior stays on the owning [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object).
- HTML-owned global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) such as `GlobalScope` (implementing the [global object](https://html.spec.whatwg.org/#global-object) concept) and [Window](https://html.spec.whatwg.org/#window) live in `content/src/html`, and DOM dispatch code here depends on them when the DOM Standard talks about window-backed targets.
- `content/src/js/bindings` should delegate DOM algorithms here instead of embedding DOM logic in the binding layer.
- Native UI-event to DOM-dispatch bridging belongs here, with activation-target selection kept in `dispatch.rs`.
- Use the `web_standards` extension (`spec_lookup`) with `https://dom.spec.whatwg.org/` to read the DOM spec, and for single-sentence spec definitions quote the defining sentence instead of inventing `Step N:` comments.