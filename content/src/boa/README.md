# content/src/boa

`content/src/boa` integrates Boa with the content-process runtime and keeps JavaScript-facing wrapper identity separate from DOM and HTML carrier state.

- `content/src/html/environment_settings_object.rs` owns the Boa `Context`, global-object construction, and the Rust state that corresponds to an HTML environment settings object.
- `content/src/html/global_scope.rs` owns per-global wrapper caches and callback state so repeated lookups reuse the same `JsObject` identity.
- `html_parser.rs` bridges html5ever parsing to Blitz mutations, records parser errors, and collects parser-discovered classic scripts.
- `content/src/boa/bindings` should convert arguments, downcast carriers, and delegate; stateful algorithms belong on the owning DOM, HTML, Streams, or runtime type.
- Run microtask checkpoints at task boundaries rather than after every Rust-to-JavaScript callback.
- Document runtime structs against HTML concepts such as `#environment-settings-object` and `#global-object`, not as ad hoc DOM interfaces.