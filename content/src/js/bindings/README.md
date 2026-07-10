# content/src/js/bindings — JS bindings glue layer

Every Web-exposed feature (DOM, HTML, Streams, WebAssembly) follows the same
three-layer split.  This directory is the **outermost layer** — the JS bindings
glue that bridges domain types to the JavaScript engine.

## Three-layer architecture

From inside out:

| Layer | Location | What it contains | Signature convention |
|---|---|---|---|
| **Domain** | `content/src/<domain>/` | Rust struct + spec-algorithm methods and functions. Domain code implements the algorithm. Receives `&mut dyn ExecutionContext<T>` for all ECMA-262 operations. | `fn domain_method(&self) -> RustType` for pure-computation methods; `fn namespace_op(ec, arg) -> Completion<T::JsValue, T>` for promise-returning functions |
| **Web IDL bindings infra** | `content/src/webidl/bindings/` | Generic traits (`WebIdlInterface`, `WebIdlNamespace`), registration (`register_interface_spec`), and member definitions (`OperationDef`, `AttributeDef`). NOT domain-specific. `OperationDef` and `AttributeDef` are parameterized over `T: JsTypes`. | `register_interface_spec::<T>(ec)` |
| **JS bindings glue** | `content/src/js/bindings/<domain>/` | `WebIdlInterface` impl + thin function pointers that extract JS arguments, call domain functions, and wrap results. | `fn binding_fn(this, args, ec: &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>` — must be thin, no algorithm logic |

### Rules of thumb

1. **Domain methods are `impl` blocks on the domain struct.**  Never a standalone
   function that takes `&wasmtime::Module` — put it on `WasmModule` where the
   spec says it belongs.

2. **JS bindings functions downcast then delegate.**  A binding function looks like:
   ```rust
   fn my_method_binding(
       this: &T::JsValue,
       args: &[T::JsValue],
       ec: &mut dyn ExecutionContext<T>,
   ) -> Completion<T::JsValue, T> {
       let obj = T::value_as_object(this).ok_or_else(|| ec.new_type_error("..."))?;
       if let Some(data) = ec.with_object_any(&obj) {
           if let Some(domain) = data.downcast_ref::<MyDomainType>() {
               let result: RustType = domain.my_method(arg1, arg2);
               return Ok(ec.value_from_string(ec.js_string_from_str(&result)));
           }
       }
       Err(ec.new_type_error("receiver is not a MyDomainType"))
   }
   ```

3. **The Web IDL bindings infra is generic — never add domain-specific code to it.**
   Adding a new interface means implementing `WebIdlInterface` in `content/src/js/bindings/<domain>/`
   and adding domain methods on the struct in `content/src/<domain>/`.  The infra
   (`OperationDef`, `register_interface_spec`, `legacy_namespace()`) stays untouched.

4. **`WebIdlInterface` impls define *which members* an interface exposes.**
   Domain methods implement *what those members do*.

5. **Never use fully qualified paths in binding function bodies.**  Import
   domain functions with `use` at the top of the file and call them
   unqualified.  `crate::wasm::namespace::compile_fn(bytes, ctx)` is wrong;
   `compile_fn(bytes, ctx)` is right.

### Spec step annotation rules

Only the **domain method** (the one that implements the spec algorithm) gets
spec annotations.  The JS binding function that calls it does **not** — it
is plumbing, not an algorithm implementation.

**Domain method — annotated:**

```rust
/// <https://webassembly.github.io/spec/js-api/#dom-module-exports>
//                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
//                 Only the anchor URL.  No description, no step summary.
pub(crate) fn export_descriptors(&self) -> Vec<(String, &'static str)> {
    // Step 3: "For each (name, type) of module_exports(module),"
    //         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //         Verbatim from the spec.  Every step gets `// Step N:`.
    //         Step numbering must match the spec exactly.
    for export in self.module.exports() {
        let kind = match export.ty() {
            wasmtime::ExternType::Func(_) => "function",
            // Step 3.1: "Let kind be the string value of the extern type type."
            // Step numbers like 3.1 match the spec's nested numbering.
            ...
        };
    }
    // Step 4: "Return exports."
    // Note: ... (only for discrepancies)
}
```

**JS binding function — NO annotations:**

```rust
fn module_exports_binding<T: JsTypes>(
    _this: &T::JsValue,
    args: &[T::JsValue],
    ec: &mut dyn ExecutionContext<T>,
) -> Completion<T::JsValue, T> {
    // No doc comment, no step comments.  This is plumbing:
    // downcast -> call domain method -> wrap result.
    let wasm_module = downcast_arg::<WasmModule>(args)?;
    let descriptors = wasm_module.export_descriptors();
    Ok(build_js_array(ec, descriptors))
}
```

**Common mistakes:**

| ❌ Wrong | ✅ Right |
|---|---|
| Step summary in doc comment (`/// Steps 1-3: ...`) | Doc comment is **only** the anchor URL |
| Abbreviated step (`// Step 2-3: Iterate and collect`) | Verbatim spec text (`// Step 3: "For each (name, type) of module_exports(module),"`) |
| No `// Step N:` at all | Every spec-algorithm step has a `// Step N:` line before the corresponding code |
| Combined steps (`// Steps 7-8: Set this.[[Module]]`) | Only when adjacent steps are implemented in the same code block — each still gets its own `// Step N:` line |
| Inlining a named sub-algorithm into the parent function | When the spec calls a named sub-algorithm (e.g. "initialize an instance object", "create an exports object"), create a separate function with its own `/// <url>` anchor and `// Step N:` comments — mirror the spec's structure |
| Steps in a doc comment above the function | Steps go inside the function body — `//` not `///` |
| **Step comments on a JS binding function** | **Binding functions don't implement algorithms — no step comments** |

