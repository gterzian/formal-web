# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Migration status — builtin function creation unified

All builtin function creation now goes through a single method on
`ExecutionContext<T>`:

- **`create_builtin_function(behaviour, length, name, is_constructor)`** —
  the one canonical method.
- **`create_builtin_fn(behaviour, length, name)`** — convenience default
  method delegating with `is_constructor: false`.

Removed: `Behaviour` trait, `create_builtin_function_from_behaviour`,
`create_constructor`, and the unsafe `NativeFunction::from_closure` path.
The Boa backend now uses only `NativeFunction::from_copy_closure_with_captures`.

## Remaining issues

### 1. 🟡 Direct `downcast_ref<T>()` on `create_interface_instance` objects

`create_interface_instance` stores data as `TraceableBox(T)` inside a
`Box<dyn Any>`.  Boa's native `JsObject::downcast_ref::<T>()` can't see
through the wrapper.  Must use
`ec.with_object_any(&obj).and_then(|d| d.downcast_ref::<T>())`.

**Fixed:**
- `content/src/dom/ui_event_dispatch.rs` — `has_activation_behavior`,
  `run_activation_behavior`, `apply_to_event_state`.
- `content/src/js/bindings/wasm/mod.rs` — `instantiate_fn` now uses
  `ec.with_object_any(&module_object)` instead of direct `downcast_ref::<WasmModule>()`.

**Not a bug — handled by comment:** `content/src/html/location.rs` stores the
Window as a raw `JsObject` field (not via `create_interface_instance`), so
direct `downcast_ref` works correctly.  The code has a TODO comment noting
that if the storage strategy changes, it should switch to
`ec.with_object_any(&self.window)` and thread `ec` through the navigation
call chain.

### 2. ✅ Attribute accessor descriptors — VERIFIED WORKING

`define_regular_attributes` correctly creates accessor descriptors on
prototypes.  Verified through:
- `test_button_inherits_widget_accessors_via_prototype_chain` —
  `ExecutionContext::get`/`set` through prototype chain
- `attribute_accessor_descriptors_accessible_via_js_eval` — full JS
  evaluation (`new TestWidget().title`, setter via `w.title = 'Hello'`,
  `'title' in TestWidget.prototype`)

`Object.getOwnPropertyDescriptor` and `Object.getOwnPropertyNames` fail
on prototype objects created via `create_object_with_any`.  This is a
Boa exotic-object limitation, not an accessor registration bug — the
accessor descriptors themselves work correctly through `[[Get]]` and
`[[Set]]`.

### 3. ✅ `create_builtin_function` produces constructable functions (verified)

`create_builtin_function(behaviour, length, name, true)` correctly creates
constructable functions on the Boa backend.  All 91 unit tests pass,
including `register_interface_spec` (which creates interface constructors),
`construct_calls_constructor`, and `create_builtin_function_and_call`.

The `FunctionObjectBuilder::constructor(true)` + `from_copy_closure_with_captures`
path sets `NativeFunctionObject.constructor = Some(ConstructorKind::Base)`,
which causes `NativeFunctionObject::internal_methods()` to return the
`&CONSTRUCTOR` vtable (including `native_function_construct`).

### 4. ✅ `perform_promise_then` result_capability piping — FIXED (2026-07-06)

The `BoaContext::perform_promise_then` trait implementation was ignoring
the `result_capability` parameter.  Callers (stream code, async iterators)
create a `PromiseCapability` and pass it to `perform_promise_then`, expecting
the capability's promise to resolve/reject after the handler fires.

**Root cause:** The implementation called `promise.then(on_fulfilled,
on_rejected, &mut self.context)` which creates its own internal capability
(inside `Promise::inner_then` → `PerformPromiseThen`).  The caller's
capability was completely ignored, leaving `capability.promise` pending
forever.  This caused timeouts in callers that depend on the capability.

**Fix:** When `result_capability` is provided, chain a second `.then()`
on the result promise to pipe the handler result through the capability's
resolve/reject functions.  Uses `NativeFunction::from_copy_closure_with_captures`
with properly traced captures (`PromiseThenResolve`/`PromiseThenReject`)
to avoid GC issues.

**Verified by unit test:** `perform_promise_then_with_result_capability`
confirms that both the handler fires AND the capability's promise resolves.

### 5. ✅ GC-traceable builtin function captures — FIXED (2026-07-07)

`create_builtin_function` was storing the behaviour closure in
`GcBox<Box<dyn Fn(...)>>` with a **no-op `Trace`** impl.  Any `JsObject`,
`GcCell`, or other GC-managed value captured inside the closure was invisible
to Boa's garbage collector, causing "not a callable function" errors.

**Fix:** Added `create_builtin_fn_with_captures` in
`js_engine/src/boa/engine.rs` — a standalone function that stores the
captures as a concrete traceable type `C: boa_gc::Trace + 'static`
directly in `NativeFunction::from_copy_closure_with_captures`, preserving
proper GC reachability.  Added helper function
`crate::js::create_builtin_fn_with_traced_captures` in content crate
with `#[cfg]`-based backend dispatch.

All stream domain closures (`ReadableStreamDefaultController`,
`ReadableByteStreamController`, `WritableStreamDefaultController`,
`ReadableStream`, `TransformStream`, `ReadableStreamFromIterableState`,
`AbortThenCancelState`, `TeeState`, `ByteTeeState`, etc.) and
`webidl/async_iterable.rs` closures converted to use
`create_builtin_fn_with_traced_captures`.

