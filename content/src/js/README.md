# content/src/js

`content/src/js` integrates Boa (the JavaScript engine) with the content
process and keeps JavaScript-facing wrapper identity separate from DOM and
HTML [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object)
state.

- `content/src/html/environment_settings_object.rs` owns the Boa `Context`,
  global-object construction, and the Rust state that corresponds to an HTML
  environment settings object.
- `content/src/html/global_scope.rs` owns per-global wrapper caches and
  callback state so repeated lookups reuse the same `JsObject` identity.
- `html_parser.rs` bridges html5ever parsing to Blitz mutations, records
  parser errors, and collects parser-discovered classic scripts.
- `content/src/js/bindings` should convert arguments, check [inherited
  interfaces](https://webidl.spec.whatwg.org/#dfn-inherited-interfaces) to
  identify the platform object's type, and delegate; stateful algorithms
  belong on the owning DOM, HTML, or Streams platform object type.
- Run microtask checkpoints at task boundaries rather than after every
  Rust-to-JavaScript callback.
- Document process structs against HTML concepts such as
  `#environment-settings-object` and `#global-object`, not as ad hoc DOM
  interfaces.

## Exotic objects

Some HTML spec objects (WindowProxy, Location) require exotic internal methods.
Boa supports custom internal methods via `JsData::internal_methods()` returning
a custom `InternalObjectMethods` vtable. However, `InternalObjectMethods` and
`InternalMethodPropertyContext` are `pub(crate)` to `boa_engine`, so exotic
objects cannot be implemented from outside the engine crate without exposing
these types publicly.

See `content/src/webidl/README.md` for the exotic-object pattern and the current
workaround used by `WindowProxy`.

## Related

- `content/src/webidl/README.md` — Boa platform object integration, exotic pattern
- `content/src/html/README.md` — WindowProxy, window.open, navigation split
