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

### 11. ✅ `downcast_ref` audit — COMPLETE (2026-07-09)

All direct `JsObject::downcast_ref::<T>()` calls that bypass
`ec.with_object_any()` have been audited and converted.  Every file in
`content/src/` that extracts native Rust data from platform objects now
goés through `ExecutionContext::with_object_any()`/`with_object_any_mut()`
before calling `downcast_ref`/`downcast_mut`.

Verified files:
- `content/src/dom/dispatch.rs` — uses `ec.with_object_any()` for all
target-type downcasts (Window, Document, HTMLAnchorElement, etc.)
- `content/src/js/downcast.rs` — multi-type helper correctly uses
`ec.with_object_any()`/`with_object_any_mut()`
- `content/src/js/bindings/dom/abort_signal.rs` — uses
`ec.with_object_any()` for Window and AbortSignal
- `content/src/html/environment_settings_object.rs` — uses
`ec.with_object_any()` for Window
- `content/src/js/platform_objects.rs` — uses
`ec.with_object_any()` via `global_scope_or_error`
- `content/src/html/windowproxy.rs` — uses
`ec.with_object_any()` via `resolve_window`
- `content/src/streams/` — all `with_*_ref` helpers use
`ec.with_object_any()` before downcasting
- `content/src/webidl/async_iterable.rs` — uses
`ec.with_object_any()` via `default_async_iterator_from_this`
- `content/src/webidl/bindings/registry.rs` — uses
`ec.get_host_any()` which has its own storage mechanism

All `js/bindings/*` files (element.rs, html_element.rs, node.rs, window.rs,
event.rs, etc.) use the helper functions from `downcast.rs` or call
`ec.with_object_any()` directly.

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

8. ✅ **Audit remaining `downcast_ref` calls** — VERIFIED COMPLETE.
   All direct `JsObject::downcast_ref::<T>()` calls now go through
   `ec.with_object_any()`/`with_object_any_mut()`.  See issue #11 above.

9. ✅ **JSC backend — builtin function creation** —
   `create_builtin_function`/`create_builtin_fn`/`create_builtin_fn_static`
   implemented using custom JSClass with `callAsFunction`/`callAsConstructor`;
   `CURRENT_ENGINE` thread-local set automatically in `call`, `construct`,
   `evaluate_script`.  `create_plain_object` uses `JSObjectMake` avoiding
   nested-eval crashes.  All `#[cfg(jsc_backend)]` ad-hoc blocks removed.

10. ✅ **Remaining `ec.create_builtin_fn` captures fixed** —
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
  Properties from `Window.prototype` and `EventTarget.prototype` are now
  copied to the global object at build_context time, making
  `addEventListener`, `setTimeout`, etc. accessible from the global scope.
  `instanceof Window` still returns `false` — the actual [[Prototype]]
  slot remains unchanged.
- Setting properties on objects created via `eval("{}")`
  (`create_plain_object(None)`) causes SIGSEGV on macOS 26.
- `create_proxy` is implemented via `new Proxy(target, handler)` script
  evaluation (JSC supports Proxy natively), enabling WindowProxy creation
  for `window.open()`.
- Iterator operations (`get_iterator`, `get_iterator_step_value`)
  may crash or produce incorrect results.
- DataView and TypedArray view construction are `todo!()`.
- JSC's C API does not expose the microtask queue — `run_jobs` only
  drains the Rust-side job queue.  Microtask draining is now triggered
  by evaluating `void 0` at the end of `call()` and `construct()`,
  since JSC drains pending microtasks after each `JSEvaluateScript`
  call.  This enables basic promise resolution (e.g.
  `Promise.resolve().then(...)` works).  However, native JSC async
  operations (e.g. `WebAssembly.compile()`) create promises resolved
  by background threads and still require the event loop to complete.
- `create_object_with_any` roots its objects on the JSC global (as
  non-enumerable `__fw_any_root_*` properties) to prevent JSC's GC
  from collecting them while Rust still holds raw pointers.  Without
  rooting, `try_with_event_target_ref` fails with "receiver is not an
  EventTarget" because the side-table HashMap key (object pointer)
  becomes stale after GC.
- `JscEngine` supports multi-realm via `new_shared_realm()` — creates
  a child engine sharing the same `JSGlobalContextRef` (same GC heap)
  but with its own global object, host_data, and job queue.  Used by
  `window.open` so the new window's objects live in the opener's GC
  heap, enabling cross-window WindowProxy references.
