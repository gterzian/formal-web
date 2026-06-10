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
- `functions.rs` — spec-mapped implementations of all wasm namespace
  operations and interface methods (validate, compile, instantiate,
  Module constructor, error type registration, promise resolution, buffer
  source conversion).  This is the **domain layer** — it implements the
  WebAssembly spec algorithms without being concerned with how members
  are registered or wired to the JS engine.

### Domain vs binding separation

The **domain layer** (`content/src/wasm/`) operates on Rust/wasmtime types
only — no `JsValue`, no `Context`.  All JS-interop code lives in
**`content/src/js/bindings/wasm/`**:

| Layer | Location | Responsibility |
|---|---|---|
| **Domain** | `content/src/wasm/functions.rs` | Pure Rust logic: `validate_wasm_module(&[u8]) -> bool`, JS↔wasm value converters |
| **Types** | `content/src/wasm/types.rs` | Rust data types with `JsData` (`WasmModule`, `WasmInstance`, etc.) |
| **Worker** | `content/src/wasm/worker.rs` | Background compilation worker management |
| **Bindings** | `content/src/js/bindings/wasm/interfaces.rs` | `WebIdlInterface` impls, promise resolution/rejection, exports-object creation, prototype lookups, error-type registration — everything returning `JsValue`/`JsObject` |
| **Bindings** | `content/src/js/bindings/wasm/mod.rs` | `WasmNamespace` impl, `install_wasm_namespace`, namespace operation bindings (`validate_fn`, `compile_fn`, `instantiate_fn`) |

This split ensures that domain code can be reasoned about without any
JavaScript engine knowledge.  Bindings are where Rust values become
`JsValue` — as late as possible.

**Do not put `WebIdlInterface` implementations, `JsObject` construction,
`Context` usage, or any code returning `JsValue` in `content/src/wasm/`.**
Those belong in `content/src/js/bindings/wasm/`.

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
  `compile`, and `instantiate` (bytes overload) functions.
- **`WebAssembly.validate(bytes)`** — synchronous compilation check via
  `wasmtime::Module::new`.  Returns `true`/`false`.
- **`WebAssembly.compile(bytes)`** — async compilation.  Creates a pending
  promise, pushes a `PendingRequest` onto the document's `GlobalScope`,
  and returns the promise.  On the next event-loop iteration the content
  process drains the request queue, submits the bytes to the background
  compilation worker, and when the result arrives, resolves the promise
  with a `WebAssembly.Module` object (prototype-chained correctly) or
  rejects with `WebAssembly.CompileError`.
- **`WebAssembly.Module`** — constructor that compiles synchronously.
  Static method `exports(moduleObject)` returns an array of export
  descriptors `{ name, kind }`.
- **Error types** — `CompileError`, `LinkError`, `RuntimeError` registered
  as subclasses of `Error` on the namespace.
- **Background compilation worker** — lazily started on first compile
  request.  Uses `crossbeam_channel::unbounded()` for request/result
  message passing between the content-process main thread and the
  compiler worker.
- **PendingRequest infrastructure** — generic `PendingRequest` enum on
  `GlobalScope` with a `PendingState` lifecycle (`Pending → Processing →
  removed on completion`).  The content process drains requests and
  processes results before and after every command, with a microtask
  checkpoint to flush promise `.then()` handlers.
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
  ├─ compile_fn() creates JsPromise + ResolvingFunctions
  ├─ pushes PendingRequest { bytes, promise, resolvers, state: Pending }
  │  onto GlobalScope.pending_requests
  └─ returns promise to JS

ContentProcess (before/after each command via handle_command):
  │
  ├─ drain_all_pending_wasm_requests()
  │   └─ iterates documents → take_pending_wasm_batches()
  │       → submits (request_id, bytes) to WasmWorker
  │       → stores document_id in pending_wasm_requests map
  │
  └─ process_wasm_results()
      └─ try_recv() on WasmWorker result channel
          → resolve_compile_promise() or reject_compile_promise()
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
