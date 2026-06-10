# content/src/js/bindings — JS bindings glue layer

Every Web-exposed feature (DOM, HTML, Streams, WebAssembly) follows the same
three-layer split.  This directory is the **outermost layer** — the JS bindings
glue that bridges domain types to the JavaScript engine.

## Three-layer architecture

From inside out:

| Layer | Location | What it contains | Signature convention |
|---|---|---|---|
| **Domain** | `content/src/<domain>/` | Rust struct + spec-algorithm methods. Domain code never imports `boa_engine` or returns `JsValue`/`JsObject`. | methods return Rust types: `fn export_descriptors(&self) -> Vec<(String, &str)>` |
| **Web IDL bindings infra** | `content/src/webidl/bindings/` | Generic traits (`WebIdlInterface`, `WebIdlNamespace`), registration (`register_interface_spec`), and member definitions (`OperationDef`, `AttributeDef`). NOT domain-specific. | `register_interface_spec::<T>(context)` |
| **JS bindings glue** | `content/src/js/bindings/<domain>/` | `WebIdlInterface` impl + thin function pointers that downcast `this`, call domain methods, wrap results in `JsValue`. | `fn binding_fn(this, args, ctx) -> JsResult<JsValue>` |

### Rules of thumb

1. **Domain methods are `impl` blocks on the domain struct.**  Never a standalone
   function that takes `&wasmtime::Module` — put it on `WasmModule` where the
   spec says it belongs.

2. **JS bindings functions downcast then delegate.**  A binding function looks like:
   ```rust
   fn my_method_binding(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
       let obj = this.as_object().ok_or_else(|| /* TypeError */)?;
       let domain = obj.downcast_ref::<MyDomainType>().ok_or_else(|| /* TypeError */)?;
       let result: RustType = domain.my_method(arg1, arg2);
       Ok(JsValue::from(result))
   }
   ```

3. **The Web IDL bindings infra is generic — never add domain-specific code to it.**
   Adding a new interface means implementing `WebIdlInterface` in `content/src/js/bindings/<domain>/`
   and adding domain methods on the struct in `content/src/<domain>/`.  The infra
   (`OperationDef`, `register_interface_spec`, `legacy_namespace()`) stays untouched.

4. **`WebIdlInterface` impls define *which members* an interface exposes.**
   Domain methods implement *what those members do*.

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
fn module_exports_binding(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // No doc comment, no step comments.  This is plumbing:
    // downcast -> call domain method -> wrap result.
    let wasm_module = downcast_arg::<WasmModule>(args)?;
    let descriptors = wasm_module.export_descriptors();
    Ok(build_js_array(context, descriptors))
}
```

**Common mistakes:**

| ❌ Wrong | ✅ Right |
|---|---|
| Step summary in doc comment (`/// Steps 1-3: ...`) | Doc comment is **only** the anchor URL |
| Abbreviated step (`// Step 2-3: Iterate and collect`) | Verbatim spec text (`// Step 3: "For each (name, type) of module_exports(module),"`) |
| No `// Step N:` at all | Every spec-algorithm step has a `// Step N:` line before the corresponding code |
| Combined steps (`// Steps 7-8: Set this.[[Module]]`) | Only when adjacent steps are implemented in the same code block — each still gets its own `// Step N:` line |
| Steps in a doc comment above the function | Steps go inside the function body — `//` not `///` |
| **Step comments on a JS binding function** | **Binding functions don't implement algorithms — no step comments** |

### What NOT to do

| Mistake | Why it's wrong |
|---|---|
| Putting `JsObject::downcast_ref` or `JsValue`-returning code in the domain layer | Domain code should be pure Rust logic, testable without a JS engine |
| Putting spec-algorithm logic (iterating wasm exports, computing descriptors) in the JS bindings glue | The binding should be a thin call → wrap; the algorithm lives on the domain struct |
| Using `FunctionObjectBuilder` or Boa-native APIs directly in JS bindings | Use `WebIdlInterface`, `OperationDef`, `register_interface_spec` from `content/src/webidl/bindings/` instead |
| Adding domain-specific conditionals to `content/src/webidl/bindings/` | The infra must stay generic; use the trait methods (`legacy_namespace()`, `constructor_length()`) to customize |

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
  impl WebIdlInterface for WasmModule {
      fn define_members(def) { def.add_operation(OperationDef { method: binding_fn, … }) }
  }
  fn binding_fn(this, args, ctx) -> JsResult<JsValue> {
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
