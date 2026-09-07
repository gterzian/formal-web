# content/src/wasm — WebAssembly JS API

Implements the [`WebAssembly`](https://www.w3.org/TR/wasm-js-api/) namespace
exposed to web content.  Uses the `wasmtime` crate (crates.io) as the
underlying WebAssembly engine.  Only compiled on the Boa backend behind the
`wasm` feature (V8 and JSC implement WebAssembly natively).

## Module layout

- `types.rs` — Rust data types for JS-visible wasm objects (`WasmModule`,
  `WasmInstance`, etc.) with `JsData` implementations.
- `wasm_state.rs` — `WasmState` (GlobalScope-level wasm state),
  `ContentWasmState` (ContentProcess-level wasm state), `PendingRequest`,
  `PendingState`.
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

## Domain vs binding separation

The **domain layer** (`content/src/wasm/`) implements the spec algorithms.
The **bindings layer** (`content/src/js/bindings/wasm/`) is the thin
outermost wrapper — it extracts JS arguments, calls the domain function, and
transforms the return value into `JsResult<JsValue>`.  Domain functions
receive clean Rust types, never raw `JsValue`; the domain file may create
promises and return their JS value where the algorithm calls for it.

**Do not put `WebIdlInterface` implementations, `JsObject` construction,
or `WebIdlNamespace` impls in `content/src/wasm/`.**  Those belong in
`content/src/js/bindings/wasm/` (interfaces and error types in
`interfaces.rs`, the `WasmNamespace` and thin binding functions in
`mod.rs`), which registers them through the Web IDL infra
(`register_namespace_spec`, `register_interface_spec`, `legacy_namespace()`)
rather than calling the engine directly.

## Current status

Compile, instantiate (module-object and bytes overloads), and `validate` are
implemented and covered by `tests/formal/tests/wasm-compile-instantiate.html`
(requires the `wasm` feature; enabled by uncommenting its entry in
`tests/formal/include.ini`).  Background compilation runs on a lazily
started worker thread (`WasmWorker`), fed by `ContentProcess::handle_command`
draining pending requests between commands.

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

## Dependencies

- `wasmtime` crate (crates.io) — core WebAssembly compilation.
- `crossbeam-channel` — message passing between main thread and
  background compilation worker.
