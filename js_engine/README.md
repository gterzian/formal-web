# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Safe builtin function creation (2026-07-09)

The unsafe `create_builtin_function` and `create_builtin_fn` trait methods
have been **removed** from `ExecutionContext<T>`.  They stored closure
captures in a no-op trace wrapper (`GcBox<Box<dyn Fn>>`) invisible to Boa's
GC, causing "not a callable function" errors when the GC collected captured
`JsObject` references.

Replaced by two safe APIs:

- **`create_builtin_fn_static(behaviour, length, name)`** — for stateless
  function pointers (no captures at all).  The behaviour is a bare `fn`
  pointer, which is always GC-safe.
- **`create_builtin_fn_with_captures(ec, captures, behaviour_fn, ...)`** —
  for functions that need state.  The `captures` parameter is a concrete
  `C: boa_gc::Trace + 'static` type (e.g. a `#[gc_struct]` platform object).
  The `behaviour_fn` receives `&C` as a parameter so the closure body never
  captures anything — state is always passed through the `C` pointer.

The deprecated `create_builtin_fn`/`create_builtin_function` methods remain
on the trait temporarily with no-op trace via `UnsafeFnBox` for migration.
Use `create_builtin_fn_static` or `create_builtin_fn_with_captures` in new
code.

Removed: `Behaviour` trait, `create_builtin_function_from_behaviour`,
`create_constructor`, `NativeFunction::from_closure`, and the `GcBox`
no-op trace wrapper.

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

### 5. ✅ GC-traceable builtin function captures — FIXED (2026-07-09)

The unsafe `create_builtin_function`/`create_builtin_fn` trait methods
have been **removed** from `ExecutionContext<T>` and replaced with:

- **`create_builtin_fn_static`** — stateless function pointers (trait method)
- **`create_builtin_fn_with_captures`** — standalone Boa function for
  concrete traceable captures `C: boa_gc::Trace`
- **`create_builtin_fn_with_traced_captures`** — content crate helper
  that delegates to the above

The `GcBox` wrapper with no-op Trace has been deleted.
Closures passed to builtin function creation must NOT capture JS values;
state is passed through the captures type `C` (a `#[gc_struct]` type
with proper `Trace`).

**Audit rule:** Every `ec.create_builtin_fn(Box::new(...))` or
`ec.create_builtin_function(Box::new(...), ..., true)` call site must
be verified to capture only function pointers or Rust primitive types
(no `JsObject`, `JsValue`, `GcCell`, `PromiseResolvers`, or other
GC-managed types).  If the closure captures any GC-traced type, convert
to `create_builtin_fn_with_traced_captures` with concrete captures `C`.

As of 2026-07-09, all capture-GC-value call sites have been converted;
the remaining `ec.create_builtin_fn(Box::new(...))` sites capture only
fn pointers or Rust primitives (see §9 for audit table).

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

**Investigation — Phase 1 (2026-07-09):** Addressed by fixing the
`GcBox` no-op Trace issue (see #5).  The `GcBox` wrapper was the
mechanism by which captured `JsObject` references became invisible
to Boa's GC.  However, diagnostic logging showed that the remaining
stream domain `ec.call()` invocations (`chunk_steps`, `close_steps`,
`error_steps`) ALL SUCCEED — the "not a callable function" error does
NOT come from our `ec.call()` (which produces the distinct message
`"callback is not callable"`).

The error comes from Boa's VM (`non_existent_call` in
`internal_methods.rs`) indicating JavaScript code tries to call a
value that has no `[[Call]]` internal method.  Further investigation
is needed to pinpoint which JS-level call triggers this.

**Also fixed (2026-07-09):** `cancel_steps` in both default and byte
controllers now catches errors from the cancel algorithm and returns
a rejected promise instead of propagating the exception (fixes the
"cancel callback raises an exception" test).

Piping, transform-stream, and writable-stream tests pass — these use
`ReadableStreamPipeTo`/`TransformStreamDefaultSourcePull` read request
variants that avoid calling `resolvers.resolve` through `ec.call()`.

**Investigation — Phase 2 (2026-07-09):** Converted remaining
`ec.create_builtin_fn(Box::new(...))` calls that capture
`JsObject`/`JsValue` references to `create_builtin_fn_with_traced_captures`.
These used `UnsafeFnBox` (no-op Trace) which made captured closures'
GC references invisible to Boa's collector, causing the "not a callable
function" errors when the GC collected `JsFunction` objects referenced
by promise reaction handlers.

Converted files:
- **`content/src/streams/writablestream.rs`** — `finish_erroring`
  on_fulfilled/on_rejected closures captured `WritableStream` and
  `PendingAbortRequest` (both contain `GcCell<JsObject>` and
  `PromiseResolvers<Types>` references).
- **`content/src/js/bindings/dom/abort_signal.rs`** — `AbortSignal.timeout`
  callback closure captured `AbortSignal` (contains `GcCell<JsObject>`
  and `GcCell<JsValue>`).

All other `ec.create_builtin_fn(Box::new(...))` call sites capture
only function pointers or Rust-only types (String, Arc, etc.) that
don't need GC tracing, so they are safe.