### 6. ✅ Wasm branding tests — FIXED (2026-07-08)

`module_exports_binding` and `get_instance_exports_binding` now use
`ec.with_object_any()` instead of direct `JsObject::downcast_ref`,
matching the `TraceableBox` storage strategy for platform objects.
Also fixed `rejected_promise_from_error_boa` to produce a proper
error message when the opaque error value is unavailable.

### 7. 🟡 WPT stream test failures

13 test files still produce unexpected results.  The dominant error
pattern is `TypeError: not a callable function` as unhandled promise
rejections, affecting all readable-stream tests that involve reading,
canceling, teeing, or async-iterating.

**Not caused by:**
- Basic promise resolution (unit tests confirm `.then()` works)
- Attribute accessor descriptors (verified working through JS eval)
- GC tracing of closure captures (all stream closures use
  `create_builtin_fn_with_traced_captures`)
- `ec.call()` in `chunk_steps` (diagnostic check confirms the
  resolver function is callable when checked)

The error comes from Boa's VM (`non_existent_call` in
`internal_methods.rs`) indicating JavaScript code tries to call a
value that has no `[[Call]]` internal method.  Likely an opaque
`JsFunction` handle inside a `PromiseJob` closure that was collected
or became invalid between when `.then()` was called and when the
microtask runs.

Piping, transform-stream, and writable-stream tests pass — these use
`ReadableStreamPipeTo`/`TransformStreamDefaultSourcePull` read request
variants that avoid calling `resolvers.resolve` through `ec.call()`.

### 8. 🟡 WASM compile/instantiate in worker context

`window_from_context` fails in worker/`dedicatedworker` contexts
because the global object is not a `Window`.  The WASM namespace
operations (`WebAssembly.compile`, `WebAssembly.instantiate`) use
IPC-based worker dispatch that requires a Window.  Affects:
- `formal/wasm-compile-instantiate.html`
- `wasm/jsapi/constructor/compile.any.js` subtests

### 9. 🟡 JSC backend not functional

JSC compiles and launches but `addEventListener` is missing, the content
process loops at 100% CPU, and WPT tests time out.  Pre-existing
condition; full JSC integration deferred.

### 10. 🔍 Audit remaining direct `downcast_ref` calls

Find and convert all remaining `JsObject::downcast_ref::<T>()` calls that
bypass `ec.with_object_any()`.  Many files in `content/src/` still use
direct `downcast_ref`:
- `content/src/dom/dispatch.rs` — Window, Document, HTMLAnchorElement, etc.
- `content/src/js/downcast.rs` — multi-type downcast helper
- `content/src/js/bindings/dom/abort_signal.rs` — Window, AbortSignal
- `content/src/html/environment_settings_object.rs` — Window
- `content/src/js/platform_objects.rs` — Window
- `content/src/html/windowproxy.rs` — Window
- `content/src/streams/` — various stream types
- `content/src/webidl/async_iterable.rs` — DefaultAsyncIterator
- `content/src/webidl/bindings/registry.rs` — InterfaceRegistry

Some of these may use `create_object_with_any` (wrapping in `TraceableBox`),
some may use `create_platform_object` (which keeps the concrete type).  Each
call site needs individual verification.

### 11. Restore JSC backend — Wire `addEventListener`/DOM event
    infrastructure on JSC; fix the content-process infinite loop.

## Tasks for migration completion

1. ✅ **`create_builtin_function` produces constructable functions** —
   Verified.  All 91 unit tests pass.

2. ✅ **`perform_promise_then` pipes result_capability** — FIXED.
   The capability promise now correctly resolves after the handler fires.

3. ✅ **`create_builtin_fn_with_captures` added and stream closures converted** —
   All stream and async-iterator closures use traceable captures.

4. ✅ **Wasm branding tests** — FIXED.
   `module_exports_binding` and `get_instance_exports_binding` now use
   `ec.with_object_any()` to access `WasmModule`/`WasmInstance` data.

5. ✅ **Attribute accessor descriptors** — VERIFIED WORKING.
   `define_regular_attributes` correctly creates accessor descriptors on
   prototypes; both `[[Get]]` and `[[Set]]` work through JS evaluation.
   `Object.getOwnPropertyDescriptor`/`Object.getOwnPropertyNames` fail on
   `create_object_with_any`-created prototype objects (Boa exotic-object
   limitation), but this does not affect property access.

6. 🟡 **WPT stream test failures** — `TypeError: not a callable function`
   in all readable-stream reading/canceling/teeing/async-iterator tests.
   Likely a Boa runtime issue with `JsFunction` handles inside opaque
   `PromiseJob` closures that are invisible to the GC.

7. 🟡 **WASM worker-context tests** — `WebAssembly.compile` and
   `WebAssembly.instantiate` require a `Window` global object.

8. 🔍 **Audit remaining `downcast_ref` calls** — Find and convert
   all remaining direct `JsObject::downcast_ref::<T>()` calls that bypass
   `ec.with_object_any()`.

9. 🟡 **JSC backend** — Wire `addEventListener`/DOM event infrastructure
   on JSC; fix the content-process infinite loop.

10. **Prune historical notes** — Remove Category 1-8 fix attempts, GC
    tracing investigations, and per-test WPT inventories from this
    document (completed).
