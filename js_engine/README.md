# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## End state

All content code operates exclusively on the generic API —
`ExecutionContext<T>`, `EcmascriptHost<T>`, `JsTypes`.

- Zero `boa_engine::*` imports in content.
- Zero `ec_to_ctx` / `context_as_ec` bridges in content.
- Zero `#[cfg(boa_backend)]` logic switches in content — except `build_context`
  (the single engine-instantiation point) and `wasm/` (requires wasmtime,
  Boa-only).
- One message loop in `main.rs` — not two.  The loop works with the generic
  engine type; no `#[cfg]` branches.
- Backend-specific code lives only inside `js_engine/src/{boa,jsc}/`.
- **WPT tests pass with zero unexpected results on both backends.**

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

## Generic API surface (POC: 86/86 tests pass on Boa)

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

## Replacement table (old Boa API → generic EC trait)

| Boa-specific | Generic EC trait method |
|---|---|
| `JsPromise::new_pending(context)` | `ec.new_promise_pending()?` |
| `JsPromise::from_object(p)?.then(...)` | `ec.perform_promise_then(...)` |
| `JsPromise::from_object(x)?.state()` | `ec.promise_state(&x)?` |
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `JsValue::undefined()` | `ec.value_undefined()` |
| `NativeFunction::from_copy_closure_with_captures(...)` | `ec.create_builtin_function(...)` |
| `resolvers.resolve.call(&u, &[v], ctx)` | `ec.call(&resolve, &undefined, &[v])` |
| `object.get(js_string!(key), context)` | `ec.get(object, key)?` |
| `JsUint8Array::from_iter(src, ctx)` | `typed_array_buffer` + `clone_array_buffer` + `construct` |
| `ObjectInitializer::new(context)` / `register_global_property(...)` | `ec.create_plain_object(...)` + `ec.set(global, key, val, ...)` |

## Per-backend details

| Backend | Status |
|---|---|
| Boa | ✅ Full parity — all trait methods, all POC tests pass |
| JSC | 🔶 Trait surface complete. 1 ignore: `SharedArrayBuffer`. `exercise_context_lifecycle` is Boa-only. |
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

## Migration status

### Done

- `_ec` suffix functions: **zero remaining.**
- `completion_to_js_result` bridges: **eliminated.**
- `evaluate_script` on `ExecutionContext<T>` (Boa + JSC).
- Generic console namespace installer (`console_generic.rs`) — uses only
  EC trait methods.
- Structured clone (`safe_passing_of_structured_data.rs`) — fully generic,
  zero Boa imports.
- `#[gc_struct]` — backend-specific proc-macro variants, `#[ignore_trace]`
  attribute.
- Build system: `content/build.rs` sets `boa_backend`/`jsc_backend` cfg flags;
  `content/Cargo.toml` uses feature flags (not target-specific deps).
- JSC backend: `JSValueIsDate`, `object_is_regexp`/`object_is_error` via
  `Object.prototype.toString.call(this)` eval fallback.
- Wasm gated behind `#[cfg(boa_backend)]`.
- **All `#[derive(Clone, Trace, Finalize)]` patterns converted to `#[gc_struct]`**
  across Boa-only gated modules: `dom/abort.rs`, `html/global_scope.rs`,
  `streams/*.rs`, `webidl/async_iterable.rs`.  This eliminates all direct
  `boa_gc::Trace`/`boa_gc::Finalize` derive imports from content code.
- **Proc-macro fix:** `#[gc_struct]` now correctly transforms `#[ignore_trace]`
  in enum variant fields (both `gc_struct_boa` and `gc_struct_jsc`).
- **Generic CSS namespace** (`css_generic.rs`) following `console_generic.rs`
  pattern (`create_plain_object` + `Behaviour` trait), wired into JSC path.
- **`buffer_source.rs`, `array_index.rs` converted to generic EC API** —
  no `boa_engine::*` imports.
- **`promise.rs` cleaned up:** removed dead `_boa` suffix functions; converted
  `rejected_promise_from_error`/`error_to_rejection_reason` to take `JsValue`;
  converted all function signatures to generic `ExecutionContext<Types>`.
- **Console/css namespace switch** — both backends now use the generic
  `console_generic.rs` and `css_generic.rs` implementations (deleted
  `content/src/js/bindings/console.rs` and `content/src/js/bindings/css.rs`).
  Zero `boa_engine::*` imports in console/css namespaces.
