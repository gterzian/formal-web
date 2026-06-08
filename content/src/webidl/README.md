# content/src/webidl

`content/src/webidl` stores the shared Web IDL algorithms that sit between DOM, HTML, and Streams code and the ECMAScript operations used by the current JavaScript engine.

- Callback-interface conversion, `call a user object's operation`, and promise helpers belong here.
- This layer should depend on abstract `Get`, `IsCallable`, and `Call` hooks instead of reaching into engine-specific context APIs directly.
- Keep the context-backed adapters for those hooks here so DOM, HTML, and Streams code can delegate instead of reimplementing callback glue locally.
- Promise helpers here should follow the Web IDL promise algorithms, including `#js-promise-manipulation`, `#a-promise-resolved-with`, `#a-promise-rejected-with`, and `#js-to-promise`.
- DOM event dispatch and other callback sites should call into this layer instead of calling Boa directly.
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
- **Boa-class registrations and bindings** (argument conversion, method impls): `content/src/js/bindings/`

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
4. Call the inner object's `__xxx__` methods directly (now public) to delegate
   to its ordinary internal methods.

The `content` crate uses this pattern for `WindowProxy` — see
`content/src/html/windowproxy.rs`.

**Note:** `#[derive(JsData)]` cannot be used when manually overriding
`internal_methods()` because the derive macro generates a conflicting
implementation. Use `#[derive(Trace, Finalize)]` and implement `JsData` by hand.

**Visibility note:** This required making `InternalObjectMethods`,
`InternalMethodPropertyContext`, `InternalMethodCallContext`, `CallValue`,
and `ORDINARY_INTERNAL_METHODS` publicly visible from the vendored boa engine
crate, along with all `__xxx__` dispatch methods on `JsObject`.

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

## Related documentation

- `content/README.md` — Content-crate overview
- `content/src/js/README.md` — Boa integration specifics (Context ownership, bindings)
- `content/src/html/README.md` — HTML platform objects, WindowProxy, navigation split