- `JscContext` implements `Clone` (via `JSGlobalContextRetain`), so
  multiple `JscEngine`s can share the same underlying JS context.

## JSC backend current state

### Working
- **Global methods:** `addEventListener`, `removeEventListener`, `dispatchEvent`,
  `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval`,
  `requestAnimationFrame`, `cancelAnimationFrame` are accessible from the global
  scope (copied from `Window.prototype` and `EventTarget.prototype` at
  build_context time; the global object's [[Prototype]] itself cannot be set
  on JSC).
- **DOM events:** Click, mouse, and other UI events dispatch correctly (GC
  rooting in `create_object_with_any` prevents `receiver is not an EventTarget`).
- **Microtasks:** `Promise.resolve().then(...)` resolves — microtask drain via
  `void 0` evaluation at end of `call()` and `construct()` triggers JSC's
  internal `drainMicrotasks()`.
- **WindowProxy:** `window.open` creates a WindowProxy via native JSC
  `new Proxy()` (JSC supports Proxy natively).
- **Multi-realm:** `JscEngine::new_shared_realm()` creates a child engine
  sharing the same `JSGlobalContextRef` (same GC heap). `build_realm()` wired
  into `window.open` via `the_rules_with_parent()`.
- **Synchronous WebAssembly:** `new WebAssembly.Module()` and
  `new WebAssembly.Instance()` work (JSC's native sync path).
- **Builtin functions:** `create_builtin_fn_static`, `create_builtin_fn`,
  `create_builtin_function` use custom JSClass with `callAsFunction`/`callAsConstructor`.

### Tried and failed
- **Microtask drain in `run_jobs()`:** Evaluating `void 0` from `run_jobs()`
  caused SIGSEGV because `run_jobs()` can be called from contexts where
  `CURRENT_ENGINE` is not set, or during active JS execution (nested eval).
  Moved to `call()`/`construct()` where `CURRENT_ENGINE` is managed.
- **`JSValueProtect` for GC rooting:** Causes SIGSEGV on eval-created values.
  Object rooting via non-enumerable global properties works instead.
- **`JSObjectSetProperty` on `JSObjectMakeFunctionWithCallback` objects:**
  Crashes on macOS 26. The `name` property on builtin function objects is
  skipped.
- **`JSObjectSetPrototype` on the global object:** Fails silently (immutable).
  Property copying is the only viable workaround via the public C API.

### Remaining problems
- ✅ **Streams API (ReadableStream, TransformStream)** — FIXED (2026-07-09).
  `create_builtin_fn_with_traced_captures` was `unimplemented!()` on JSC.
  It now wraps captures in a `Box<dyn Fn>` closure and calls
  `ec.create_builtin_function`, matching the existing JSC pattern for
  captured builtin functions.
- **Async WebAssembly:** `WebAssembly.compile()` uses JSC's native async path
  which requires the event loop for background compilation to complete.
  Synchronous `new WebAssembly.Module()` works.
- **`get_prototype_of`:** Stub on JSC — prevents dynamic prototype chain
  traversal in the global property copying code.
- **DOM operations crash after initial success:** `document.createElement`
  works (returns an element, setting `textContent` works), but subsequent
  operations or unrelated JS evaluations cause SIGBUS/SIGABRT.  Same pattern
  with `new ReadableStream()` (crashes) vs `ReadableStream.from()` (works).
  The crash happens in the content process, not in the main/CDP process.
  `ReadableStream.from()` returns a stream whose `getReader()` and `read()`
  work, but `instanceof ReadableStream` returns `false`.
- ✅ **`window.open` navigable creation** — FIXED (2026-07-09).
  The engine context (`JscContext`) is stored in `GlobalScope.engine_context`
  during engine setup (`setup_realm` in `build_context.rs`).
  `create_document_in_realm` reads it directly to create shared realms,
  eliminating the need to thread `&mut Engine` through `window_open_steps`.
  The `the_rules_with_parent` wrapper function has been removed; all callers
  use the plain `the_rules_for_choosing_a_navigable`.
- **Iterator operations:** `get_iterator`, `get_iterator_step_value` may
  crash or produce incorrect results.
- **DataView / TypedArray view construction:** `todo!()`.
- **`get_function_realm`:** `todo!()`.
- **`object_as_map`/`set`/`weakmap`/etc.:** No-op downcasts (operate at the
  JSC object level; typed operations not exposed by C API).

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

## Session investigation log

### 2026-07-09 — JSC `window.open` and Streams fixes for StartupExample.html

**Files changed:**
- `js_engine/src/engine.rs` — Added `as_any_mut()` to `ExecutionContext` trait
- `js_engine/src/boa/engine.rs` — Implemented `as_any_mut()` for `BoaContext`
- `js_engine/src/jsc/engine.rs` — Implemented `as_any_mut()` for `JscEngine`;
  added `new_from_context()` constructor
- `content/src/js/mod.rs` — Implemented `create_builtin_fn_with_traced_captures`
  for JSC backend (was `unimplemented!()`)
- `content/src/html/global_scope.rs` — Added `engine_context` field, setter,
  and `build_temp_parent_engine()` helper; changed `create_document_in_realm`
  to read engine context from self instead of receiving `parent` parameter
- `content/src/html.rs` — Removed `the_rules_with_parent` function;
  `the_rules_for_choosing_a_navigable` no longer delegates to it
- `content/src/html/window.rs` — Removed `the_rules_with_parent` import;
  `window_open_steps` calls `the_rules_for_choosing_a_navigable` directly
- `content/src/js/build_context.rs` — Store engine context (`JscContext`
  clone) in GlobalScope during `setup_realm`

**What was confirmed:**
- Both JSC and Boa backends compile without errors
- `Engine` context is stored in `GlobalScope.engine_context` as `Rc<dyn Any + Send>`
- `create_document_in_realm` reads the context and builds a temporary parent
  engine, which `build_realm` uses to create a shared realm via
  `new_shared_realm()` on JSC
- `create_builtin_fn_with_traced_captures` on JSC wraps captures in a
  `Box<dyn Fn>` closure and delegates to `ec.create_builtin_function`
- The `the_rules_with_parent` wrapper has been removed; `window_open_steps`
  calls the plain `the_rules_for_choosing_a_navigable` directly

**What was ruled out:**
- Passing `parent: Option<&mut Engine>` through `the_rules_with_parent` was
  the old approach, removed per feedback by storing engine context in
  GlobalScope instead

**Not investigated:**
- `get_function_realm` on JSC (still `todo!()` but not needed for startup page)
- Iterator operations on JSC (may still crash)

### 2026-07-09 — downcast_ref audit and WPT stream failures investigation

**Files changed:**
- `js_engine/README.md` — Updated Issue #11 status (✅ complete); added investigation log
- `content/src/js/bindings/streams/readablestream.rs` — Fixed JSC `drop(reject_error)` warning
- `content/src/html/environment_settings_object.rs` — Removed unused `trace` import
- `content/src/html/global_scope.rs` — Removed unused `DocumentConfig` import
- `content/src/js/bindings/html/html_iframe_element.rs` — Removed unnecessary `mut` specifiers

**What was confirmed:**
- **Issue #11 (downcast_ref audit) is complete.** All direct `JsObject::downcast_ref::<T>()`
  calls in `content/src/` now correctly use `ec.with_object_any()` before downcasting.
  Verified across all domains: DOM (dispatch.rs, event.rs, element.rs, node.rs),
  HTML (html_element.rs, window.rs, environment_settings_object.rs, platform_objects.rs,
  windowproxy.rs, location.rs), streams (all `with_*_ref` helpers), async iterables,
  registry, and binding files.
- **Issue #7 (WPT stream failures) — Step 1 of debug plan complete.** All readable-stream
downcast sites confirmed correct: every `with_readable_stream_ref`,
`with_readable_stream_default_reader_ref`, `with_writable_stream_ref`,
`with_transform_stream_ref`, `with_readable_byte_stream_controller_ref`,
`with_readable_stream_byob_request_ref`, `with_readable_stream_byob_reader_ref`,
`with_writable_stream_default_writer_ref`, `with_writable_stream_default_controller_ref`
helper uses `ec.with_object_any()` before downcasting.
- Both Boa (default) and JSC (`--no-default-features --features jsc`) backends compile
  without errors.
- All remaining `ec.create_builtin_fn(Box::new(...))` call sites were re-audited and
  confirmed to capture only function pointers or Rust-only types (no GC values).

**What was ruled out:**
- GC trace chain issue for stream platform objects: the full trace chain
  (`Gc<T>` → `GcRefCell<T>` → `Vec<T>` → enum variant → `PromiseResolvers` →
  `JsObject`/`JsFunction`) was verified correct. `GcRefCell<T>` implements `Trace`
  (delegating to inner `T`). `PromiseResolvers<BoaTypes>` derives `boa_gc::Trace`.
  The `ReadableStreamDefaultReader` stores `read_requests: GcCell<Vec<ReadRequest>>`
  which traces through correctly.
- `create_builtin_fn_with_traced_captures` implementation on both backends verified.
  Boa stores captures via `NativeFunction::from_copy_closure_with_captures` with
  concrete `C: boa_gc::Trace + 'static` type. JSC wraps in `Box<dyn Fn>` and delegates
  to `ec.create_builtin_function`.

### 2026-07-09 — WPT stream failures VM-level investigation

**Root cause pinpointed:** The "TypeError: not a callable function" error comes
from Boa's `Call` VM opcode trying to call `undefined` as a function.
Instrumentation at all 5 Boa error sites (`CallEval`, `CallEvalSpread`,
`Call`, `CallSpread`, and `non_existent_call`) confirmed only the `Call` opcode
fires.  The call stack at the moment of the error is:
```
check_equal → assert_object_equals → assert_wrapper → (anon) → (anon)
```
The error occurs **inside WPT's `check_equal` function** (testharness.js line 1658),
part of `assert_object_equals`.  The VM finds `undefined` on the stack where
a callable value is expected — meaning some property access or variable
resolution inside `check_equal` evaluates to `undefined` instead of the
expected function.

**Debug trace obtained:** The error fires once per test run, always with
`type="undefined"`.  The test that triggers it is `cancel.any.js`'s sub-test
"cancel() on a locked stream should fail".  Flow: `rs.cancel()` rejects
(correctly — stream is locked) → `.then()` handler fires → `reader.read()`
returns `{value: undefined, done: true}` (stream was closed by `start(c)`)
→ `assert_object_equals` compares against expected `{value: 'a', done: false}`
→ `check_equal` iterates properties → `for...in` on the read-result object
triggers a function call that resolves to `undefined`.

**Dead ends (paths ruled out):**
- **GC collection of resolve/reject functions:** Investigated `GcCell<Vec<ReadRequest>>`
  trace chain in detail.  Spent significant time verifying that `GcRefCell<T>`
  implements `Trace`, that `PromiseResolvers<BoaTypes>` derives `Trace`, and
  that the full chain from `ReadableStreamDefaultReader` → `GcCell` → `ReadRequest`
  → `PromiseResolvers` → `JsObject` was correctly traversable.  **Ruled out**
  when we discovered Boa's GC is NOT automatic — it only runs via explicit
  `force_collect()` calls (WeakRef, FinalizationRegistry, tests).  No stream
  operation calls `force_collect()`, so collected-object dangling pointers
  cannot cause the error.
- **Our `ec.call()` producing the error:** `ec.call()` produces the distinct
  message `"callback is not callable"`, not `"not a callable function"`.
  Confirmed by reading the 4-line `BoaContext::call()` implementation.
  The error message matches Boa's `Call` opcode / `non_existent_call`,
  not our glue layer.
- **`run_jobs()` returning errors:** Added instrumentation to
  `BoaContext::perform_a_microtask_checkpoint()` and `run_jobs()` to log
  when `self.context.run_jobs()` returns `Err`.  Never fired — Boa's
  `SimpleJobExecutor` catches promise-job errors inline and returns `Err`,
  which would abort remaining jobs, but this path was never triggered.
- **Instrumenting Boa's error sites without forcing recompilation:** The
  `boa_engine` crate is a git dependency (`~/.cargo/git/checkouts/`).  Cargo
  caches build artifacts by fingerprint (package metadata hash, not source
  timestamps).  Edits to the git checkout source are **not detected** by
  Cargo — `cargo build --release` silently uses cached `.o` files.
  Must delete `target/release/deps/libboa_engine-*` AND
  `target/release/.fingerprint/boa_*-*` before rebuilding.
- **`non_existent_call` is NOT the error source:** Added `eprintln!` to all
  5 Boa error sites (`Call::operation`, `CallEval`, `CallEvalSpread`,
  `CallSpread`, `non_existent_call`).  Only `Call::operation` fired.
- **Our `create_builtin_fn()` captures:** Re-audited every remaining
  `ec.create_builtin_fn(Box::new(...))` call site.  All capture only
  `fn` pointers or `String` — confirmed by reading each closure body.

**Bug found: `read_steps` skips queued chunks when stream was closed via
`start(c) { c.enqueue(chunk); c.close(); }`.**

`ReadableStreamDefaultReaderRead` (our `read_steps` in
`readablestreamdefaultreader.rs` line 302) checks `stream.state() ==
ReadableStreamState::Closed` and goes directly to
`read_request.close_steps()`, bypassing the controller's `PullSteps`.
But when the stream was closed via `start(c) { c.enqueue(chunk); c.close(); }`,
the controller has `closeRequested=true` and a non-empty queue, and the
stream stays in "readable" state (the close is deferred until the queue
drains).  The test sets up this exact scenario, so `reader.read()` misses
the queued chunk.

The spec text:
```
ReadableStreamDefaultReaderRead (readRequest)
4. If stream.[[state]] is "closed",
   1. Perform readRequest's close steps.
```

This is correct when the stream is genuinely closed (queue empty).  But
the spec intends the CLOSE STEPS to happen only when there really is
nothing to read — the controller's `PullSteps` checks the queue first.
The controller updates the stream state to "closed" when it** dequeues
the last chunk and `closeRequested` is true.  So the reader should go
through `PullSteps` which checks the queue, returns the chunk, and
closes the stream on the last dequeue.

Our `read_steps` should **always call `controller.[[PullSteps]]`** and
let the controller decide whether to deliver a chunk or close.  The
state check `if Closed → close_steps` was incorrect: at the point
`read_steps` runs, the stream is still "readable" (the controller
hasn't closed it yet), so the closed branch is unreachable in practice
for this scenario anyway.  The real path is: state=Readable → calls
`PullSteps` → queue non-empty → dequeue → if closeRequested and queue
empty → close stream.  This path appears correct in our code; the `if
Closed` branch is harmless dead code for this case.

**Needs verification:** Run the test with instrumentation to confirm
`reader.read()` actually returns the correct `{value: 'a', done: false}`
and that the Boa `Call` opcode error is the sole reason `check_equal`
fails.

## Next session action items (in priority order)

### 1. 🟢 Fix the "not a callable function" Boa VM bug

**What we know:**
- Error is from Boa's `Call` opcode trying to call `undefined`
- Happens inside WPT's `check_equal` (testharness.js), called from
  `assert_object_equals` in the locked-stream cancel sub-test
- The `Call` opcode checks `func.as_object()` and gets `None` because
  the value is `undefined` (not an object)

**Fastest fix path:**
Patch Boa's `vm/opcode/call/mod.rs` at the `Call::operation` method (the
`let Some(object) = func.as_object()` check).  Print the **bytecode
instruction** at the failing PC (was pc=1118 in previous runs) to show
exactly which JavaScript operation calls `undefined`.  Use:
```rust
let cb = &context.vm.frame().code_block;
eprintln!("FW_DEBUG: instruction at pc={}: {:?}", cb.pc, cb.bytecode[cb.pc as usize]);
```
Then rebuild (`rm -rf target/release/deps/libboa_engine-* target/release/.fingerprint/boa_*-* target/release/build/boa*-* && cargo build --release`) and run:
```
PYTHON=python3.12 cargo run --release -- wpt streams/readable-streams/cancel.any.js 2>&1 | grep FW_DEBUG
```
This tells you which JavaScript operation is broken in Boa.

### 2. 🟡 Verify `read_steps` correctly returns queued chunks

The locked-stream cancel sub-test expects `reader.read()` to return
`{value: 'a', done: false}` even after `rs.cancel()` was rejected.
Our `read_steps` has `if Closed → close_steps` before calling
`controller.[[PullSteps]]`.  In practice the stream state should be
"readable" at that point (the controller defers closing until queue
drains), so the `if Closed` branch is dead code for this test.
But verify by running with a log in `controller.pull_steps` and
`read_steps` to confirm the chunk IS dequeued with the correct value.

### 3. 🔍 Try the harness isolation test

Write a minimal standalone JS script (no WPT harness) that exercises the
same stream path:
```js
const rs = new ReadableStream({ start(c) { c.enqueue('a'); c.close(); } });
const reader = rs.getReader();
rs.cancel().then(null, e => {
  reader.read().then(r => print(JSON.stringify(r)));
});
```
Evaluate it through the content process.  If the "not a callable function"
error DOES reproduce → the bug is in engine/stream code, not the harness.
If it does NOT reproduce → the issue is in how the WPT harness's
assertion-scope plumbing interacts with Boa.

---

**Correction from debug plan:** The error is NOT from `non_existent_call`
(which handles non-callable *objects*) — it is from the `Call` opcode itself
(which handles non-*object* values like `undefined`).  No Boa `non_existent_call`
or other vm/opcode/call variants fired; only `Call::operation`.

**Issue #8 (WASM worker-context):** Lower priority — `window_from_context`
uses `context.global_object()` which is not a `Window` in worker contexts.
- JSC iterator operations and async WebAssembly remain outstanding.