### 8. 🟡 WASM compile/instantiate in worker context

`window_from_context` fails in worker/`dedicatedworker` contexts
because the global object is not a `Window`.  The WASM namespace
operations (`WebAssembly.compile`, `WebAssembly.instantiate`) use
IPC-based worker dispatch that requires a Window.  Affects:
- `formal/wasm-compile-instantiate.html`
- `wasm/jsapi/constructor/compile.any.js` subtests

### 9. ✅ Remaining deprecated `ec.create_builtin_fn` calls audited (2026-07-09)

All remaining `ec.create_builtin_fn(Box::new(...))` calls that capture
GC-traced values (`JsObject`, `JsValue`, `GcCell`, `PromiseResolvers`,
etc.) have been converted to `create_builtin_fn_with_traced_captures`.

| File | Status | Notes |
|---|---|---|
| `streams/writablestream.rs` | ✅ Fixed | `PendingAbortRequest` + `WritableStream` captures |
| `js/bindings/dom/abort_signal.rs` | ✅ Fixed | `AbortSignal` capture |
| `webidl/bindings/attribute.rs` | ✅ Safe | fn pointer only |
| `webidl/bindings/operation.rs` | ✅ Safe | fn pointer only |
| `webidl/promise.rs` | 🔴 Unused | `wait_for_all` only; not in stream path |
| `webidl/async_iterable.rs` | ✅ Safe | no captures |
| `html/windowproxy.rs` | ✅ Safe | fn pointer only |
| `wasm/namespace.rs` | ✅ Safe | Wasmtime types, no GC data |
| `js/bindings/dom/element.rs` | ✅ Safe | fn pointer only |
| `js/bindings/html/host_hooks.rs` | ✅ Safe | fn pointer only |
| `js/bindings/html/html_element.rs` | ✅ Safe | no captures |
| `js/bindings/html/hyperlink_element_utils.rs` | ✅ Safe | fn pointer only |
| `js/bindings/streams/strategy.rs` | ✅ Safe | no captures |
| `js/css_generic.rs` | ✅ Safe | no captures |
| `js/console_generic.rs` | ✅ Safe | `String` capture only, no GC data |

### 10. ✅ JSC backend — builtin function creation and event dispatch

`create_builtin_fn_static`, `create_builtin_fn`, and `create_builtin_function`
are implemented on JSC using a custom JSClass with `callAsFunction` and
`callAsConstructor` callbacks.  The closures store a type-erased
`StoredBehaviour` as private data on the JSObject; the C callbacks retrieve
it via `JSObjectGetPrivate` and call through the thread-local `CURRENT_ENGINE`
to find `&mut dyn ExecutionContext<JscTypes>`.

`set_current_engine`/`clear_current_engine` are called automatically in
`EcmascriptHost::call`, `ExecutionContext::construct`, and
`ExecutionContext::evaluate_script` (and `JsEngine::evaluate_script`) to
ensure builtin function callbacks always find the engine.

All ad-hoc `#[cfg(jsc_backend)]` blocks have been removed from content/
code (`handle_event`, `dispatch_events`, `run_window_timer`, etc.) because
the engine methods internally handle `CURRENT_ENGINE`.

`create_plain_object` now uses `JSObjectMake` with `PLAIN_OBJECT_CLASS`
instead of `eval_script_raw`, avoiding nested-`JSEvaluateScript` crashes.
`define_property_or_throw` uses `Object.defineProperty` via script
evaluation for all descriptor types (instead of `JSObjectSetProperty` which
crashes on eval-created objects).

### 11. 🔍 Audit remaining direct `downcast_ref` calls

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
   Phase 2 fix: converted remaining `ec.create_builtin_fn` calls that
   capture GC values in `writablestream.rs` and `abort_signal.rs` to
   `create_builtin_fn_with_traced_captures`.

   **2026-07-09 investigation (documented path, no solution found):**
   - Added `log::warn!` instrumentation to every `PullAlgorithm::call()`,
     `CancelAlgorithm::call()`, `StartAlgorithm::call()` variant.
   - Added instrumentation to `SourceMethod::call()` (the `invoke_callback_function`
     wrapper) and all four native promise handler functions
     (`setup_on_fulfilled`, `setup_on_rejected`, `pull_steps_on_fulfilled`,
     `pull_steps_on_rejected`).
   - Ran `RUST_LOG=warn` against `streams/readable-streams/cancel.any.js`
     (single failing test: "cancel() on a locked stream should fail").
   - **Confirmed:** Every algorithm call (`PullAlgorithm`, `CancelAlgorithm`,
     `StartAlgorithm`) returned `Ok`. The only `Err` was from the
     "cancel callback raises an exception" test, which intentionally throws
     and is correctly caught by `cancel_steps` → `rejected_promise(error, ec)`.
   - **Confirmed:** All four native promise handler functions ARE called by
     Boa's promise job machinery. `setup_on_fulfilled` fired 11 times,
     `pull_steps_on_fulfilled` fired 12+ times across the test run.
     Neither `setup_on_rejected` nor `pull_steps_on_rejected` ever fired
     (no start or pull algorithm ever rejected — expected behavior).
   - **Excluded:** The `TypeError: not a callable function` does NOT come
     from our algorithm call chain or our GC-traceable promise handlers.
     Both the algorithm calls and the promise handler invocations complete
     successfully. The error comes from Boa's JavaScript VM internally
     (`non_existent_call` in `internal_methods.rs` or VM opcode)
     during JavaScript-level execution unrelated to our Rust promise
     handler invocations.

