# Finding Boa API matches via spec anchors

When implementing Web IDL or HTML algorithms in Rust with Boa, the ECMAScript
operations referenced by web specs (e.g. `ProxyCreate`, `CreateBuiltinFunction`,
`SetImmutablePrototype`) map to Boa's Rust API. The ECMAScript spec anchor
(e.g. `#sec-proxycreate`) pinpoints the implementation location.

## Methodology

1. Web IDL/HTML specs link to ECMAScript operations with URLs like
   `https://tc39.es/ecma262/#sec-proxycreate`.
2. Search for that anchor (`sec-proxycreate`) in Boa's git checkout
   (cargo downloads it to `~/.cargo/git/checkouts/boa-*/`):
   ```bash
   grep -rn "sec-proxycreate" ~/.cargo/git/checkouts/boa-*/
   ```
3. The spec anchor appears in Boa's doc comments directly:
   ```rust
   // `10.5.14 ProxyCreate ( target, handler )`
   //
   // [spec]: https://tc39.es/ecma262/#sec-proxycreate
   pub fn create(target, handler, context) { ... }
   ```
4. If the function is `pub(crate)`, check whether a **public alternative**
   exists (e.g. `JsProxyBuilder::new(target)...build(context)` instead of
   `Proxy::create`, or `JsObject::get(key, context)` instead of `__get__`).
   Boa is an external dependency — `pub(crate)` internals are not accessible
   outside the `boa_engine` crate.

When the spec anchor URL is shortened (e.g.
`https://tc39.es/ecma262/#sec-proxycreate` without the multipage path), search
for just the anchor (`sec-proxycreate`) in the Boa source under
`~/.cargo/git/checkouts/boa-*`. Boa's doc comments use both full multipage URLs
and shortened ones.

## Common ECMAScript → Boa mappings

| Spec operation | Boa API | Notes |
|---|---|---|
| `ProxyCreate(target, handler)` | `JsProxyBuilder::new(target)...build(context)` (from `boa_engine::object::builtins`) | Returns `JsResult<JsProxy>`; preferred public API |
| `CreateBuiltinFunction(steps, length, name, ...)` | `FunctionObjectBuilder::new(realm, NativeFunction::from_fn_ptr(trap)).name(name).length(length).build()` | Plain function pointers (`NativeFunctionPointer`) for `JsProxyBuilder` traps |
| `OrdinaryObjectCreate(null, slots)` | `JsObject::with_null_proto()` | Creates object with null prototype |
| `CreateDataPropertyOrThrow(O, P, V)` | `handler.create_data_property_or_throw(key, value, context)` | Method on `JsObject` |
| `SetImmutablePrototype(O, V)` | Check `window.prototype()` then `JsObject::equals` | Use the public `prototype()` method, not `__get_prototype_of__` |
| `IsCallable(target)` | `target.is_callable()` | Method on `JsObject` |
| `IsConstructor(target)` | `target.is_constructor()` | Method on `JsObject` |
| `ToPropertyDescriptor(obj)` | `obj.to_property_descriptor(context)` | Method on `JsObject` |
| `GetMethod(V, P)` | `handler.get_method(key, context)` | Currently `pub(crate)` in Boa |
