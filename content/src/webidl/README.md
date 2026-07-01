# content/src/webidl

`content/src/webidl` implements the algorithms defined in Web IDL §3
(JavaScript binding): type conversion between IDL and JavaScript values,
promise manipulation ("react", "a new promise", "upon fulfillment"),
and the binding infrastructure for exposing Web IDL interfaces to JS.

**Architecture:** This layer sits between domain code (streams, HTML, DOM)
and the generic `js_engine` trait.  When a spec algorithm calls Web IDL —
for type conversion, callback invocation, or promise reaction — domain code
routes through here.  When a spec algorithm calls ECMA-262 directly (e.g.
realm creation in HTML §8.1.3.3), domain code calls `js_engine` directly,
bypassing this layer.  See `js_engine/README.md` for the full design
philosophy.

```
Web spec  →  content/src/webidl/  →  js_engine trait
(Streams,   (invoke_callback_fn,     (Get, IsCallable,
 HTML, DOM)  call_user_obj_op)        Call, ToNumber, …)
```

- Callback-interface conversion, `call a user object's operation`, and promise helpers belong here.
- This layer depends on abstract `EcmascriptHost<T>` hooks (`get`, `is_callable`, `call`) from `js_engine` — no engine-specific context APIs.
- DOM event dispatch and other callback sites call into this layer instead of calling Boa directly.
- Promise helpers follow the Web IDL promise algorithms (`#js-promise-manipulation`, `#a-promise-resolved-with`, `#a-promise-rejected-with`, `#js-to-promise`).
- Use the `web_standards` extension (`spec_lookup`) with `https://webidl.spec.whatwg.org/` to read the Web IDL spec.

## Boa integration of [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object)

