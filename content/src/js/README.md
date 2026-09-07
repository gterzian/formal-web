# content/src/js

`content/src/js` is the content crate's JS integration layer.  It provides
type aliases (`Types`, `Engine`) pointing to the concrete `js_engine` backend
(selected by feature flag in the top-level `js_engine/` crate), and keeps
JavaScript-facing wrapper identity separate from DOM and HTML
[platform object](https://webidl.spec.whatwg.org/#dfn-platform-object)
state.  Content code only sees the generic traits from the `js_engine` crate.

The three-layer split (domain → Web IDL infra → JS bindings glue) that this
layer is the outer half of is documented once, in
`content/src/js/bindings/README.md` — read it before writing bindings.

## Layout

- `content/src/html/environment_settings_object.rs` owns the realm execution
  context (the selected backend's engine implementing `ExecutionContext<T>`),
  global-object construction, and the Rust state that corresponds to an HTML
  environment settings object.
- `content/src/html/global_scope.rs` owns per-global wrapper caches and
  callback state so repeated lookups reuse the same `JsObject` identity.
- `content/src/js/bindings/` holds the `WebIdlInterface` / `WebIdlNamespace`
  impls and the thin binding functions; see its `README.md`.
- `console_generic.rs` and `css_generic.rs` still install their namespaces
  by hand (`create_plain_object` + `create_builtin_fn`) instead of
  `WebIdlNamespace` + `register_namespace_spec`; migrate them.

## Conventions for the domain/bindings boundary

Rules specific to code living under or touching this crate:

- **Domain code must not store a `GlobalScope` on non-global platform
  objects.**  A gc struct has no memory of the realm it was created in; the
  general accessor is `with_global_scope(ec, ...)`, which resolves the
  *current* realm's global object.  Operations on a platform object always
  run in its own realm (bindings run in the creation realm; objects created
  during deserialization are created in the target realm before any use),
  so the current realm is the object's realm.  Only `Window` stores its
  `GlobalScope`, because it is the realm's global object.
- **Domain code returns the platform object struct, not a raw `JsObject`
  handle, when a record already holds the object.**  The JS handle is only
  needed at the JS boundary (reflectors, bindings); identity comparisons
  between platform objects (e.g. "transfer contains targetPort") compare
  their unique ids instead.
- Run microtask checkpoints at task boundaries rather than after every
  Rust-to-JavaScript callback.
- Document process structs against HTML concepts such as
  `#environment-settings-object` and `#global-object`, not as ad hoc DOM
  interfaces.

## Exotic objects

Some HTML spec objects (WindowProxy, Location) require exotic internal
methods (they override `[[Get]]`, `[[Set]]`, `[[GetPrototypeOf]]`, …).  The
generic `ExecutionContext` builds them as proxies: each trap is a function
created with `ec.create_builtin_fn()`, set as a property on a handler
object, and handed to `ec.create_proxy(target, handler)`.  Content code only
ever calls the generic trait method — each backend executes `create_proxy`
natively.  The concrete patterns (the WindowProxy traps, the Boa
`InternalObjectMethods`/`%Proxy%` routes) are documented in
`content/src/html/windowproxy.rs` and `content/src/webidl/README.md`
("Exotic objects and custom internal methods").

### Working with the engine's public API: use spec links, not `pub(crate)` internals

The JS engine is an external dependency of the content crate, and content code
sees it only through the generic traits in `js_engine`.  The content crate
**must not** depend on any `pub(crate)` internal function, type, or method
inside a backing engine (on the Boa backend this means using only public Boa
APIs).  Instead, follow this methodology:

1. Read the relevant spec (e.g. HTML §7.2.3 The WindowProxy exotic object)
   using `spec_lookup`.
2. Look at the **index of links** at the bottom of the spec section — each
   JS operation references an ECMAScript spec algorithm by URL, e.g.
   [`OrdinaryGetPrototypeOf`](https://tc39.es/ecma262/#sec-ordinarygetprototypeof)
   or [`OrdinaryGetOwnProperty`](https://tc39.es/ecma262/#sec-ordinarygetownproperty).
3. On the Boa backend, check for an **already-public equivalent** in Boa:

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

4. When the proxy pattern is needed (WindowProxy, Location, etc.), use the
   generic `ec.create_proxy`; on the Boa backend this is `JsProxyBuilder`
   from `boa_engine::object::builtins`, which lets you supply each trap as a
   plain `NativeFunctionPointer` — no captures, no custom handler struct, no
   access to `pub(crate)` internals.
5. When no existing public method covers the exact operation needed (e.g.,
   getting a raw `PropertyDescriptor` for [[GetOwnProperty]]), restructure
   the implementation to use the available public methods, or contribute the
   missing public wrapper upstream (to `rusty_v8` or Boa).

**Never modify the external engine dependency to make internal APIs public.**

## Adding a new HTML element type

Every HTML element exposed to JavaScript needs entries in several dispatch
tables.  When the existing set (HTMLAnchorElement, HTMLIFrameElement,
HTMLInputElement, HTMLMediaElement, HTMLVideoElement) doesn't cover a new
tag, add a domain struct in `content/src/html/`, a `WebIdlInterface` impl in
`content/src/js/bindings/html/`, then wire it into each of the following:

1. **`content/src/html.rs`** — declare the module and re-export the type.
2. **`content/src/js/bindings/html/mod.rs`** — declare the bindings module.
3. **`content/src/js/build_context.rs`** (`setup_realm`) —
   - Call `reg!(NewType);` alongside the existing `reg!` calls.
   - Add a `wire_registry_prototype::<NewType, ParentType>(engine);`
     line to link the new type's prototype into the inheritance chain.
     This is **required** even though `parent_name()` returns `Some(...)`
     — the parent lookup via `parent_name()` is not yet automatic.  Without
     this call the new type's prototype falls back to `%Object.prototype%`
     and inherited methods (`addEventListener`, `dispatchEvent`, etc.)
     will not be found.
   - Add a `wire_registry_constructor_prototype::<NewType, ParentType>(engine);`
     line so the interface object inherits from its parent's interface object.
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
prototype-to-parent and constructor-to-parent linkages are done by explicit
`wire_registry_prototype` / `wire_registry_constructor_prototype` calls in
`build_context.rs`.  Each new type that inherits from an existing interface
must have the corresponding wiring lines.

## Event platform-object downcast convention

Every Event platform-object type (Event itself and its subclasses UIEvent,
MouseEvent, …) embeds the base `Event` — `Event` is the type itself, subclasses
carry it as an `event` field (possibly through their parent chain).  Each type
implements `crate::dom::event::HasEvent` (`event()` / `event_mut()`), and
`crate::js::downcast::event_from_js_object` walks the known types once.  Code
that needs the embedded `Event` from a JS object calls that helper; it must
**not** hand-roll its own downcast chain.  When adding a new Event subclass,
embed the parent type (which embeds `Event`), implement `HasEvent`, and add an
arm to `event_from_js_object`.

The UI Events spec types (UIEvent, MouseEvent) live in `content/src/ui_events/`,
not `content/src/dom/` — spec code is placed by which spec it implements.

## Related

- `content/src/js/bindings/README.md` — three-layer architecture and spec-annotation rules (definitive)
- `content/src/webidl/README.md` — platform-object integration and exotic-object backend notes
- `content/src/html/README.md` — WindowProxy, window.open, navigation split