- **All 13 stream domain files converted** to generic EC API — `readablestream.rs` and
  `readablebytestreamcontroller.rs` no longer use any `boa_engine::*` imports.
  Zero `boa_engine` or `boa_gc` imports in the entire `streams/` directory.
- **All 3 stream binding files converted** (`readablestream.rs`, `strategy.rs`,
  `writablestream.rs`).
- **`environment_settings_object.rs` call sites updated** — console/CSS
  installer calls now pass `&mut engine` (generic `ExecutionContext`) instead
  of `engine.context()` (Boa-specific).
- **Both backends compile clean:** `cargo check -p content` (JSC default) and
  `cargo check -p content --no-default-features --features boa,media` (Boa)
  both produce zero errors.
- **9 JS binding files converted** to generic API (zero `boa_engine::*` imports):
  `dom/abort_controller.rs`, `dom/dom_exception.rs`, `dom/event.rs`,
  `dom/ui_event.rs`, `html/html_input_element.rs`, `html/html_anchor_element.rs`,
  `html/html_media_element.rs`, `html/html_video_element.rs`, `html/location.rs`.
  All use `<Types as JsTypes>::JsValue` signatures, `ec.new_type_error()` errors,
  and pre-computed fallback values for borrow-safe closures.

### Current blockers

| Blocked operation | Reason |
|---|---|
| ~~`readablestream.rs` (domain)~~ | ✅ **RESOLVED** — all stream files fully converted, zero `boa_engine::*` imports. |
| ~~`readablebytestreamcontroller.rs` (domain)~~ | ✅ **RESOLVED** — all stream files fully converted, zero `boa_engine::*` imports. |
| `downcast.rs` cfg removal | Still gated because domain types depend on `boa_engine` |
| `js/mod.rs` cfg removal | Helper functions (`builtin_with_captures_*`, `js_result_to_completion`) still use `boa_engine` types |
| `main.rs` message loop unification | Two separate `run_*_message_loop` functions — needs merge into single generic loop |
| `ui_event_dispatch.rs` | Depends on `EnvironmentSettingsObject` conversion |
| `environment_settings_object.rs` core bridge | Still stores `BoaContext` directly — needs generic engine pointer |
| `global_scope.rs` | Uses `boa_engine::Gc` for per-global caches |
| `windowproxy.rs` | Uses `JsProxyBuilder` (public Boa API) — fine to stay Boa-specific |

### Remaining `#[cfg(boa_backend)]` gating to remove

Every `#[cfg(boa_backend)]` in content must go except:
- `build_context.rs` (the single engine-instantiation point)
- `wasm/` (requires wasmtime, Boa-only)
- `generic_js_test.rs` (Boa/JSC test sections)

Files currently gated behind `#[cfg(boa_backend)]` that must be un-gated:

| Module | Gating | Status |
|---|---|---|
| `webidl/` | module-level `#[cfg(boa_backend)]` | **DONE** — un-gated, zero `boa_engine::*` imports |
| `dom/` | module-level `#[cfg(boa_backend)]` | `event.rs` + `abort.rs` + `dispatch.rs` converted; `ui_event_dispatch.rs` remains (depends on `EnvironmentSettingsObject`) |
| `html/` | module-level `#[cfg(boa_backend)]` | `html_anchor_element.rs`, `html_element.rs`, `html_input_element.rs`, `html_media_element.rs`, `hyperlink_element_utils.rs`, `location.rs` are generic. `environment_settings_object.rs`, `global_scope.rs`, `windowproxy.rs` remain (core engine bridge) |
| `streams/` | module-level `#[cfg(boa_backend)]` | **DONE** — all 13 files un-gated, zero `boa_engine::*` imports. |
| `js/bindings/` | module-level `#[cfg(boa_backend)]` | **9 files converted this session.** ~9 remaining: `element.rs`, `abort_signal.rs`, `event_target.rs`, `node.rs`, `document.rs`, `html_element.rs`, `html_iframe_element.rs`, `hyperlink_element_utils.rs`, `window.rs`. |
| `js/bindings/wasm/` | module-level `#[cfg(boa_backend)]` | Boa-only (wasmtime); bridge functions moved locally |
| `js/downcast.rs` | module-level `#[cfg(boa_backend)]` | Generic `try_with_*` helpers (using `<crate::js::Types as JsTypes>` syntax) merged back into single file. `downcast_generic.rs` removed. Remains Boa-gated because `crate::dom`/`crate::html` are Boa-only. |
| `js/platform_objects.rs` | module-level `#[cfg(boa_backend)]` | Fully converted to generic `<crate::js::Types as JsTypes>::JsObject`; no `boa_engine::*` imports. Remains Boa-gated because domain types still depend on `boa_engine`. |
| `js/mod.rs` helpers | function-level `#[cfg(boa_backend)]` | `builtin_with_captures_ctx`, `builtin_with_captures`, `builtin_callback*`, bridge functions |
| `main.rs` | ~21 inline `#[cfg(boa_backend)]` annotations | Many tied to `ContentProcess` which is Boa-only |

