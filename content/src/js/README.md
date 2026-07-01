# content/src/js

`content/src/js` integrates the generic `js_engine` trait with the content
process and keeps JavaScript-facing wrapper identity separate from DOM and
HTML [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object)
state.  The actual engine backend (Boa or JSC) is selected by a feature flag
in `js_engine`; content code only sees the generic traits.

- `content/src/html/environment_settings_object.rs` owns the realm execution
  context (currently `BoaContext` implementing `ExecutionContext<T>`),
  global-object construction, and the Rust state that corresponds to an HTML
  environment settings object.
- `content/src/html/global_scope.rs` owns per-global wrapper caches and
  callback state so repeated lookups reuse the same `JsObject` identity.
- `html_parser.rs` bridges html5ever parsing to Blitz mutations, records
  parser errors, and collects parser-discovered classic scripts.
- **`content/src/js/bindings/` is the single home for Web IDL binding
  definitions** — DOM, HTML, Streams, WebAssembly, CSS, or any other spec.
  Each binding:
  - Implements `WebIdlInterface` or `WebIdlNamespace` to define *which
    members* the interface or namespace exposes.
  - Provides thin getter/setter/method functions that convert JavaScript
    arguments and delegate to domain-level implementations.
  - Uses the Web IDL bindings infrastructure (`WebIdlInterface`,
    `WebIdlNamespace`, `register_interface_spec`, `register_namespace_spec`,
    etc.) from `content/src/webidl/bindings/` instead of calling Boa directly.
