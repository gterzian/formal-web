# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Architecture

> **Principle:** The architecture is defined by the standards.  We don't
> invent new layers — we follow the spec chain exactly and make it generic.

### Two paths into JavaScript

#### Path 1: Domain → Web IDL → ECMA-262

Most web-exposed APIs (Streams, DOM) call Web IDL, which calls ECMA-262.

| Layer | Example spec | Our code |
|---|---|---|
| Domain | `readable-stream-cancel` | `content/src/streams/readablestream.rs` |
| Web IDL | `a-promise-resolved-with`, `a-promise-rejected-with`, `react` | `content/src/webidl/promise.rs` |
| ECMA-262 | `PerformPromiseThen`, `NewPromiseCapability`, `CreateBuiltinFunction` | `js_engine` trait |

#### Path 2: Domain → ECMA-262 (bypasses Web IDL)

Some HTML algorithms call ECMA-262 directly (realm creation, script evaluation).

| Layer | Example spec | Our code |
|---|---|---|
| HTML | `creating-a-new-javascript-realm` | `content/src/html/` → `js_engine::create_realm()` |
| ECMA-262 | `CreateRealm` | `js_engine` trait |

**The rule:** read the spec, follow its call chain exactly.  Route through
`content/src/webidl/` only when the spec calls Web IDL.  Call `js_engine`
directly when the spec calls ECMA-262 directly.  Never insert an artificial
intermediary layer that doesn't exist in the spec.

### Crate layering

```
content/src/<domain>/           ← domain algorithms (streams, HTML, DOM)
  → content/src/webidl/          ← only when the spec calls Web IDL
  → content/src/js/bindings/     ← Web IDL interface definitions
  → js_engine trait               ← ECMA-262 abstract operations
    → js_engine/src/boa/          ← Boa impl (only here)
    → js_engine/src/jsc/          ← JSC impl (only here)
```

**Rules:**

1. **Content code never calls Boa APIs directly.**  Domain code calls
   into `content/src/webidl/` when the spec calls Web IDL, or into the
   `js_engine` trait when the spec calls ECMA-262 directly.

2. **The js_engine trait only exposes ECMA-262 operations.**  Operations
   like "report an exception" or "perform a microtask checkpoint" are
   HTML concepts — they live on `EcmascriptHost`.

3. **The webidl/ layer implements Web IDL §3.**  Type conversion,
   promise manipulation ("react", "upon fulfillment"), and the binding
   infrastructure (interface prototypes, operation/attribute definitions).

4. **The js/bindings/ layer defines which members exist.**  Each
   `WebIdlInterface` impl registers operations and attributes.  The
   binding functions themselves are thin: extract JS args, call domain,
   wrap result.

5. **Ad-hoc Boa patterns must be replaced by spec algorithms:**
   `NativeFunction::from_closure` → `create_builtin_function`,
   `JsArray::from_iter` → `create_empty_array` + `array_push`,
   `JsNativeError::syntax()` → `new_syntax_error`.

6. **Test the full chain end-to-end.**  The generic test file
   (`content/src/generic_js_test.rs`) proves every content pattern works
   through the generic API with zero `boa_engine::*` imports.

## Traits

| Trait | Role | Spec basis |
|---|---|---|
| `JsEngine<T>` | **Stateless factory** — creates realms, built-in functions. Process-level singleton. | `CreateRealm` (§9.3), `CreateBuiltinFunction` (§10.3) |
| `ExecutionContext<T>` | **Stateful runtime** — the realm execution context. Owned by `EnvironmentSettingsObject`. | HTML §8.1.3.2 → all of ECMA-262 §7 |
| `EcmascriptHost<T>` | Subset of `ExecutionContext<T>` — `Get`, `IsCallable`, `Call`, `report_exception`, value construction. Supertrait of `ExecutionContext<T>`. | Web IDL §3 |

### `ExecutionContext<T>` owns the runtime

Everything stateful: type conversion (§7.1), testing (§7.2), object
operations (§7.3 — `get`, `set`, `call`, `construct`), iteration (§7.4),
promise operations (`new_promise_capability`, `perform_promise_then`),
value construction, buffer operations, `evaluate_script`.

### `JsEngine<T>` is the factory

Stateless: `create_realm`, `set_realm_global_object`, `set_default_global_bindings`,
`create_builtin_function`, `evaluate_script` (realm-parameterized),
`evaluate_module`, buffer allocation.

## Layout