**~35 files still import `boa_engine::*`** (down from ~60).  Files no longer importing `boa_engine`:
- Earlier sessions: `platform_objects.rs`, `dom/abort.rs`, `dom/dispatch.rs`,
  `html/html_media_element.rs`, `dom/event.rs`, `streams/strategy.rs`
- **This session (9 files):** `dom/abort_controller.rs`, `dom/dom_exception.rs`,
  `dom/event.rs`, `dom/ui_event.rs`, `html/location.rs`,
  `html/html_input_element.rs`, `html/html_anchor_element.rs`,
  `html/html_media_element.rs`, `html/html_video_element.rs`
- **Streams (2 files):** `readablestream.rs`, `readablebytestreamcontroller.rs`
- `bindings/console.rs` and `bindings/css.rs` deleted (replaced by generic versions).

### Converted files (previous sessions — keep)

- **`js_engine` foundation:** Added `PartialEq` to `JsTypes::JsObject` bound; implemented `PartialEq + Eq` for `JscObject`.
- **`webidl/callback.rs`:** Converted from `boa_engine::{JsValue, JsObject}` to generic `Types::JsValue`/`JsObject`; replaced `JsObject::equals` with `PartialEq`.
- **`webidl/bindings/constant.rs`:** Converted to generic `ExecutionContext`-based API.
- **`webidl/promise.rs`:** Removed dead `a_new_promise_boa` and `rejected_promise_from_error_boa` bridge functions.
- **`webidl/mod.rs`:** Removed dead re-exports.
- **`dom/event.rs`:** Converted from `boa_engine::JsObject` to generic type alias.
- **`html.rs` (root):** Converted from `boa_engine::{Context, JsResult, JsValue, JsObject}` to generic type aliases.
- **`html/window_or_worker_global_scope.rs`:** Converted `JsValue` to generic alias.
- **`html/location.rs`:** Converted `JsObject` to generic alias.
- **`html/html_anchor_element.rs`:** Converted `JsObject` to generic alias.
- **`html/window.rs`:** Converted `JsValue` to generic alias; replaced `JsValue::null()` with `ec.value_null()`.
- **`wasm/namespace.rs`, `js/bindings/wasm/mod.rs`:** Moved `a_new_promise_boa`/`rejected_promise_from_error_boa` bridge functions locally into the Boa-only wasm module.
- **Un-gated `webidl` module** in `main.rs` (removed `#[cfg(boa_backend)]`).
- **Split `downcast.rs` into `downcast_generic.rs` + `downcast.rs`** (temporary, now merged back) — generic `try_with_*` functions separated from Boa-specific `with_*` functions.
- **Converted `platform_objects.rs`** — replaced `boa_engine::{JsValue, object::JsObject}` with `<crate::js::Types as JsTypes>::JsObject`/`::JsValue`. No `boa_engine::*` imports remain.
- **Converted `dom/abort.rs`** — replaced `boa_engine::{JsValue, JsObject, JsResult, JsNativeError, JsError}` and `boa_gc::Gc` with generic equivalents (`<Types as JsTypes>::JsValue/JsObject`, `gc_cell_ptr_eq`, `ec.value_undefined()`). Added `gc_cell_ptr_eq` to `js_engine::gc`. Gated `ReadableStreamPipeTo` variant and `EventDispatchHost`-dependent functions behind `#[cfg(boa_backend)]`.
- **Converted `dom/dispatch.rs`** — replaced all `JsValue::from(event.clone())` with `<Types as JsTypes>::value_from_object(event.clone())`, `&JsObject` with `&<Types as JsTypes>::JsObject`, and `object.downcast_ref::<T>()` with `ec.with_object_any(object).and_then(|d| d.downcast_ref::<T>())`. Added `ec` parameter to `debug_target_label`.
- **Converted `html/html_media_element.rs`** — replaced `boa_engine::JsValue` and `JsValue::undefined()` with `ec.value_undefined()`.
- **Merged `downcast_generic.rs` back into `downcast.rs`** — `downcast_generic.rs` was temporary; now deleted.