- **Domain logic belongs in the domain directory; JS-interop code belongs
  in the bindings.**  Pure Rust/wasmtime logic goes in the owning domain
  directory (`content/src/dom/`, `content/src/html/`, `content/src/streams/`,
  `content/src/wasm/`).  `WebIdlInterface` implementations, promise
  resolution, object construction, and any code returning `JsValue` goes in
  `content/src/js/bindings/`.  The binding code converts arguments, checks
  [inherited
  interfaces](https://webidl.spec.whatwg.org/#dfn-inherited-interfaces) to
  identify the platform object's type, and delegates to domain functions.
- **Domain code must not depend on `boa_engine` or return `JsValue`.**
  The domain layer returns Rust types; the bindings layer converts to JS
  values as late as possible.
- Run microtask checkpoints at task boundaries rather than after every
  Rust-to-JavaScript callback.
- Document process structs against HTML concepts such as
  `#environment-settings-object` and `#global-object`, not as ad hoc DOM
  interfaces.

## Exotic objects

Some HTML spec objects (WindowProxy, Location) require exotic internal methods.
The `InternalObjectMethods` vtable (`pub(crate)`) is not accessible from outside
the `boa_engine` crate.  Exotic objects must be implemented using only public
Boa APIs — primarily `JsProxyBuilder` (from `boa_engine::object::builtins`)
for proxy-based exotic objects.

See `content/src/webidl/README.md` for the exotic-object implementation pattern,
and `content/src/html/windowproxy.rs` for a concrete example.

### Working with Boa's public API: use spec links, not `pub(crate)` internals

Boa is an external dependency of the content crate (via crates.io or GitHub).
The content crate **must not** depend on any `pub(crate)` internal function,
type, or method
inside Boa.  Instead, follow this methodology:

1. Read the relevant spec (e.g. HTML §7.2.3 The WindowProxy exotic object)
   using `spec_lookup`.
2. Look at the **index of links** at the bottom of the spec section — each
   JS operation references an ECMAScript spec algorithm by URL, e.g.
   [`OrdinaryGetPrototypeOf`](https://tc39.es/ecma262/#sec-ordinarygetprototypeof)
   or [`OrdinaryGetOwnProperty`](https://tc39.es/ecma262/#sec-ordinarygetownproperty).
3. Check if there is an **already-public equivalent** in Boa:

   | ECMAScript operation | Public Boa API |
   |---|---|
   | `ProxyCreate(target, handler)` | `JsProxyBuilder::new(target)...build(context)` |
   | `OrdinaryGetPrototypeOf` | `JsObject::prototype()` |
   | `OrdinaryIsExtensible` | `JsObject::is_extensible(context)` |
   | `OrdinaryGet` | `JsObject::get(key, context)` |
   | `OrdinarySet` | `JsObject::set(key, value, throw, context)` |
   | `OrdinaryDelete` | `JsObject::delete_property_or_throw(key, context)` |
   | `OrdinaryHasProperty` | `JsObject::has_property(key, context)` |
   | `OrdinaryOwnPropertyKeys` | `JsObject::own_property_keys(context)` |

4. When the proxy pattern is needed (WindowProxy, Location, etc.), use
   `JsProxyBuilder` from `boa_engine::object::builtins`.  This public API
   lets you supply each trap as a plain `NativeFunctionPointer` — no captures,
   no custom handler struct, no access to `pub(crate)` internals.

5. When no existing public method covers the exact operation needed (e.g.,
   getting a raw `PropertyDescriptor` for [[GetOwnProperty]]), restructure
   the implementation to use the available public methods, or submit a PR
   to the upstream Boa project to add the missing public wrapper.

**Never modify the external Boa dependency to make internal APIs public.**

The WindowProxy currently uses `JsProxyBuilder` (the public Boa API) to
create a proper Proxy with native-function traps for all 10 overridden
internal methods (see `content/src/html/windowproxy.rs`).  This avoids any
`pub(crate)` access.  When cross-origin support requires additional
internal-method overrides, follow the same pattern — use `JsProxyBuilder`
traps backed by the public `JsObject` methods above.

## Adding a new HTML element type

Every HTML element exposed to JavaScript needs entries in several dispatch
tables.  When the existing set (HTMLAnchorElement, HTMLIFrameElement,
HTMLInputElement, HTMLMediaElement, HTMLVideoElement) doesn't cover a new
tag, add a domain struct in `content/src/html/`, a `WebIdlInterface` impl in
`content/src/js/bindings/html/`, then wire it into each of the following:

1. **`content/src/html.rs`** — declare the module and re-export the type.
2. **`content/src/js/bindings/html/mod.rs`** — declare the bindings module.
3. **`content/src/js/bindings/html/host_hooks.rs`** —
   - Call `reg!(NewType);` alongside the existing `reg!` calls.
   - Add a `wire_registry_prototype::<NewType, ParentType>(&mut context);`
     line to link the new type's prototype into the inheritance chain.
     This is **required** even though `parent_name()` returns `Some(...)`
     — the parent lookup via `parent_name()` is not yet automatic.  Without
     this call the new type's prototype falls back to `%Object.prototype%`
     and inherited methods (`addEventListener`, `dispatchEvent`, etc.)
     will not be found.
4. **`content/src/js/platform_objects.rs`** — add a new `kind` value in
   `resolve_element_object` for the tag name, and a matching
   `create_interface_instance` arm.
5. **`content/src/js/bindings/dom/element.rs`** — add a downcast arm in
   `with_element_ref` for the new type.  Also add arms in `class_list_value`
   and `class_list_set_value` if they use the element-punning pattern.
6. **`content/src/js/bindings/html/html_element.rs`** — add a downcast arm
   in `try_with_html_element_ref`, and arms in `element_style_attribute_ec`
   and `set_element_style_attribute_ec`.
7. **`content/src/js/downcast.rs`** — add arms in both
   `with_event_target_mut` and `with_event_target_ref`.
8. **`content/src/dom/dispatch.rs`** — add an arm in `path_for_target`.

The prototype chain is only partially automatic.  The `register_interface_spec`
code sets up each prototype object and registers its members, but the
prototype-to-parent linkage is done by explicit `wire_registry_prototype`
calls in `host_hooks.rs`.  Each new type that inherits from an existing
interface must have a corresponding `wire_registry_prototype` line.

## Related

- `content/src/webidl/README.md` — Boa platform object integration, exotic pattern
- `content/src/html/README.md` — WindowProxy, window.open, navigation split