```
js_engine/src/
  lib.rs        Crate root
  types.rs      JsTypes — language types (§6.1) and object subtypes
  engine.rs     JsEngine<T>, EcmascriptHost<T>, Completion, HostHooks
  enums.rs      Numeric, PreferredType, IntegrityLevel, etc.
  records.rs    IteratorRecord, PromiseCapability, PropertyDescriptor
  gc.rs         Trace, Finalize, GcRootHandle, GcCell<T>, gc_cell_new()
  boa/          Boa backend (feature = "boa")
  jsc/          JSC backend (feature = "jsc")

js_engine_macros/ — proc-macro crate providing `#[gc_struct]`.
```

## Build & feature flags

macOS only.  JSC is the default backend.  Boa+Wasmtime is opt-in.

### Content crate backend selection

```bash
# JSC (default on macOS — no flags needed)
cargo build --release -p content

# Boa + Wasmtime
cargo build --release -p content --no-default-features --features boa,media
```

The `content/build.rs` ensures the features are mutually exclusive and sets
the `jsc_backend` / `boa_backend` cfg flags accordingly.

### js_engine backend selection

```bash
# JSC (default)
cargo check -p js_engine

# Boa
cargo check -p js_engine --no-default-features --features boa
```

### Feature flags

| Feature | Backend | Cargo.toml default |
|---|---|---|
| `jsc` | JavaScriptCore (macOS) | **default** |
| `boa` | Boa + Wasmtime | opt-in |

Mutually exclusive — only one backend at a time.

## Generic API surface

```rust
// Platform objects: create, read, mutate native data
fn create_object_with_any(prototype: T::JsObject, data: Box<dyn Any>) -> T::JsObject;
fn with_object_any(&self, obj: &T::JsObject) -> Option<&dyn Any>;
fn with_object_any_mut(&mut self, obj: &T::JsObject) -> Option<&mut dyn Any>;

// GC
#[gc_struct]  // proc-macro: derives Clone + Trace + Finalize
GcCell<T>     // Gc<GcRefCell<T>> (Boa) or Rc<RefCell<T>> (JSC)
gc_cell_new(val), .borrow(), .borrow_mut()

// Values
ec.value_undefined(), .value_null(), .value_from_bool(b)
ec.value_from_number(n), .value_from_string(ec.js_string_from_str(s))
ec.create_plain_object(prototype), .create_empty_array()
ec.array_push(&arr, val)?, .object_set_property(obj, key, val)?
ec.new_type_error(msg), .new_range_error(msg)