The content crate defines Rust types that correspond to Web IDL interface types (e.g.
`Window`, `Document`, `HTMLAnchorElement`). In comments and documentation, refer to these
as a [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) that implements
the *named interface* — for example:
- "a [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) that implements
  the [Document](https://dom.spec.whatwg.org/#interface-document) interface"
- "the [Window](https://html.spec.whatwg.org/#window) [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object)"

The Rust `downcast_ref` operation checks which interface a `JsObject`'s backing data
implements — this maps to the Web IDL concept of
[inherited interfaces](https://webidl.spec.whatwg.org/#dfn-inherited-interfaces).
Prefer phrasing like "check the platform object's inherited interfaces" over
"downcast the platform object".

These types implement `boa_engine::JsData`
(derived via `#[derive(Trace, Finalize, JsData)]`) and are stored inside Boa `JsObject`s
via `from_proto_and_data()` or `ObjectInitializer::with_native_data_and_proto()`.

The typical pattern for a platform object:

```rust
#[derive(Trace, Finalize, JsData)]
pub struct MyInterface {
    /// Rust backing state — not JS-visible properties.
    pub inner: Rc<RefCell<InnerState>>,
}
```

The JS-visible properties and methods are registered separately via the `Class` trait
or `ObjectInitializer`. The Rust struct holds only the backing state.

### Where [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) types live

- **DOM interfaces** (`Document`, `EventTarget`, `Element`, …): `content/src/dom/`
- **HTML interfaces** (`Window`, `HTMLAnchorElement`, `HTMLIFrameElement`, `Location`, …): `content/src/html/`
- **Streams interfaces** (`ReadableStream`, `WritableStream`, …): `content/src/streams/`
- **WebAssembly domains** (`WasmModule`, compilation worker, …): `content/src/wasm/`

### Three-layer architecture

Every Web-exposed feature follows a three-layer split:

1. **Domain** (`content/src/<domain>/`) — Rust struct + spec-algorithm methods returning Rust types.
2. **Web IDL bindings infra** (`content/src/webidl/bindings/`) — generic traits
   (`WebIdlInterface`, `WebIdlNamespace`, `OperationDef`, etc.). Not domain-specific.
3. **JS bindings glue** (`content/src/js/bindings/<domain>/`) — `WebIdlInterface` impl,
   thin function pointers that downcast, call domain methods, wrap in `JsValue`.

See `content/src/js/bindings/README.md` for the definitive description.

**What belongs where:**

| What | Where |
|---|---|
| Rust struct definition (`WasmModule`), JsData derive | `content/src/<domain>/types.rs` |
| Spec-algorithm methods returning Rust types (`export_descriptors() → Vec<…>`) | `content/src/<domain>/functions.rs` — `impl WasmModule` |
| `WebIdlInterface` impl (`define_members`, `create_platform_object`) | `content/src/js/bindings/<domain>/` |
| Thin JsValue-wrapping function pointers (`fn(this, args, ctx) → JsResult<JsValue>`) | `content/src/js/bindings/<domain>/` |
| `WebIdlInterface` trait, `register_interface_spec`, `OperationDef`, `AttributeDef` | `content/src/webidl/bindings/` (generic — no domain logic) |

**Never add domain-specific code to `content/src/webidl/bindings/`.**
Use the trait methods (`legacy_namespace()`, `constructor_length()`) to
customize behaviour.  Never add an `impl WebIdlInterface` outside of
`content/src/js/bindings/`.

### Exotic objects and custom internal methods

Some Web/HTML spec objects (e.g. `WindowProxy`, `Location`) require exotic internal
methods — they override `[[Get]]`, `[[Set]]`, `[[GetPrototypeOf]]`, etc. rather than
using the ordinary object behaviour.

Boa supports exotic objects through `InternalObjectMethods` (a vtable stored on every
`JsObject`). To create an exotic object:

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

The `content` crate uses this exotic-object pattern with `JsProxyBuilder`
for `WindowProxy` — see `content/src/html/windowproxy.rs`.

**Rejected approach:** Modifying the external Boa dependency to make internal
APIs public. All exotic-object implementations must use only public Boa APIs.

**Note:** `#[derive(JsData)]` cannot be used when manually overriding
`internal_methods()` because the derive macro generates a conflicting
implementation. Use `#[derive(Trace, Finalize)]` and implement `JsData` by hand.

**Visibility note:** When implementing exotic objects, **do not modify**
the external Boa dependency to make internal APIs public. Instead, use only
what boa already exposes publicly. See `content/src/js/README.md` ("Working
with Boa's public API: use spec links, not `pub(crate)` internals") for the
correct methodology.

### The ObjectInitializer pattern

For platform objects that don't need exotic behaviour and just need a prototype chain:

```rust
let object = ObjectInitializer::with_native_data_and_proto(
    MyInterface::new(...),
    prototype,  // e.g. context.intrinsics().constructors().my_interface().prototype()
    context,
)
.property("someProp", js_string!("value"), Attribute::all())
.build();
```

See `content/src/js/bindings/` for concrete examples per interface.

## Buffer source types

<https://webidl.spec.whatwg.org/#js-buffer-source-types>

The Web IDL buffer source types (`ArrayBuffer`, `ArrayBufferView`, `BufferSource`)
have specific conversion algorithms implemented in `buffer_source.rs`.

| Function | Spec algorithm | Purpose |
|---|---|---|
| `get_a_copy_of_the_buffer_source` | [#dfn-get-buffer-source-copy](https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy) | Extract bytes from an `ArrayBuffer` or typed array |
| `convert_js_value_to_idl_array_buffer` | [#js-arraybuffer](https://webidl.spec.whatwg.org/#js-arraybuffer) | Convert a JS value to an IDL `ArrayBuffer`, rejecting `SharedArrayBuffer` |
| `is_buffer_source` | [#dfn-buffer-source-type](https://webidl.spec.whatwg.org/#dfn-buffer-source-type) | Check whether a JS value is a buffer source type |

The `get_a_copy_of_the_buffer_source` function is called by the bindings layer (e.g.
`content/src/js/bindings/wasm/mod.rs`) to convert JS values into Rust `Vec<u8>`
before passing them to domain functions.  Domain functions receive clean Rust types,
never raw `JsValue`.

Both `get_a_copy_of_the_buffer_source` and `convert_js_value_to_idl_array_buffer`
enforce that `SharedArrayBuffer` is rejected (the `[AllowShared]` constraint) and
note where `IsFixedLengthArrayBuffer` / `[AllowResizable]` checks are skipped due to
Boa's API surface.

## Related documentation

- `content/README.md` — Content-crate overview
- `content/src/js/README.md` — Boa integration specifics (Context ownership, bindings)
- `content/src/html/README.md` — HTML platform objects, WindowProxy, navigation split
