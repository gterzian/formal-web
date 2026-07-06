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

**Fixed in this session:** `content/src/dom/ui_event_dispatch.rs` —
`has_activation_behavior`, `run_activation_behavior`, `apply_to_event_state`.

**Unfixed:** `content/src/html/location.rs:565` (`self.window.downcast_ref::<Window>()`)
requires threading `ec` through all Location methods.

### 2. 🔴 Attribute accessor descriptors not registering on prototypes

`define_regular_attributes` builds accessor descriptors via
`create_builtin_fn` + `define_property_or_throw`, but the properties
never appear on the prototype.  Operations (methods) register fine via
value descriptors.  Suspected in the `PropertyDescriptor<BoaTypes>` →
Boa native descriptor conversion for `get`+`set`-only descriptors.

### 3. ✅ `create_builtin_function` produces constructable functions (verified)

`create_builtin_function(behaviour, length, name, true)` correctly creates
constructable functions on the Boa backend.  All 86 unit tests pass,
including `register_interface_spec` (which creates interface constructors),
`construct_calls_constructor`, and `create_builtin_function_and_call`.

The `FunctionObjectBuilder::constructor(true)` + `from_copy_closure_with_captures`
path sets `NativeFunctionObject.constructor = Some(ConstructorKind::Base)`,
which causes `NativeFunctionObject::internal_methods()` to return the
`&CONSTRUCTOR` vtable (including `native_function_construct`).

### 4. 🔴 15 unexpected Boa WPT failures — introduced by migration

81 executed, 66 PASS.  The 15 unexpected regressions were introduced by
the generic JS layer migration.  The goal for Boa is
**zero unexpected failures** — every migration regression must be fixed.

Breakdown:
- 13 readable-stream tests: `TypeError: not a callable function` — Boa
  promise microtask issue
- 2 wasm branding tests: `Module.exports: argument is not a
  WebAssembly.Module` — wasm module internal slot not wired through
  `create_builtin_function`

#### Root cause: All JavaScript-created promises fail to resolve `.then()` handlers

When a promise is created through **JavaScript evaluation** (e.g.
`Promise.resolve(42).then(h)`, `new Promise(r => r(42)).then(h)`,
`async function() { await 1; }`), the `.then()` handler NEVER fires.
`perform_a_microtask_checkpoint` → `Context::run_jobs()` →
`SimpleJobExecutor::run_jobs_async` runs, but the promise reaction job
never appears in the executor's `promise_jobs` queue — `enqueue_job` is
never called.

However, promises created through our **Rust API** via
`BoaContext::perform_promise_then` → `promise.then(on_fulfilled, on_rejected,
&mut self.context)` DO resolve correctly — the handler is called after
`run_jobs()`.  The unit test
`rooted_promise_capability_survives_gc_pressure` passes.

#### Problem: two different call paths for `Promise.prototype.then`

Boa exposes two paths for calling `Promise.prototype.then`:

**Path A — through the Rust API (`JsPromise::then`):**
```rust
// jspromise.rs:562
pub fn then(&self, on_fulfilled: Option<JsFunction>,
            on_rejected: Option<JsFunction>,
            context: &mut Context) -> JsResult<Self>
{
    Promise::inner_then(self, on_fulfilled, on_rejected, context)
        .and_then(Self::from_object)...
}
```
This goes through `Promise::inner_then` → `PerformPromiseThen`, which
calls `context.job_executor().enqueue_job(...)`.  The `Context` here is
`&mut self.context` from our `BoaContext`.  **This path works.**

**Path B — through JavaScript evaluation (`promise.then` in the VM):**
When the VM executes `promise.then(handler)`, it looks up `then` on the
promise, finds `Promise.prototype.then` (a NativeFunction), and calls it
via `native_function_call`.  Inside, `function.call(&this, &args, context)`
is called where `context` is `&mut InternalMethodCallContext` (which derefs
to `&mut Context`).  **This path fails — `enqueue_job` is never called.**

Both paths use the same `Context` and the same `PerformPromiseThen`
implementation.  The difference is how the function is invoked: Rust API
goesthrough `JsPromise::then()` which directly calls `Promise::inner_then`,
while JavaScript evaluation goes through the VM's `native_function_call`
which creates an `InternalMethodCallContext` around the `Context`.