### What NOT to do

| Mistake | Why it's wrong |
|---|---|
| Putting `WebIdlInterface` impls or `WebIdlNamespace` impls in the domain layer | Those register members (which members exist) — domain code implements *what members do* |
| Putting spec-algorithm logic (iterating wasm exports, computing descriptors, creating promises) in the JS bindings glue | The binding should be a thin call → wrap; the algorithm lives in the domain layer |
| Using `FunctionObjectBuilder` or Boa-native APIs directly in JS bindings | Use `WebIdlInterface`, `OperationDef`, `register_interface_spec` from `content/src/webidl/bindings/` instead |
| Adding domain-specific conditionals to `content/src/webidl/bindings/` | The infra must stay generic; use the trait methods (`legacy_namespace()`, `constructor_length()`) to customize |
| **Manually installing a namespace via `create_plain_object` + `create_builtin_fn`** | Use `WebIdlNamespace` + `register_namespace_spec` from `content/src/webidl/bindings/` instead.  The Web IDL infra handles namespace-object creation, member installation, and global registration automatically.  See `content/src/js/bindings/wasm/mod.rs` for a correct example, or `content/src/js/bindings/testutils/mod.rs`. |

### Concrete example: WebAssembly.namespace operations (promise-returning)

Namespace operations like `WebAssembly.compile()` follow a slightly different
pattern because they return promises.  The key principle is the same: the
bindings convert JS arguments to Rust types (via `content/src/webidl/` helpers),
then call the domain function which does the rest.

```
Spec says:  compile(bytes)
              → Let stableBytes be a copy of the bytes held by the buffer bytes.
              → Asynchronously compile a WebAssembly module from stableBytes
                using options and return the result.

Bindings (arg extraction + webidl conversion):
  content/src/js/bindings/wasm/mod.rs
  fn compile_fn(this, args, ec: &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T> {
      let val = args.first()?;
      let bytes: Vec<u8> = get_stable_bytes(val, ec)?;   // webidl helper
      crate::wasm::namespace::compile_fn(bytes, ec)       // domain call
  }

Domain (algorithm):
  content/src/wasm/namespace.rs
  fn compile_fn(bytes: Vec<u8>, ec: &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T> {
      let (promise, resolvers) = a_new_promise(ec); // webidl helper
      // ... push pending request, store resolvers ...
      Ok(T::Types::value_from_object(promise))
  }
```

The domain function receives `Vec<u8>` (not `&JsValue`) because the JsValue→Rust
type conversion is the bindings' job.  `get_stable_bytes` from `content/src/webidl/`
implements the Web IDL "get a copy of the buffer source" algorithm.

### Concrete example: WebAssembly.Module.exports

```
Spec says:  Module.exports(moduleObject)
            → Let module be moduleObject.[[Module]]
            → For each (name, type) of module_exports(module), ...

Domain                           content/src/wasm/functions.rs
  impl WasmModule {
      fn export_descriptors(&self) -> Vec<(String, &'static str)>
  }

JS bindings glue                 content/src/js/bindings/wasm/interfaces.rs
  impl WebIdlInterface<crate::js::Types> for WasmModule {
      fn define_members(def) { def.add_operation(OperationDef { method: binding_fn, … }) }
  }
  fn binding_fn(this, args, ec: &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T> {
      // 1. downcast this to WasmModule
      // 2. call domain.export_descriptors()
      // 3. wrap Vec in JsArray
  }
```

## Related documentation

- `AGENTS.md` — Algorithm Implementation (step comments, anchor URLs, Note conventions)
- `content/README.md` — Content-crate overview
- `content/src/webidl/README.md` — Web IDL bindings infrastructure, platform object pattern
- `content/src/<domain>/README.md` — Domain-specific README (e.g. `content/src/wasm/README.md`)