// Standard binding function signature
fn binding_fn(
    this: &Types::JsValue,
    args: &[Types::JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Types::JsValue, Types>;
```

## Per-backend details

| Backend | Status |
|---|---|
| Boa | ✅ Full parity — all trait methods pass |
| JSC | 🔶 Trait surface complete. `exercise_context_lifecycle` is Boa-only. |
| GC | ✅ Complete — `#[gc_struct]`, `GcCell<T>`, `GcRootHandle<T>`. |

## Design notes

- **`with_object_any` / `with_object_any_mut`:** Return `Option<&dyn Any>` /
  `Option<&mut dyn Any>` — the caller downcasts.  Object-safe on
  `&dyn ExecutionContext<T>`.
- **`with_object_any_mut_with`:** Passes both `&mut dyn Any` and
  `&mut dyn ExecutionContext<T>` to a closure for patterns where mutation
  needs ECMA-262 operations.
- **What does NOT belong on the EC trait:** `js_string_from_str` (convenience),
  `report_error` (logging), `report_exception`/`perform_a_microtask_checkpoint`
  (HTML concepts, on `EcmascriptHost`).
- **Spec documentation:** Every trait method has only the spec anchor URL as
  its doc comment — zero prose.  Infrastructure traits (`Trace`, `Finalize`)
  carry no spec links (not spec-defined).
- **Test-file-first:** Every new generic interface, downcast helper, or
  host-data abstraction must first be validated in
  `content/src/generic_js_test.rs` on both backends before production code.

## Debugging workflow

Use the **browser extension** (`.pi/extensions/browser/`) for fast interactive
feedback during development, and **WPT** for full regression verification.

### Quick feedback: browser extension

```bash
# 1. Build and start formal-web with CDP on a test page
cargo build --release --no-default-features --features boa,media
./target/release/formal-web cdp --port 9222 \
  --startup-url "file:///path/to/test.html"

# 2. Inside pi, connect the extension
# (the /browser-connect command connects automatically on first use)

# 3. Use browser_evaluate to run JavaScript
browser_evaluate({ expression: "document.getElementById('out').textContent" })
browser_evaluate({ expression: "console.log('test'); 42" })
```

The CDP tools (`browser_navigate`, `browser_evaluate`, `browser_get_text`,
`browser_screenshot`, etc.) give sub-second turnaround without needing to
restart the browser process.  Create minimal `.html` test pages in
`scratchpad/` to isolate specific patterns before running the WPT suite.

### Full verification: WPT

```bash
cargo run --release --no-default-features --features boa,media -- wpt
```

The WPT runner tests all covered APIs against the web-platform-tests suite
in `vendor/wpt/`.  Always run WPT before committing changes to verify no
regressions were introduced.  When debugging a WPT failure, isolate the
specific test first:

1. Find the test in `vendor/wpt/` (e.g. `streams/piping/close-propagation-forward.any.js`)
2. Read the test assertions to understand the expected behavior
3. Create a minimal reproduction in `scratchpad/` and run via CDP
4. Add `log::debug!` or `error!` traces, iterate with CDP, then run WPT to confirm the fix

### `ExecutionContext` — Symbol property keys

Two methods were added for well-known Symbol access:

```rust
fn property_key_from_symbol(&self, sym: &T::JsSymbol) -> T::PropertyKey;
fn property_key_from_well_known_symbol(&mut self, name: &str) -> T::PropertyKey;
```

Supported well-known symbol names: `asyncIterator`, `hasInstance`,
`isConcatSpreadable`, `iterator`, `match`, `matchAll`, `replace`,
`search`, `species`, `split`, `toPrimitive`, `toStringTag`,
`unscopables`, `dispose`, `asyncDispose`.

All `get_readable_stream_from_iterator_record` lookups now use Symbol keys.

### 🟢 Remaining `#[cfg(boa_backend)]` (intentional)

| File | `#[cfg]` lines | Reason |
|---|---|---|
| `content/src/js/build_context.rs` | 2 | Engine instantiation point (allowed exception) |
| `content/src/js/bindings/html/host_hooks.rs` | 2 (in `mod.rs` gate) | Creates Boa `Context` with `WindowHostHooks` |
| `content/src/main.rs` | 14 | All wasm-related (`pub mod wasm;`, struct fields, drain methods) |
| `content/src/generic_js_test.rs` | 2 | Test file exercising both backends |
| `content/src/wasm/` | all | Requires `wasmtime` crate, Boa-only FFI bridge |
| `content/src/js/bindings/wasm/` | all | Requires `wasmtime`, Boa-only |

### Replacement reference

| Boa-specific | Generic replacement |
|---|---|
| `js_string!("foo")` | `ec.property_key_from_str("foo")` or `ec.js_string_from_str("foo")` |
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `JsPromise::new_pending(context)` | `ec.new_promise_pending()?` |
| `JsPromise::from_object(p)?.then(...)` | `ec.perform_promise_then(...)` |
| `JsPromise::from_object(x)?.state()` | `ec.promise_state(&x)?` |
| `NativeFunction::from_closure(closure)` | `ec.create_builtin_function(Box::new(behaviour), length, name)` |
| `NativeFunction::from_copy_closure_with_captures(...)` | `ec.create_builtin_function_with_captures(captures, behaviour, length, name)` |
| `JsObject::downcast_ref::<T>()` | `ec.with_object_any(&obj).and_then(|d| d.downcast_ref::<T>())` |
| `JsObject::from_proto_and_data(proto, data)` | `ec.create_object_with_any(prototype, Box::new(data))` |
| `boa_gc::GcRefCell::new(val)` | `js_engine::gc::gc_cell_new(val)` |
| `boa_engine::JsResult<T>` | `Completion<T, Types>` |
| `PropertyDescriptor::builder().value(v).writable(true).build()` | `PropertyDescriptor { value, writable, .. }` |
| `boa_engine::builtins::promise::ResolvingFunctions` | `js_engine::records::PromiseResolvers<Types>` |
| `JsSymbol::async_iterator()` | `ec.property_key_from_str("@@asyncIterator")` or symbol creation |
| `Context::register_global_property(key, val, attrs)` | `ec.create_data_property(global, key, val)` |

## Boa backend — WPT inventory (2026-07-04)

Default WPT suite: ~97 tests.

### ✅ PASS (tests that were expected PASS and pass)

| Test | Notes |
|---|---|
| `CSS.supports-*` (3 tests) | CSS.supports() works |
| `dom/nodes/Element-hasAttribute` | |
| `dom/nodes/Element-insertAdjacentText` | |
| `dom/nodes/Element-remove` | |
| `dom/nodes/Node-constants` | |
| `html/dom/document.title-01/03/05` | |
| `html/dom/document-dir` | |
| `html/iframe-element/*` (2) | |
| `html/HTMLAnchorElement/*` (2) | |
| `streams/piping/close-propagation-forward` | |
| `streams/piping/error-propagation-forward` | |
| `streams/piping/flow-control` | |
| `streams/piping/general-addition` | |
| `streams/piping/multiple-propagation` | |
| `streams/piping/pipe-through` | |
| `streams/piping/then-interception` | |
| `streams/piping/transform-streams` | |
| `streams/readable-streams/constructor` | |
| `streams/readable-streams/bad-strategies` | |
| `streams/readable-streams/floating-point-total-queue-size` | |
| `streams/readable-streams/garbage-collection` | |
| `streams/readable-byte-streams/construct-byob-request` | |
| `streams/readable-byte-streams/crashtests/tee-locked-stream` | |
| `streams/transform-streams/flush` | |
| `streams/transform-streams/formal-debug-order` | |
| `streams/transform-streams/formal-debug-terminate` | |
| `streams/transform-streams/lipfuzz` | |
| `streams/transform-streams/patched-global` | |
| `streams/transform-streams/properties` | |
| `streams/transform-streams/strategies` | |
| `streams/writable-streams/bad-strategies` | |
| `streams/writable-streams/bad-underlying-sinks` | |
| `streams/writable-streams/byte-length-queuing-strategy` | |
| `streams/writable-streams/count-queuing-strategy` | |
| `streams/writable-streams/error` | |
| `streams/writable-streams/floating-point-total-queue-size` | |
| `streams/writable-streams/properties` | |
| `streams/writable-streams/reentrant-strategy` | |
| `streams/writable-streams/start` | |

### ✅ FIXED (Categories 1 and 2 — PipeTo pump stall)

The PipeTo pump was stalling after the first write because
`process_write_on_fulfilled` called `advance_queue_if_needed` before
`finish_in_flight_write`. Since the in-flight write slot was still occupied,
`advance_queue_if_needed` returned early without starting the next queued
write or close sentinel. Fix: swapped the order so `finish_in_flight_write`
runs first (freeing the in-flight slot), then `advance_queue_if_needed`
picks up the next operation.

Additionally, `write_controller` was computing backpressure *before*
enqueueing, causing `update_backpressure` to use the stale value. Fix:
compute backpressure after enqueueing so it reflects the actual queue state.

**Category 4** (byte stream controller `stream` slot never set): added
`*controller.stream.borrow_mut() = Some(stream)` to
`set_up_readable_byte_stream_controller`.

**Category 5** (Symbol-based iterator lookup): added
`property_key_from_symbol` / `property_key_from_well_known_symbol` to
`ExecutionContext` trait. Updated `get_readable_stream_from_iterator_record`
to use Symbol keys for `@@asyncIterator` and `@@iterator`.

Fixes: `close-propagation-forward`, `error-propagation-forward`, `flow-control`,
`transform-streams`, `flush`, `formal-debug-order`, `lipfuzz`, `strategies`,
`writable-streams/bad-underlying-sinks`, `writable-streams/byte-length-queuing-strategy`,
`writable-streams/count-queuing-strategy`, `writable-streams/floating-point-total-queue-size`,
`writable-streams/reentrant-strategy`.

### ❌ UNEXPECTED FAIL (fix plan)

The console.log crash in microtasks has been **fixed** (`println!` → `writeln!`
with explicit stdout handle).  The PipeTo pump stall (Categories 1 and 2) has been
**fixed**.  Remaining failures are logic errors from the generic migration.  The
plan below lists each category, the root cause hypothesis, and the concrete fix
steps.

---

**Category 3: "TypeError: not a callable function" in basic stream tests
(count-queuing-strategy-integration, general, default-reader partial)**

| Aspect | Detail |
|---|---|
| Symptom | `promise_test` returns an unhandled rejection with this TypeError |
| Investigation | The error "not a callable function" comes from Boa's internal `[[Call]]` operation (`non_existent_call` in `core/engine/src/object/internal_methods/mod.rs`). The `invoke_callback_function` `is_callable` check passes — the stored function IS callable at the time it's stored and at the time it's invoked. The error surfaces as a *rejected Promise*, not a synchronous throw, suggesting it comes from a promise reaction job (microtask) rather than from direct `ec.call()` invocation.<br><br>Key finding: `JsObject::call()` in Boa pushes arguments to the VM stack and uses the VM calling convention, then calls `self.__call__()`. For NativeFunction objects, `__call__` is `native_function_call` which directly invokes the closure. For regular functions, `__call__` is `function_call` which may enter the bytecode interpreter. If the VM is in an unexpected state or the call target was garbage-collected, this could produce the error.<br><br>Most likely root cause: `NativeDataWrapper<T>` in `js_engine/src/boa/engine.rs` has a no-op `Trace` implementation, meaning any `JsObject` references stored inside Rust data via `create_object_with_any` / `create_interface_instance` are invisible to the Boa GC. If the GC runs during a test, these `JsObject` references (e.g. `Callback::object`, promise resolve/reject functions) can be freed. This matches the symptom perfectly — the function object is valid when stored and checked, but becomes invalid when the promise reaction job tries to call it later as a microtask. |
| Fix plan | 1. Make `NativeDataWrapper<T>` properly trace its inner `T` when `T: Trace`. This requires changing `NativeDataWrapper<T>` from storing `Box<dyn std::any::Any>` to storing a type that implements both `Any` and `Trace`, and updating `create_object_with_any` / `with_object_any` / `with_object_any_mut` to use the new type.<br>2. Alternative short-term workaround: ensure the Boa GC does not run during tests by disabling automatic GC collection (`context.gc()` or similar).<br>3. Workaround in `invoke_callback_function`: replace the silent `return Ok(host.value_undefined())` with `return Err(host.new_type_error(...))` for the non-callable case to surface the real error location. |

**Category 4: Byte stream — "ReadableByteStreamController is missing its stream"
(read-min, templated, respond-after-enqueue)**

| Aspect | Detail |
|---|---|
| Symptom | All BYOB read operations fail because the stream's controller slot is `None` (";ReadableStream is missing its controller") |
| Root cause | `set_up_readable_byte_stream_controller` in `readablebytestreamcontroller.rs` took `_stream: ReadableStream` (prefixed with underscore, unused!) and never set the stream's controller slot. Both the controller→stream link AND the stream→controller link were missing. |
| Fix | 1. Added `*controller.stream.borrow_mut() = Some(stream.clone());` to set controller's stream slot.<br>2. Added `stream.set_controller_slot(Some(ReadableStreamController::Byte(...)))` and `stream.set_controller_object_slot(Some(...))` to set stream's controller slot. |
| Remaining | After the controller fix, tests now fail with "TypedArray view is missing its kind" — this is a separate issue in `ec.typed_array_element_type()` returning `None` for TypedArrays. |

**Category 5: Async iterator / from — "requires a default reader"
(async-iterator, from)**

| Aspect | Detail |
|---|---|
| Symptom | `ReadableStream.values()` throws "requires a default reader" or from() throws "requires an async iterable or iterable" |
| Root cause | `from()` looked up `@@asyncIterator` using string key `"asyncIterator"` instead of `Symbol.asyncIterator`. Standard iterables (arrays, Set, Map, generators) only expose `Symbol.iterator` / `Symbol.asyncIterator`, not string properties, so `from()` couldn't find them. |
| Fix | Added `property_key_from_symbol` and `property_key_from_well_known_symbol` methods to `ExecutionContext` trait (Boa + JSC backends). Updated `get_readable_stream_from_iterator_record` to use `ec.property_key_from_well_known_symbol("asyncIterator")` and `ec.property_key_from_well_known_symbol("iterator")`. |
| Remaining | The `values()` error may still occur if `ReadableStream` platform objects are not created via `create_interface_instance`. Verify that `ReadableStream` platform objects are created via `create_interface_instance` everywhere, not via `ec.create_object_with_any`. |

---

**Workflow for each category:**
1. Read the relevant test in `vendor/wpt/streams/` to understand exactly what it asserts.
2. Use the browser extension (`browser_evaluate`) to reproduce the specific assertion in isolation.
3. Add `log::debug!` traces or `error!` in the suspected code path.
4. Run the single failing test via `cargo run --release --no-default-features --features boa,media -- wpt` and capture stderr.
5. Compare the failing code path with the corresponding pre-migration code on `main` (use `git show main:content/src/streams/...`). |

**Other (pre-existing, not migration-related):**
| Test | Failure | Status |
|---|---|---|
| `html/structured-clone/*` | `structuredClone` not implemented; `Blob` undefined; `BorrowError` panic | Pre-existing |
| `wasm/jsapi/*` | WASM global not a Window | Pre-existing |
| `formal/wasm-compile-instantiate` | WASM global not a Window | Pre-existing |

## Known issues — JSC backend

| # | Problem | Root cause | Status |
|---|---|---|---|
| 7 | JSC backend does not compile (220+ errors) | Missing methods on `JscValue`/`JscObject` (`is_undefined`, `downcast_ref`, `downcast_mut`, `as_object`, `display`, `value_null`); `wasmtime::Module` references in non-wasm code not gated | Not started — migration override documents this as expected |



