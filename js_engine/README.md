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
- **Both backends compile clean:** `cargo check -p content` (JSC default) and
  `cargo check -p content --no-default-features --features boa,media` (Boa)
  both produce zero errors.

### Current blockers

| Blocked operation | Reason |
|---|---|
| `ObjectInitializer` / `register_global_property` (document property) | Boa object-model construction; needs conversion to `ec.create_plain_object` + `ec.set` pattern. CSS namespace done (`css_generic.rs`). |

### Remaining `#[cfg(boa_backend)]` gating to remove

Every `#[cfg(boa_backend)]` in content must go except:
- `build_context.rs` (the single engine-instantiation point)
- `wasm/` (requires wasmtime, Boa-only)
- `generic_js_test.rs` (Boa/JSC test sections)

Files currently gated behind `#[cfg(boa_backend)]` that must be un-gated:

| Module | Gating | Status |
|---|---|---|
| `webidl/` | module-level `#[cfg(boa_backend)]` | **DONE** — un-gated, zero `boa_engine::*` imports |
| `dom/` | module-level `#[cfg(boa_backend)]` | `event.rs` converted; `abort.rs`, `dispatch.rs`, `ui_event_dispatch.rs` remain |
| `html/` | module-level `#[cfg(boa_backend)]` | `html.rs`, `window_or_worker_global_scope.rs`, `location.rs`, `html_anchor_element.rs`, `window.rs` converted; `environment_settings_object.rs`, `global_scope.rs`, `windowproxy.rs` remain |
| `streams/` | module-level `#[cfg(boa_backend)]` | All 13 stream files remain |
| `js/bindings/` | module-level `#[cfg(boa_backend)]` | ~18 binding files remain (mostly `ObjectInitializer`-based) |
| `js/bindings/wasm/` | module-level `#[cfg(boa_backend)]` | Boa-only (wasmtime); bridge functions moved locally |
| `js/downcast.rs` | module-level `#[cfg(boa_backend)]` | Generic `try_with_*` helpers exist; Boa-specific `with_*` helpers need removal or gating |
| `js/platform_objects.rs` | module-level `#[cfg(boa_backend)]` | Partially generic; uses `boa_engine::{JsValue, JsObject}` directly |
| `js/mod.rs` helpers | function-level `#[cfg(boa_backend)]` | `builtin_with_captures_ctx`, `builtin_with_captures`, `builtin_callback*`, bridge functions |
| `main.rs` | ~21 inline `#[cfg(boa_backend)]` annotations | Many tied to `ContentProcess` which is Boa-only |

**~55 files still import `boa_engine::*`** (down from ~60).

### Converted files (this session)

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

### 2. Convert domain/webidl/bindings modules from `boa_engine::*` to generic

Work module by module.  For each file:

- Replace `use boa_engine::...` with `use js_engine::{ExecutionContext, ...}`
  and `crate::js::Types` aliases.
- Replace `&mut Context` with `&mut dyn ExecutionContext<Types>`.
- Replace Boa-specific API calls with the equivalent EC trait method
  (see replacement table above).
- Un-gate the module from `#[cfg(boa_backend)]`.
- Verify on **both backends** (Boa keeps working, JSC compiles).

Conversion order (lowest-level first, highest-level last):

1. `webidl/` — **DONE** (callback, promise, buffer_source, array_index,
   bindings/constant converted; module un-gated; zero `boa_engine::*` imports)
2. `dom/` — `event.rs` done; `abort.rs`, `dispatch.rs`, `ui_event_dispatch.rs`
   remain (blocked by downcast/platform_objects dependency)
3. `html/` — `html.rs`, `window_or_worker_global_scope.rs`, `location.rs`,
   `html_anchor_element.rs`, `window.rs` done; `environment_settings_object.rs`,
   `global_scope.rs`, `windowproxy.rs` remain
4. `streams/` — all 13 stream files remain (blocked by streams being deeply
   entangled with Boa promise/resolver types)
5. `js/bindings/` — wasm bridges moved locally; ~17 binding files remain
6. `js/downcast.rs`, `js/platform_objects.rs`, `js/mod.rs` helpers —
   generic `try_with_*` variants exist; Boa-specific `with_*` remain

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