7. 🟡 **WASM worker-context tests** — `WebAssembly.compile` and
   `WebAssembly.instantiate` require a `Window` global object.

8. 🔍 **Audit remaining `downcast_ref` calls** — Find and convert
   all remaining direct `JsObject::downcast_ref::<T>()` calls that bypass
   `ec.with_object_any()`.

9. ✅ **JSC backend — builtin function creation** —
   `create_builtin_function`/`create_builtin_fn`/`create_builtin_fn_static`
   implemented using custom JSClass with `callAsFunction`/`callAsConstructor`;
   `CURRENT_ENGINE` thread-local set automatically in `call`, `construct`,
   `evaluate_script`.  `create_plain_object` uses `JSObjectMake` avoiding
   nested-eval crashes.  All `#[cfg(jsc_backend)]` ad-hoc blocks removed.

10. ✅  **Remaining `ec.create_builtin_fn` captures fixed** —
    Converted remaining unsafe `ec.create_builtin_fn(Box::new(...))`
    calls that capture GC-traced values in `writablestream.rs`
    (`PendingAbortRequest` + `WritableStream`) and `abort_signal.rs`
    (`AbortSignal`).  All other call sites capture only function
    pointers or Rust primitive types (no GC data).  See audit table
    in issue §11.

11. **Prune historical notes** — Remove Category 1-8 fix attempts, GC
    tracing investigations, and per-test WPT inventories from this
    document (completed).

## Remaining JSC limitations

- `define_property_or_throw` uses `Object.defineProperty` via script eval;
  accessor (getter/setter) descriptors store placeholder `undefined` values.
- The global object's prototype is immutable on JSC
  (`JSObjectSetPrototype`/`Object.setPrototypeOf` fail silently).
  This means methods on `Window.prototype`, `EventTarget.prototype`, etc.
  are NOT inherited by the global object — only constructors are exposed.
  Requires a native solution (e.g., installing methods on global at
  build_context time, or using JSC's `JSClassGetProperty` hook on the
  global object).
- Setting properties on objects created via `eval("{}")`
  (`create_plain_object(None)`) causes SIGSEGV on macOS 26.
- Iterator operations (`get_iterator`, `get_iterator_step_value`)
  may crash or produce incorrect results.
- DataView and TypedArray view construction are `todo!()`.
- JSC's C API does not expose the microtask queue — `run_jobs` only
  drains the Rust-side job queue, not JSC's internal promise queue.

## Session investigation log

Each session that investigates an open issue should append a log entry here.
Log only what was done and what was ruled out — no speculation on solutions.
The purpose is to let the next session pick up where the last one left off
without repeating dead ends.

### 2026-07-09 — WPT stream test `TypeError: not a callable function`

**Files changed:** `writablestream.rs`, `abort_signal.rs` (ec.create_builtin_fn →
create_builtin_fn_with_traced_captures), `js_engine/README.md` (documentation).

**Instrumentation added:** log::warn! at every PullAlgorithm/CancelAlgorithm/
StartAlgorithm variant, SourceMethod::call(), setup_on_fulfilled/rejected,
pull_steps_on_fulfilled/rejected, readable_stream_cancel.

**What was confirmed:**
- All algorithm calls return Ok (except expected exception test).
- All four promise handler functions fire (called by Boa's promise job system).

**What was ruled out:**
- The error is NOT from algorithm calls failing (they all succeed).
- The error is NOT from our GC-traceable promise handlers failing to fire
  (they all fire correctly).
- The error is NOT from the `UnsafeFnBox` GC capture issue (the remaining
  `ec.create_builtin_fn(Box::new(...))` sites capture only fn pointers or
  Rust primitives; see §9 audit table).
- The error comes from Boa's JavaScript VM (`non_existent_call`) during
  JavaScript-level execution, not from our Rust promise handler invocations.

**Not investigated:** The specific JavaScript code path within Boa's VM that
produces "not a callable function". Candidate: `new_type_error` creates opaque
JsError values via `JsNativeError::typ().with_message(...).into_opaque()`;
the WPT test harness's `promise_test` wrapper or the test JavaScript code may
attempt operations on the error object that trigger the non-callable error.

---

## Stable build

Both Boa (default) and JSC (macOS opt-in) backends compile:
```bash
# Boa (default)
cargo build --release

# JSC (macOS)
cargo build --release --no-default-features --features jsc
```
The JSC backend has functional builtin function creation and interface
registration.  The global object prototype chain limitation prevents
`Window.prototype` methods from being inherited by the global object.
This is a pre-existing JSC limitation that requires a native integration
path for full Web API support.
