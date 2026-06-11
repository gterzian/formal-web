# content/src/wasm — WebAssembly JS API

Implements the [`WebAssembly`](https://www.w3.org/TR/wasm-js-api/) namespace
exposed to web content.  Uses the vendored `wasmtime` crate
(`vendor/wasmtime/`) as the underlying WebAssembly engine.

## Architecture

### Module layout

- `mod.rs` — crate-level re-exports.
- `types.rs` — Rust data types for JS-visible wasm objects (`WasmModule`,
  `WasmInstance`, etc.) with `JsData` implementations.
- `worker.rs` — `WasmWorker` (background compilation worker management),
  `WasmRequest`, `WasmResult`.
- `conversions.rs` — JS↔wasm value conversion (`js_val_to_wasm_val`,
  `wasm_val_to_js_value`, `default_val_for_type`), implementing the
  [Core Embedding](https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type)
  value-type conversion algorithms.
- `namespace.rs` — Spec-mapped implementations for the `WebAssembly`
  namespace and its algorithms: `validate`, `compile`, `instantiate`
  (bytes + module overloads), `instantiate the core`, `initialize an
  instance object`, and `create an exports object`.  These functions
  receive already-converted Rust types (`Vec<u8>` for buffer sources,
  `&WasmModule` for module objects) — the JsValue→Rust conversion
  happens in the bindings layer via `content/src/webidl/` helpers.
  They orchestrate the async flow: create promises via
  `crate::webidl::a_new_promise`, push pending requests onto
  `GlobalScope`, and return the JS promise.

### Domain vs binding separation

The **domain layer** (`content/src/wasm/`) implements the spec algorithms.
It may import Boa types (`Context`, `JsValue`) when the algorithm
requires it (e.g., creating promises).  The **bindings layer**
(`content/src/js/bindings/wasm/`) is the thin outermost wrapper — it
extracts JS arguments, calls the domain function, and transforms the
return value into `JsResult<JsValue>`.

| Layer | Location | Responsibility |
|---|---|---|
| **Conversions** | `content/src/wasm/conversions.rs` | JS↔wasm value conversion per Core Embedding spec |
| **Namespace operations** | `content/src/wasm/namespace.rs` | Spec-mapped `compile`, `instantiate` (bytes + module overloads) — promise creation, pending-request push, result-wrapping |
| **Types** | `content/src/wasm/types.rs` | Rust data types with `JsData` (`WasmModule`, `WasmInstance`, etc.) |
| **Worker** | `content/src/wasm/worker.rs` | Background compilation worker management |
| **Bindings** | `content/src/js/bindings/wasm/interfaces.rs` | `WebIdlInterface` impls, promise resolution/rejection, exports-object creation, prototype lookups, error-type registration |
| **Bindings** | `content/src/js/bindings/wasm/mod.rs` | `WasmNamespace` impl + thin binding functions — arg extraction → domain call → result wrap |

This split keeps spec-mapped algorithm code in the domain layer while
keeping the binding functions ignorant of the algorithm details.  A
binding function should consist of little more than:

```rust
fn binding_fn(_this, args, context) -> JsResult<JsValue> {
    let arg = args.first().ok_or_else(|| /* TypeError */)?;
    domain_fn(arg, context)  // returns JsResult<JsValue>
}
```

**Do not put `WebIdlInterface` implementations, `JsObject` construction,
or `WebIdlNamespace` impls in `content/src/wasm/`.**  Those belong in
`content/src/js/bindings/wasm/`.

### JS bindings (Web IDL → JavaScript engine)

The WebAssembly API's JS-facing registration (namespace, type constructors,
operations) lives under the common bindings directory:

**`content/src/js/bindings/wasm/`**

This follows the project convention: all Web IDL bindings — whether for DOM,
HTML, Streams, or WebAssembly — go in `content/src/js/bindings/` and use the
Web IDL bindings infrastructure (`register_namespace_spec`, `WebIdlNamespace`,
`WebIdlInterface`, etc.) instead of calling into Boa directly.

- The `WasmNamespace` marker type implements `WebIdlNamespace`, registering
  operations (`validate`, `compile`, `instantiate`) and the `JSTag` attribute
  via `register_namespace_spec`.
- Error types (`CompileError`, `LinkError`, `RuntimeError`) and the
  `[LegacyNamespace=WebAssembly]` interfaces (`Module`, `Instance`) use
  the `WebIdlInterface` trait with `legacy_namespace()` and are registered
  via `register_interface_spec` in `content/src/js/bindings/wasm/interfaces.rs`.

**All JS-interop code goes in `content/src/js/bindings/wasm/`, not in
`content/src/wasm/`.**  Domain code in `content/src/wasm/` returns Rust
values (`bool`, `wasmtime::Module`, `WasmModule`).  Bindings code in
`content/src/js/bindings/wasm/` wraps those in `JsValue`/`JsObject`, resolves
promises, and implements `WebIdlInterface`.

## Current Status

### Working

- **`WebAssembly` namespace** installed on the global object with `validate`,
  `compile`, and `instantiate` (bytes + module-object overloads).
- **`WebAssembly.validate(bytes)`** — synchronous compilation check via
  `wasmtime::Module::new`.  Returns `true`/`false`.
