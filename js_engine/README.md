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

## Problems found

### 1. 🟡 Direct `JsObject::downcast_ref<T>()` broken for wrapped platform objects

`create_interface_instance` stores data as `NativeDataWrapper(TraceableBox(T))`
inside the JsObject.  Boa's native `downcast_ref::<T>()` can't see through
the wrapper.  Must use `ec.with_object_any(&obj).and_then(|d| d.downcast_ref::<T>())`.

**Fixed:**
- `content/src/dom/ui_event_dispatch.rs` — `has_activation_behavior`,
  `run_activation_behavior`, `apply_to_event_state`.
- `content/src/js/bindings/wasm/mod.rs` — `instantiate_fn` now uses
  `ec.with_object_any(&module_object)` instead of direct `downcast_ref::<WasmModule>()`.

**Unfixed:** `content/src/html/location.rs` (`self.window.downcast_ref::<Window>()`)
requires threading `ec` through all Location navigation methods.

### 2. 🟡 Attribute accessor descriptors not registering on prototypes

`define_regular_attributes` builds accessor descriptors via
`create_builtin_fn` + `define_property_or_throw`, but the properties
never appear on the prototype.  Operations (methods) register fine via
value descriptors.  Suspected in the `PropertyDescriptor<BoaTypes>` →
Boa native descriptor conversion for `get`+`set`-only descriptors.

### 3. ✅ `create_builtin_function` produces constructable functions (verified)

`create_builtin_function(behaviour, length, name, true)` correctly creates
constructable functions on the Boa backend.  All 90 unit tests pass,
including `register_interface_spec` (which creates interface constructors),
`construct_calls_constructor`, and `create_builtin_function_and_call`.

The `FunctionObjectBuilder::constructor(true)` + `from_copy_closure_with_captures`
path sets `NativeFunctionObject.constructor = Some(ConstructorKind::Base)`,
which causes `NativeFunctionObject::internal_methods()` to return the
`&CONSTRUCTOR` vtable (including `native_function_construct`).

### 4. ✅ `perform_promise_then` result_capability piping — FIXED (2026-07-06)

### 5. 🔴 GC-traceable builtin function captures — NEW (2026-07-07)

`create_builtin_function` stores the behaviour closure in
`GcBox<Box<dyn Fn(...)>>` with a **no-op `Trace`** impl.  Any `JsObject`,
`GcCell`, or other GC-managed value captured inside the closure is invisible
to Boa's garbage collector.  This can cause "not a callable function" errors
when the GC collects the captured objects.

**Partial fix:** Added `create_builtin_fn_with_captures` in
`js_engine/src/boa/engine.rs` — a standalone function that stores the
captures as a concrete traceable type `C: boa_gc::Trace + 'static`
directly in `NativeFunction::from_copy_closure_with_captures`, preserving
proper GC reachability.

**Not yet converted:** All callers in streams (`readablestreamdefaultcontroller.rs`,
`writablestreamdefaultcontroller.rs`, `transformstream.rs`, `readablestream.rs`,
`async_iterable.rs`) still use the closure-based `create_builtin_fn`, which
has the no-op trace issue.

### 6. 🔴 Investigate WPT stream test failures

13 readable-stream tests fail with `TypeError: not a callable function`.
Not caused by basic promise resolution (which works correctly).  Likely
related to how native functions created by `create_builtin_function` interact
with promise reaction jobs in certain edge cases.  Run isolated WPT stream
tests to capture actual error messages.

### 7. 🔴 Fix wasm branding tests

2 tests fail with `Module.exports: argument is not a WebAssembly.Module`.

**Fixed in this session:** `content/src/js/bindings/wasm/mod.rs` —
`instantiate_fn` now uses `ec.with_object_any(&module_object)` to access
the `WasmModule` data through the `TraceableBox` wrapper.

### 8. Restore JSC backend — Wire `addEventListener`/DOM event
infrastructure on JSC; fix the content-process infinite loop.

### 9. Prune historical notes — Remove Category 1-8 fix attempts, GC
tracing investigations, and per-test WPT inventories.

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

**Note on earlier diagnosis (now obsolete):** The earlier README claimed
that "all JavaScript-created promises fail to resolve `.then()` handlers".
This was incorrect.  Unit tests confirm that `Promise.resolve(42).then(h)`,
`new Promise(r => r(42)).then(h)`, and `async function() { await 1; }` all
resolve correctly through JavaScript evaluation.  The WPT stream test
failures (`TypeError: not a callable function`) likely have a separate root
cause not related to basic promise resolution.

### 5. 🟡 JSC backend not functional

JSC compiles and launches but `addEventListener` is missing, the content
process loops at 100% CPU, and WPT tests time out.  Pre-existing
condition; full JSC integration deferred.

## Tasks for migration completion

1. ✅ **`create_builtin_function` produces constructable functions** —
   Verified.  All 90 unit tests pass.

2. ✅ **`perform_promise_then` pipes result_capability** — FIXED.
   The capability promise now correctly resolves after the handler fires.

3. ✅ **`create_builtin_fn_with_captures` added** —
   Standalone function `js_engine::boa::create_builtin_fn_with_captures`
   stores captures as a concrete traceable type.  Added helper function
   `crate::js::create_builtin_fn_with_traced_captures` in content crate
   with `#[cfg]`-based backend dispatch.

4. ✅ **Wasm branding tests** — FIXED.
   `instantiate_fn` now uses `ec.with_object_any()` to access `WasmModule`.

5. 🔴 **Fix attribute accessor descriptor registration** —
   `define_regular_attributes` builds accessor descriptors but the
   properties never appear on the prototype.  Operations (value descriptors)
   work fine.  Needs comparison of data-descriptor vs accessor-descriptor
   paths in `define_property_or_throw` → Boa native conversion.

6. 🟡 **Fix `location.rs` direct downcast** —
   `self.window.downcast_ref::<Window>()` always returns `None`.  Needs
   an `ec` parameter threaded through Location navigation methods.

7. 🔍 **Audit remaining direct `downcast_ref` calls** — Find and convert
   all remaining `JsObject::downcast_ref::<T>()` calls that bypass
   `ec.with_object_any()`.

8. ✅ **Convert stream closures to use `create_builtin_fn_with_captures`** —
   All stream domain closures (`ReadableStreamDefaultController`,
   `ReadableByteStreamController`, `WritableStreamDefaultController`,
   `ReadableStream`, `TransformStream`, `ReadableStreamFromIterableState`,
   `AbortThenCancelState`, `TeeState`, `ByteTeeState`, etc.) converted
   to use `create_builtin_fn_with_traced_captures`.  Also converted
   `webidl/async_iterable.rs` closures.

9. 🟡 **WPT stream test failures** — Migration fixed many failures but
   some remain with `TypeError: not a callable function` in:
   - `default-reader.any.js` (7)
   - `from.any.js` (20)
   - `cancel.any.js` (2)
   - `count-queuing-strategy-integration.any.js` (3)
   - `templated.any.js` (2)
   - `general.any.js` (3)
   
   These may have a root cause beyond `GcBox` no-op tracing — possibly
   related to attribute descriptor registration (task 5) or another
   issue in how native functions interact with Boa's job queue.

10. **Restore JSC backend** — Wire `addEventListener`/DOM event
    infrastructure on JSC; fix the content-process infinite loop.

11. **Prune historical notes** — Remove Category 1-8 fix attempts, GC
    tracing investigations, and per-test WPT inventories.