Our `BoaContext::perform_promise_then` trait implementation (engine.rs:1638)
uses Path A:
```rust
fn perform_promise_then(&mut self, ...) -> Completion<...> {
    let result = into_completion(
        promise.then(on_fulfilled, on_rejected, &mut self.context),
        &mut self.context,
    )?;
    // TODO: _result_capability is IGNORED
    Ok(JsValue::from(result))
}
```
This works because `JsPromise::then()` directly calls `Promise::inner_then`,
bypassing the VM's `native_function_call` machinery.

The fact that `_result_capability` is ignored (marked TODO) is a symptom
that this method was written for the Rust-API path only.  When JavaScript
code calls `promise.then(handler)`, the result capability from `inner_then`
is wired into `PerformPromiseThen`, but our wrapper never uses it.

#### Failed fix attempts (2026-07-06)

1. **`create_builtin_function` from_closure → from_copy_closure_with_captures** —
   Reverted to `NativeFunction::from_closure`.  Same failure.  NOT the cause.

2. **5x loop of `context.run_jobs()` in `perform_a_microtask_checkpoint`** —
   Jobs were never enqueued; running more times doesn't help.

3. **Direct `SimpleJobExecutor::run_jobs` bypassing vtable dispatch** —
   Used `downcast_job_executor::<SimpleJobExecutor>()` + `run_jobs` on the
   concrete type.  Same failure.  `run_jobs` is not the issue.

4. **Removed explicit `job_executor(...)` from ContextBuilder** — Used the
   default executor.  Same failure.

5. **Removed custom `host_hooks(...)` from ContextBuilder** — Same failure.
   (Also caused "global object is not a Window" errors.)

6. **Pending-promise pattern via JavaScript** — Created a pending promise
   with `.then()`, then resolved via saved `resolve` reference.  This goes
   through `TriggerPromiseReactions`.  Still failed.

7. **Compared old (pre-migration) `build_boa_context` setup** — The setup
   was identical (`.host_hooks(...).job_executor(...)`).  Not the cause.

### 3. 🟡 JSC backend not functional

JSC compiles and launches but `addEventListener` is missing, the content
process loops at 100% CPU, and WPT tests time out.  Pre-existing
condition; full JSC integration deferred.

## Tasks for migration completion

1. ✅ **`create_builtin_function` produces constructable functions** —
   Verified.  The `FunctionObjectBuilder::constructor(true)` +
   `from_copy_closure_with_captures` path correctly sets
   `NativeFunctionObject.constructor = Some(ConstructorKind::Base)`.
   All 86 unit tests pass.

2. **🔴 Fix attribute accessor descriptor registration** —
   `define_regular_attributes` builds accessor descriptors but the
   properties never appear on the prototype.  Operations (value descriptors)
   work fine.  Needs comparison of data-descriptor vs accessor-descriptor
   paths in `define_property_or_throw` → Boa native conversion.

3. **🟡 Fix `location.rs` direct downcast** —
   `self.window.downcast_ref::<Window>()` always returns `None`.  Needs
   an `ec` parameter threaded through Location navigation methods.

4. **🔍 Audit remaining direct `downcast_ref` calls** — Find and convert
   all remaining `JsObject::downcast_ref::<T>()` calls that bypass
   `ec.with_object_any()`.

5. **🔴 Fix readable-stream / promise-resolution WPT failures** —
   All JavaScript-created promises fail to resolve `.then()` handlers.
   Not a `create_builtin_function` issue or executor dispatch issue.
   Suspected: Boa version-specific bug in `PerformPromiseThen` or
   `HostEnqueuePromiseJob` at rev `7ce9cae`.  Next step: `cargo update -p
   boa_engine` to try a newer revision.

6. **Restore JSC backend** — Wire `addEventListener`/DOM event
   infrastructure on JSC; fix the content-process infinite loop.

7. **Prune historical notes** — Remove Category 1-8 fix attempts, GC
   tracing investigations, and per-test WPT inventories.
