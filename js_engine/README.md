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

#### ✅ Done (zero `boa_engine::*` imports)

| Area | Coverage |
|---|---|
| **JS bindings** — 28 non-wasm binding files | All `content/src/js/bindings/<domain>/*.rs` converted. Only `host_hooks.rs` (Boa engine builder) and `wasm/` (Boa-only wasmtime) remain, intentionally. |
| **Streams** — 13 domain + 3 binding files | Whole `content/src/streams/` directory, zero `boa_engine`/`boa_gc` imports. Un-gated from `#[cfg(boa_backend)]`. |
| **Web IDL infra** — `webidl/` module | `callback.rs`, `promise.rs`, `bindings/constant.rs`, `mod.rs` all converted. Un-gated from `#[cfg(boa_backend)]`. |
| **Console/CSS namespaces** | Deleted `bindings/console.rs`, `bindings/css.rs`; replaced with generic `console_generic.rs`, `css_generic.rs`. |
| **`buffer_source.rs`, `array_index.rs`** | Converted to generic EC API. |
| **`dom/` helper files** | `abort.rs`, `dispatch.rs`, `platform_objects.rs` — all generic API. |
| **`downcast.rs`** | Generic `try_with_*` helpers merged back (Boa-only `downcast_generic.rs` deleted). |

Both backends compile clean:
```bash
cargo check -p content                            # JSC  → zero errors
cargo check -p content --no-default-features --features boa,media  # Boa → zero errors
```

#### ✅ `EnvironmentSettingsObject` — converted to generic engine

`EnvironmentSettingsObject.engine` field type is now `Engine` (the content-level
type alias, resolved to `BoaContext` on Boa or `JscEngine` on JSC). All
operations go through `ExecutionContext<T>` / `EcmascriptHost<T>` trait methods.
The `context()` and `context_ref()` Boa-specific bridge methods have been
removed. Callers (`html.rs`, `main.rs`, `ui_event_dispatch.rs`) updated to
use `settings.engine.realm_global_object()` instead of
`settings.context().global_object()`.

#### 📋 Remaining `#[cfg(boa_backend)]` gating

Files still behind `#[cfg(boa_backend)]` that must be un-gated:

| Module | Blocking issue |
|---|---|
| `html/global_scope.rs` | Uses `boa_engine::Gc` for per-global caches |
| `html/windowproxy.rs` | Uses `JsProxyBuilder` (public Boa API) — fine to stay |
| `js/downcast.rs` | Domain types export Boa GC types |
| `js/mod.rs` helpers | `builtin_with_captures_*`, `js_result_to_completion` bridge functions |
| `main.rs` | ~21 inline `#[cfg(boa_backend)]` annotations; two message loops |

Will **stay** Boa-only:
- `host_hooks.rs` (Boa engine builder bridge)
- `build_context.rs` (engine instantiation point)
- `wasm/` (requires wasmtime)
- `generic_js_test.rs` (Boa/JSC test sections)

**~8 files still import `boa_engine::*`** (down from ~60).

#### 🎯 Key remaining blockers

1. **`GlobalScope`** — uses `boa_engine::Gc` for per-global caches
2. **Message loop** — `main.rs` has two separate `run_*_message_loop` functions
3. **`WindowProxy`** — uses `JsProxyBuilder` (Boa public API)

#### 📅 Remaining work order

1. **Unify the message loop** — merge `run_boa_message_loop` and `run_jsc_message_loop`
   into one. The `run_content_process` entry point already has no `#[cfg]`.

2. **Final cleanup** — Remove `#[cfg(boa_backend)]` from `js/mod.rs` (`bindings`,
   `downcast`, `platform_objects`, helpers). The only gated items should be:
   `wasm/`, `build_context.rs`, `host_hooks.rs`.

3. **POC tests pass** — 86/86 on Boa remain green.

4. **WPT pass on both backends** — ultimate success criterion.

#### 🧹 Cleanup completed

- Removed dead functions: `with_element_ref`, `with_node_ref`, `with_window_mut`,
  `downcast_window`, `install_document_property_with_object`
- Removed 27 dead `add_constant` calls (constants never applied in generic path)
- Removed dead `ConstantDef` re-export from `webidl/bindings/mod.rs`
- Removed dead re-export `with_element_ref` from `dom/mod.rs`
- Deleted `bindings/console.rs`, `bindings/css.rs`, `downcast_generic.rs`
- Removed unused imports across ~15 files


