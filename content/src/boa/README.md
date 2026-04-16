`content/src/boa` integrates Boa with the content-process DOM runtime.

- `content/src/html/environment_settings_object.rs` owns the Boa `Context`, global-object construction, and the Rust state that corresponds to an HTML environment settings object.

- `content/src/html/global_scope.rs` caches the platform objects associated with one global object so repeated wrapper lookups reuse the same `JsObject` identity.
- Keep per-global timer callback state with `GlobalScope` alongside animation-frame state, but route waiting through the Lean timer worker and re-enter `EnvironmentSettingsObject` with explicit timer commands instead of polling or sleeping in the content runtime.
- Invalidate cached node wrappers before DOM mutations that drop subtrees so removed node ids cannot be reused with stale `JsObject` identities; if the mutator does not report dropped ids directly, derive the affected subtree ids before applying the mutation.

- Parser-discovered classic scripts run through the dedicated parser-script list in `html_parser.rs`; the content runtime now defers their execution until the document-finish-parsing continuation sees that external scripts and critical resources are ready or have timed out, while Boa's maintained queue integration point remains the job queue used for microtasks.

- `html_parser.rs` connects html5ever parsing to Blitz mutation, records parser errors, and collects classic inline scripts in document order.

- Bindings in `content/src/boa/bindings` should convert arguments, select the right carrier object, and delegate. If a JavaScript-visible algorithm needs DOM or runtime state, move that logic onto the DOM carrier or Boa runtime struct that owns the state.

- Document runtime structs against HTML concepts such as `#environment-settings-object` and `#global-object` instead of documenting them as if they were DOM interfaces.