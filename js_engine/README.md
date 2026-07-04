# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## End state (achieved)

All content code operates exclusively on the generic API —
`ExecutionContext<T>`, `EcmascriptHost<T>`, `JsTypes`.  This is the
current state of the codebase.

- ✅ Zero `boa_engine::*` imports in non-wasm content.
- ✅ Zero `ec_to_ctx` / `context_as_ec` bridges in non-wasm content.
- ✅ Zero `#[cfg(boa_backend)]` logic switches in non-wasm content —
  except `build_context` (the single engine-instantiation point) and
  `bindings/html/host_hooks.rs` (Boa engine builder).
- ✅ One message loop in `main.rs` — not two.  The loop works with the
  generic engine type; no `#[cfg]` branches.
- ✅ Backend-specific code lives only inside `js_engine/src/{boa,jsc}/`.
- 🔶 WPT passes on Boa; JSC content process is not yet functional.

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

The `content/` crate is now fully generic.  All content code operates
exclusively on the `js_engine` trait API — `ExecutionContext<T>`,
`EcmascriptHost<T>`, `JsTypes`.

### ✅ Completed

| Area | Status |
|---|---|
| `boa_engine::*` imports in non-wasm code | **Zero** — no `use boA_engine` or `use boa_gc` anywhere outside `wasm/` and `host_hooks.rs` |
| `ec_to_ctx` / `context_as_ec` bridges in non-wasm | **Zero** — all 7 bridge calls live in `wasm/namespace.rs` only |
| `#[cfg(boa_backend)]` in non-wasm, non-exception files | **Zero** — all removed |
| `builtin_with_captures*` bridge functions | **Removed** — all 38 call sites replaced with `ec.create_builtin_function_with_captures()` |
| `crate::js::builtin_callback` | **Removed** — replaced with `Callback::from_object(Types::object_from_function(...))` |
| `js_result_to_completion` / `native_error_to_js_value` | **Moved** — now private helpers in `wasm/namespace.rs` |
| `downcast.rs` event_target functions | **Un-gated** — all unconditional; `with_abort_signal_ref` uses `ec.with_object_any` |
| `async_iterable.rs` GC derives | **Un-gated** — uses `#[gc_struct]` + `#[ignore_trace]` |
| `GlobalScope`, `WindowProxy` | **Un-gated** — fully generic, zero `boa_engine::*` imports |
| Streams domain files | **Un-gated** — all 5 files, 38 call sites converted |
| `main.rs` message loops | **Unified** — single `run_content_message_loop`; deleted `run_jsc_message_loop` |

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

The following table summarises the migration patterns.  All are validated in
`content/src/generic_js_test.rs`.

| Boa-specific construct | Generic replacement |
|---|---|
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `JsObject::downcast_ref::<T>()` | `ec.with_object_any(&obj).and_then(|d| d.downcast_ref::<T>())` |
| `boa_gc::GcRefCell::new(val)` | `js_engine::gc::gc_cell_new(val)` |
| `JsObject::from_proto_and_data(proto, data)` | `ec.create_object_with_any(prototype, Box::new(data))` |
| `NativeFunction::from_closure(closure)` | `ec.create_builtin_function(Box::new(behaviour), length, name)` |
| `NativeFunction::from_copy_closure_with_captures(...)` | `ec.create_builtin_function_with_captures(captures, behaviour, length, name)` |
| `FunctionObjectBuilder::new(realm, native).name(n).length(l).build()` | `ec.create_builtin_function(Box::new(behaviour), length, name)` |
| `PropertyDescriptor::builder().value(v).writable(true).build()` | `PropertyDescriptor { value, writable, .. }` |
| `boa_engine::builtins::promise::ResolvingFunctions` | `js_engine::records::PromiseResolvers<Types>` |
| `JsSymbol::async_iterator()` | `ec.property_key_from_str("@@asyncIterator")` or symbol creation |
| `js_string!("foo")` | `ec.property_key_from_str("foo")` or `ec.js_string_from_str("foo")` |
| `Context::register_global_property(key, val, attrs)` | `ec.create_data_property(global, key, val)` |
| `boa_engine::JsResult<T>` | `Completion<T, Types>` |

### End state achieved

- `content/` imports `js_engine` traits and types only — zero direct `boa_engine` or `boa_gc` imports
- The only `#[cfg(boa_backend)]` in non-exception, non-wasm code is **zero**
- `Cargo.toml` features `boa`/`jsc` select which backend `js_engine` compiles
- `content/build.rs` sets `cfg(boa_backend)` / `cfg(jsc_backend)` based on features
- `content/src/js/build_context.rs` has one `#[cfg]` to pick `BoaContext` vs `JscEngine`
- Backend-specific code lives only inside `js_engine/src/{boa,jsc}/` and `content/src/wasm/`


