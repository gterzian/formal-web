# content/src/webidl

`content/src/webidl` implements the algorithms defined in Web IDL §3
(JavaScript binding).  It has two distinct roles:

1. **Domain-facing capabilities** — wrappers around JS operations used by
   other web standards (Streams, HTML, DOM): promise creation, promise
   reaction, type conversion, callback invocation.  These live at the
   `content/src/webidl/` top level (`promise.rs`, `callback.rs`, `buffer_source.rs`).

2. **JS binding infrastructure** — implements the Web IDL §3 algorithms
   for exposing platform objects to JavaScript: interface object creation,
   attribute/operation/constant definition, namespace registration.
   These live in `content/src/webidl/bindings/` and are the generic infra
   that `content/src/js/bindings/` calls into.

Every call through this layer ends up at abstract `js_engine` trait methods
(`ExecutionContext<T>`, `JsEngine<T>`); no engine-specific APIs leak above.
The three-layer split this crate sits in (domain → Web IDL infra → JS
bindings glue) is described once, in `content/src/js/bindings/README.md` —
do not restate it here.

## Domain-facing capabilities

- `promise.rs` implements the Web IDL promise algorithms
  (<https://webidl.spec.whatwg.org/#a-promise-resolved-with>,
  <https://webidl.spec.whatwg.org/#a-promise-rejected-with>,
  <https://webidl.spec.whatwg.org/#js-to-promise>,
  <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>,
  <https://webidl.spec.whatwg.org/#mark-a-promise-as-handled>,
  <https://webidl.spec.whatwg.org/#react>), each following its spec
  algorithm with `// Step N:` comments on the `ExecutionContext<T>` trait
  only.  `wait_for_all()` and `wait_for_all_get_promise()` are spec-complete
  but not yet wired to any domain call site.
- `callback.rs` implements <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
  (`call_user_objects_operation`), <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
  (`invoke_callback_function`), and the callback-interface/type conversions.
- `realm.rs` hosts HTML's direct-JS-call quirks — reads the HTML spec
  performs directly on realm state in place of a Web IDL step (e.g. the
  `window`/`frames`/`self` getters' "relevant realm.[[GlobalEnv]].
  [[GlobalThisValue]]").  See `content/src/js/bindings/README.md`, "When the
  spec calls into JS directly (not via Web IDL)" for why those reads live
  here rather than in a binding.

## JS binding infrastructure (`bindings/`)

`content/src/webidl/bindings/` implements the algorithms from Web IDL §3
JavaScript binding behind generic traits — NOT domain-specific — that the
bindings layer (`content/src/js/bindings/`) calls into:

| Module | Spec section |
|---|---|
| `interface.rs` | [#js-interfaces](https://webidl.spec.whatwg.org/#js-interfaces) — `WebIdlInterface`, `WebIdlNamespace`, `register_interface_spec`, `register_namespace_spec`, `create_interface_instance` |
| `attribute.rs` | [#js-attributes](https://webidl.spec.whatwg.org/#js-attributes) |
| `operation.rs` | [#js-operations](https://webidl.spec.whatwg.org/#js-operations) |
| `constant.rs` | [#js-constants](https://webidl.spec.whatwg.org/#js-constants) |
| `registry.rs` | domain registry (`InterfaceRegistry`, `register_in_host_defined`, `wire_prototype`) |

### Spec compliance gaps

The infra implements the interface/attribute/operation/namespace creation
algorithms of Web IDL §3 with the following deviations from the spec text
(the followed steps are in the code, each algorithm annotated; the gaps are
the work left):

- **Constructor and prototype wiring is partly manual.**  Constructor
  prototype inheritance from the parent interface and prototype-chain
  wiring happen through explicit `wire_registry_constructor_prototype` /
  `wire_registry_prototype` calls in `build_context.rs` — the
  `register_interface_spec` step 3 automatic linkage is not implemented
  (a new element type must add these lines; see
  `content/src/js/README.md`, "Adding a new HTML element type").
- **`[[Unforgeables]]`** (interface creation steps 4–7): not implemented;
  unforgeable attributes/operations are handled by `configurable: false`
  on the descriptor, not a shared `[[Unforgeables]]` object.
- **Overloaded constructors** (steps 1.1–1.7): not implemented — only
  single-argument constructors.
- **`[Exposed]` realm filtering** (attribute/operation step 1.1): not
  implemented — realm-based exposure checking is deferred.
- **Observable array types** (attribute step 1.8): not implemented.
- **Attribute getter `[[LegacyLenientThis]]`**: delegated to the
  user-provided getter rather than auto-generated; the `legacy_lenient_this`
  field exists on `AttributeDef` but is unused.
- **Operation `this`-value normalization, security checks, and overload
  resolution** (operation steps 2.1.1–2.1.5): delegated to the
  user-provided method function; the wrapper the spec generates is not
  auto-generated.
- **Namespace objects**: simple creation only — no namespace prototype
  handling or extended-attribute support.

## Design decisions

### `this`-value checking is manual

The Web IDL spec defines attribute getter/setter and operation function
creation algorithms that wrap `this`-value normalization and security
checks around the user-provided steps.  Our binding infra delegates this
to the user-provided function pointer (e.g., `try_with_html_iframe_element_ref`
in the binding functions).  This is a deliberate simplification: the
binding infra would need to know the interface type to generate the
`this`-checking code, which would require type-level dispatch or macros.

The check looks like:
```rust
let obj = T::value_as_object(this).ok_or_else(|| ec.new_type_error("..."))?;
if let Some(data) = ec.with_object_any(&obj) {
    if let Some(domain_obj) = data.downcast_ref::<MyInterface>() {
        return Ok(/* ... */);
    }
}
Err(ec.new_type_error("receiver is not a MyInterface"))
```

## Platform objects

Rust types that correspond to Web IDL interface types (e.g. `Window`,
`Document`, `HTMLAnchorElement`) are [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object).
In comments and documentation, refer to them as a platform object that
implements the *named interface* — "a platform object that implements the
[Document](https://dom.spec.whatwg.org/#interface-document) interface" —
and to a `downcast_ref` check as checking the platform object's
[inherited interfaces](https://webidl.spec.whatwg.org/#dfn-inherited-interfaces).

Platform object types are `#[gc_struct]` Rust structs; the macro derives the
active backend's GC traits.  The engine stores the struct on its managed
heap and links it to the JS wrapper; domain code reaches the struct through
the generic `with_object_any` / `with_object_any_mut` /
`with_object_any_mut_with` accessors, never through backend-specific
wrapper types (comment in backend-neutral terms — wrapper mechanics are
documented in `js_engine/src/gc.rs`).

The typical pattern for a platform object:

```rust
#[gc_struct]
pub struct MyInterface {
    /// Rust backing state — not JS-visible properties.
    pub inner: GcCell<InnerState>,
}
```

The JS-visible properties and methods are registered separately via the Web
IDL bindings (`WebIdlInterface`); the Rust struct holds only the backing state.

### Exotic objects and custom internal methods

Some Web/HTML spec objects (e.g. `WindowProxy`, `Location`) require exotic
internal methods — they override `[[Get]]`, `[[Set]]`, `[[GetPrototypeOf]]`,
etc. rather than using the ordinary object behaviour.

The generic `ExecutionContext::create_proxy(target, handler)` builds these
as proxies on every backend — content only ever calls the generic trait
method.  See `content/src/html/windowproxy.rs` for the WindowProxy pattern.
On the **Boa backend** the proxy is created through the `%Proxy%`
constructor (`JsProxyBuilder`, which supplies each trap as a plain
`NativeFunctionPointer`); Boa also exposes exotic objects through
`InternalObjectMethods` (a vtable stored on every `JsObject`):

1. Define a Rust type implementing `JsData` by deriving `#[derive(Trace, Finalize)]`
   and implementing `JsData` manually.
2. Override `JsData::internal_methods()` to return a `static InternalObjectMethods`
   with the custom function pointers:

```rust
#[derive(Trace, Finalize)]
pub struct MyExotic { ... }

impl JsData for MyExotic {
    fn internal_methods(&self) -> &'static InternalObjectMethods {
        static METHODS: InternalObjectMethods = InternalObjectMethods {
            __get__: my_exotic_get,
            __set__: my_exotic_set,
            __delete__: my_exotic_delete,
            ..ORDINARY_INTERNAL_METHODS
        };
        &METHODS
    }
}
```

3. Inside each function, use `obj.downcast_ref::<MyExotic>()` to access the data.
4. Delegate to the inner object using the **public** `JsObject` methods
   (`get()`, `set()`, `prototype()`, `own_property_keys()`, etc.).
   See `content/src/js/README.md` for the full methodology.

**Notes:** `#[derive(JsData)]` cannot be used when manually overriding
`internal_methods()` — the derive macro generates a conflicting
implementation; use `#[derive(Trace, Finalize)]` and implement `JsData` by
hand.  Do not modify the external engine dependency to make internal APIs
public — use only what the engine already exposes publicly (see
`content/src/js/README.md`, "Working with the engine's public API").

For Boa platform objects that need only a prototype chain (no exotic
behaviour), the pattern is `ObjectInitializer::with_native_data_and_proto(...)`;
see `content/src/js/bindings/` for concrete examples per interface.

## Buffer source types

<https://webidl.spec.whatwg.org/#js-buffer-source-types>

`buffer_source.rs` implements the buffer source conversion algorithms:

| Function | Spec algorithm |
|---|---|
| `get_a_copy_of_the_buffer_source` | [#dfn-get-buffer-source-copy](https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy) — extract bytes from an `ArrayBuffer` or typed array |
| `is_buffer_source` | [#dfn-buffer-source-type](https://webidl.spec.whatwg.org/#dfn-buffer-source-type) |

`get_a_copy_of_the_buffer_source` is called by the bindings layer (e.g.
`content/src/js/bindings/wasm/mod.rs`) to convert JS values into Rust
`Vec<u8>` before passing them to domain functions.  `SharedArrayBuffer`
values do not match `object_as_array_buffer` on the generic `JsTypes`, so
buffer sources reject them (the `[AllowShared]` constraint).

## Related documentation

- `content/README.md` — Content-crate overview
- `content/src/js/README.md` — JS integration specifics (engine context ownership, bindings)
- `content/src/js/bindings/README.md` — the three-layer architecture and the spec-annotation rules (definitive)
- `content/src/html/README.md` — HTML platform objects, WindowProxy, navigation split