- **`WebAssembly.compile(bytes)`** — async compilation (see flow diagram
  below).  The domain function in `content/src/wasm/namespace.rs` creates
  the promise, pushes a `PendingRequest` onto the document's `GlobalScope`,
  and returns the promise.  The bindings layer only extracts the JS argument.
- **`WebAssembly.instantiate(moduleObject)`** — async instantiation of a
  previously-compiled module (empty imports).
- **`WebAssembly.instantiate(bytes)`** — bytes overload: compiles then
  instantiates.
- **`WebAssembly.Module`** — constructor that compiles synchronously.
  Static method `exports(moduleObject)` returns an array of export
  descriptors `{ name, kind }`.
- **Error types** — `CompileError`, `LinkError`, `RuntimeError` registered
  as subclasses of `Error` on the namespace.
- **Background compilation worker** — lazily started on first compile
  request.  Uses `crossbeam_channel::unbounded()` for request/result
  message passing between the content-process main thread and the
  compiler worker.
- **PendingRequest infrastructure** — `PendingRequest::WasmCompile` and
  `PendingRequest::WasmInstantiate` on `GlobalScope`, with a `PendingState`
  lifecycle (`Pending → Processing → removed on completion`).  JS-typed
  data (promise, resolvers) is stored separately in
  `GlobalScope.pending_wasm_resolvers` so that domain code can construct
  `PendingRequest` without importing `boa_engine`.
- **WPT test**: `wasm/jsapi/constructor/compile.any.js` enabled in
  `tests/wpt/include.ini`.

### Scaffolded but not wired

The following Rust data types are defined in `types.rs` with `JsData`
implementations but have no JS-visible constructors or methods yet:

- `WasmInstance` — for `WebAssembly.Instance`
- `WasmMemory` — for `WebAssembly.Memory`
- `WasmTable` — for `WebAssembly.Table`
- `WasmGlobal` — for `WebAssembly.Global`
- `WasmTag` — for `WebAssembly.Tag`

### Not yet implemented

- **`WebAssembly.instantiate(moduleObject, importObject)`** — the
  module-object overload that instantiates a compiled module with
  imports.
- **`WebAssembly.instantiate(bytes, importObject)`** — the bytes
  overload currently follows the same compile-only path as `compile()`.
  Full instantiation requires import resolution, host function wrapping,
  and exports object construction.
- **`WebAssembly.Module.imports(moduleObject)`** — returns import
  descriptors.
- **`WebAssembly.Module.customSections(moduleObject, sectionName)`** —
  returns custom-section ArrayBuffers.
- **`WebAssembly.Instance`** — the `exports` readonly attribute and
  constructor.
- **`WebAssembly.Memory`** — constructor, `buffer` getter (needs
  ["identified with"](https://www.w3.org/TR/wasm-js-api/#identified-with)
  `ArrayBuffer` DataBlock binding), `grow` method.
- **`WebAssembly.Table`** — constructor, `get`/`set`/`grow`/`length`.
- **`WebAssembly.Global`** — constructor, `value` getter/setter,
  `valueOf`.
- **`WebAssembly.Tag`** — constructor (exception tag).
- **Exported Functions** — calling wasm functions from JS via
  `WebAssembly.Instance.exports`.
- **Host Functions** — providing JS functions as wasm imports.
- **`WebAssembly` JSTag** — the `JSTag` readonly attribute.

### Async compile flow

```
JS: WebAssembly.compile(buffer)
  │
  ├─ [bindings] compile_fn() extracts bytes value from args,
  │   converts JsValue → Vec<u8> via get_stable_bytes() (webidl)
  │
  ├─ [domain] namespace::compile_fn(stable_bytes, context):
  │   ├─ a_new_promise()  (via crate::webidl)
  │   ├─ store resolvers in GlobalScope.pending_wasm_resolvers
  │   ├─ push PendingRequest::WasmCompile { bytes, request_id }
  │   │  onto GlobalScope.pending_requests
  │   └─ return promise JsValue
  └─ returns promise to JS

ContentProcess (before/after each command via handle_command):
  │
  ├─ drain_all_pending_wasm_requests()
  │   └─ iterates documents → take_pending_wasm_batches()
  │       → submits (request_id, bytes) to WasmWorker
  │       → stores document_id in pending_wasm_requests map
  │
  └─ drain_wasm_results()
      └─ tries recv() on WasmWorker result channel
          → consume_wasm_request() looks up resolvers separately
          → compile_continuation() or compile_rejection() for Compiled/CompileError
          → instantiate_continuation() or compile_rejection() for Instantiated/InstantiateError
          → flushes microtasks via perform_a_microtask_checkpoint()

Background worker (WasmWorker):
  │
  ├─ receives WasmRequest::Compile { request_id, bytes }
  ├─ compiles with wasmtime::Module::new(&engine, &bytes)
  └─ sends back WasmResult::Compiled { request_id, module }
     or WasmResult::CompileError { request_id, message }
```

## Dependencies

- `wasmtime` crate (vendored) — core WebAssembly compilation.
- `crossbeam-channel` — message passing between main thread and
  background compilation worker.