### Converted files (this session — stream layer + cleanup)

#### Stream domain files (13 files — fully migrated)

All 13 stream domain files are now converted to the generic EC API with zero
`boa_engine::*` imports.  Key pattern:

```rust
use crate::js::Types;
type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
type ArrayBuffer = <Types as JsTypes>::ArrayBuffer;
```

Files converted (in this session and prior): all writable stream files
(`writablestream*.rs`), all readable stream files (`readablestream.rs`,
`readablestreamsupport.rs`, `readablestreamdefault*.rs`,
`readablestreamasynciterator.rs`, `readablestreambyobreader.rs`,
`readablebytestreamcontroller.rs`), `transformstream.rs`, `strategy.rs`, and
stream binding files (`readablestream.rs`, `strategy.rs`, `writablestream.rs`).

#### Module gating

- **`dom/mod.rs`:** Removed `#[cfg(boa_backend)]` from `dispatch`, `ui_event_dispatch`,
  `signal_abort`, `dispatch_*`, `fire_event` — these are now compiled unconditionally.
- **`streams/mod.rs`:** Removed `#[cfg(boa_backend)]` from all stream submodules
  — `readablebytestreamcontroller` and `readablestream` are now un-gated.
- **`js/mod.rs`:** Removed `#[cfg(not(boa_backend))]` from `console_generic` and
  `css_generic` — these are now always compiled.
- **`js/bindings/mod.rs`:** Removed dead `console`/`css` module references.
  Deleted `bindings/console.rs` and `bindings/css.rs` (superseded by generic versions).

#### Cleanup

- Fixed `html_media_element.rs` compilation error: closure captured outer `ec`
  instead of using the parameter `job_ec`.
- Removed unused imports across ~15 files.
- Removed dead code variables (`value_undefined`, `prevent_key`, `receiver`, etc.).
- Both backends compile with zero errors.

### Two message loops → one

`main.rs` currently has `run_boa_message_loop` and `run_jsc_message_loop`
as separate functions selected by `#[cfg]`.  These must be unified into
a single loop that works with `Engine` (the content-level type alias for
`BoaContext` or `JscEngine`).  No `#[cfg]` on the loop itself.

The `run_content_process` entry point already has no `#[cfg]` — the
engine-selection happens inside `build_context`.  The message loop just
needs to use that `Engine` value directly instead of branching.

## Remaining work order

### 1. Port CSS namespace to generic EC API ✅

Follow the `console_generic.rs` pattern: `create_plain_object` +
`create_builtin_function` + `set`.  Move the old `bindings/css.rs`
(which uses `ObjectInitializer` + `register_global_property`) to
Boa-only, gated.  This clears the last blocker in the "not yet
abstracted" table.

### 2. Convert remaining JS binding files (~9 remaining)

Converted 2 this session: `html/html_iframe_element.rs`, `dom/abort_signal.rs`.

These files still import `boa_engine::*` and need the same conversion
patterns as the 11 already done:

- **Medium** (use `downcast_ref` on JsObject — works on Boa but needs
  `ec.with_object_any` for JSC): `dom/element.rs`, `html/html_element.rs`.
- **Complex** (use `Context` directly for prototype registration):
  `dom/event_target.rs`, `dom/node.rs`, `dom/document.rs`,
  `html/window.rs`, `html/hyperlink_element_utils.rs`.

### 3. Unify the message loop

After all modules are generic and un-gated, merge `run_boa_message_loop`
and `run_jsc_message_loop` into one loop.  Remove all `#[cfg(boa_backend)]`
from `main.rs` except for wasm-related code.

### 4. Final cleanup

- Remove `#[cfg(boa_backend)]` from `js/mod.rs` — `bindings`, `downcast`,
  `platform_objects`, and all helper functions should be unconditionally
  compiled.
- The only gated items in content: `wasm/` and the internal `#[cfg]` in
  `build_context.rs`.
- `cargo check -p content` passes on both Boa and JSC backends with zero
  errors.
- POC tests (86/86 on Boa) remain green.
- **WPT tests pass with zero unexpected results on both backends.**
  This is the success criterion for the entire migration.

## End-of-task checklist

- Make sure everything compiles wiht every feature flag. 
- Run step 9 of the top AGENTS.md end of task steps.
- suggest a commit message.
