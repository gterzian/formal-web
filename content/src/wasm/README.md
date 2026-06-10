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

Spec algorithms and interface implementations live **in this directory**
(`content/src/wasm/`), not in the bindings layer.  The bindings file at
`content/src/js/bindings/wasm/mod.rs` is intentionally thin:

| Layer | Location | Responsibility |
|---|---|---|
| **Domain** | `content/src/wasm/` | Implements spec algorithms (validate, compile), interface methods (Module constructor, exports), error type registration, buffer source conversion, promise resolution |
| **Binding** | `content/src/js/bindings/wasm/` | Defines *which* Web IDL members the namespace has (`WebIdlNamespace` impl), installs the namespace via `register_namespace_spec`, provides thin JS→Rust wrappers that handle `this` binding, argument extraction, and content-process state (global scope, pending request queue) |

This split follows the project convention: every spec domain
(streams, DOM, HTML, WebAssembly) keeps its interface implementations
in its own directory, while `content/src/js/bindings/` holds only the
Web IDL binding definitions that register members with the JS engine.

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
- Error types (`CompileError`, `LinkError`, `RuntimeError`) and the `Module`
  type constructor are added as post-registration steps; they will migrate to
  `WebIdlInterface` with `[LegacyNamespace=WebAssembly]` when the infra
  supports it.
- Promise resolution helpers (`resolve_compile_promise`,
  `reject_compile_promise`) live in `content/src/wasm/functions.rs` since they
  implement spec algorithm steps.  The bindings file calls them after receiving
  compilation results from the background worker.

**Do not create new JS bindings or interface implementations in
`content/src/wasm/`.**  All future Web IDL interfaces (Instance, Memory, Table,
Global, Tag, Exported Function) should follow this split: spec-algorithm
implementations in `content/src/wasm/` (new file or extended `functions.rs`),
Web IDL binding definitions in `content/src/js/bindings/wasm/`.

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
