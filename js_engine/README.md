# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Architecture

### Standards call chain

Every web standard (HTML, Streams, DOM) delegates JS operations through
Web IDL, which in turn calls ECMA-262 abstract operations.  The layering
is:

```
Web spec (Streams, HTML, DOM)
  → Web IDL (invoke a callback function, call a user object's operation)
    → ECMA-262 (§7.1–§7.4, §9.3, §9.6, §27.2)
      → js_engine trait (mirrors the JS spec's public API)
        → Boa / JSC backend (engine-specific impl detail)
```

The `js_engine` crate exposes **only** the ECMA-262 operations that other
standards call into (usually via Web IDL).  This is a mechanical mapping:
read the spec call chain, expose the JS spec operation on the trait,
implement it per engine.  No new abstractions beyond what the JS spec
already defines.

### Two categories of abstraction

**1. Standard: `JsEngine<T>` / `ExecutionContext<T>` mirror ECMA-262 operations**

Web standards already define their behavior in terms of ECMA-262 operations:
`Call`, `Get`, `ToNumber`, `NewPromiseCapability`, `PerformPromiseThen`,
`CreateRealm`, etc.  The traits expose them generically.

**2. Weird: `gc.rs` abstracts engine-specific GC**

GC has no ECMA-262 equivalent.  This module is deliberately the one
engine-specific part of the crate.  The only genuinely tricky part of
making the layer generic.

### Design principle: engine-specific code stays inside the backend

A Web IDL algorithm like "a promise rejected with" does not do anything
Boa-specific or JSC-specific — it calls ECMA-262 abstract operations
(`NewPromiseCapability`, `Call`).  Our implementation must do the same:
call the equivalent operations on the generic `ExecutionContext<T>` trait.
The fact that Boa's `Call` internally requires a `Context` is an
implementation detail of the Boa backend (`js_engine/src/boa/`).  Domain
code and Web IDL helpers must never reach through the trait to the
concrete engine.

Concretely:
- `ec_to_ctx` (cast from `dyn ExecutionContext` back to `&mut Context`),
  `context_as_ec` (cast from `&mut Context` to `&mut dyn ExecutionContext`),
  and `context_as_engine` (cast from `&mut Context` to `&mut BoaContext`)
  are **temporary bridges** living in `js_engine/src/boa/engine.rs` — the
  Boa backend.  They exist only because not all Boa APIs have been
  abstracted through the traits yet.  The end state is **zero** calls to
  these functions anywhere outside the Boa backend.
- Domain code that currently calls `js_engine::boa::ec_to_ctx(ec)` and then
  calls Boa-specific APIs is bypassing the trait.  The right fix is to
  call the equivalent trait method instead, or if one does not exist, to
  add it to the trait and implement it for each backend.
- The goal is that every `.rs` file outside `js_engine/src/boa/` (and
  `js_engine/src/jsc/`) contains **zero** calls to `ec_to_ctx`,
  `context_as_ec`, or `context_as_engine`.

`BoaTypes` is similarly centralized: `content/src/js/mod.rs` defines
`pub(crate) type Types = js_engine::boa::BoaTypes;` — the **only** place
`BoaTypes` is imported in the content crate.  All other files use
`crate::js::Types`.  Switching to JSC means changing one line.

### Three-trait model

The ECMA-262 spec (§9.4) defines an **execution context** as the device
that tracks runtime evaluation — it carries the Realm, the code evaluation
state, the ScriptOrModule, and is pushed/popped from the execution context
stack.  The **running execution context** (§9.4) is the top of this stack;
all implicit ECMA-262 operations (`Call`, `Get`, `ToNumber`, `SameValue`,
`currentRealm`, etc.) reference it through the **surrounding agent**.

The HTML spec (\u00a78.1.3.2) defines a **realm execution context** as the
execution context stored on an environment settings object — it is **the**
stateful JS runtime shared by all scripts in a given realm.  When we
`prepare to run script` (\u00a78.1.4.4) it becomes the top of the JS execution
context stack.  This is what `EnvironmentSettingsObject` owns.

Three traits model the split between factory and runtime:

| Trait | Role | Spec basis |
|---|---|---|
| `JsEngine<T>` | **Stateless factory** — creates realms, built-in functions.  A singleton at the process level: it has no mutable state of its own.  Factory operations only. | `CreateRealm` (§9.3), `CreateBuiltinFunction` (§10.3) |
| `ExecutionContext<T>` | **Stateful runtime** — the realm execution context.  Carries the realm, heap, global object, job queue.  Threaded through every binding function, domain method, and dispatch call.  **This is what `EnvironmentSettingsObject` owns.** | <https://html.spec.whatwg.org/#realm-execution-context> §8.1.3.2 → all of ECMA-262 §7, §9.3, §9.6 |
| `EcmascriptHost<T>` | Subset of `ExecutionContext<T>` covering only Web IDL callback algorithms (`Get`, `IsCallable`, `Call`, `report_exception`, value construction).  A supertrait of `ExecutionContext<T>`. | §3 of Web IDL |

### Concrete realization

`BoaContext` (was `BoaEngine`) wraps `boa_engine::Context` and implements
`ExecutionContext<BoaTypes>`.  It **is** a realm execution context for the
Boa backend.  The `JsEngine<BoaTypes>` impl on the same struct is a
convenience — in a clean split the factory would be a separate stateless
singleton and `BoaContext` would only implement `ExecutionContext<BoaTypes>`.

The plan is to eliminate the `JsEngine<BoaTypes>` impl from `BoaContext`
and make the factory a standalone global.  For now they co-reside on the
same struct because Boa's `Context` serves both roles internally.

### What moves where

**`JsEngine<T>` (stateless factory — a process-level singleton):**
- `create_realm`, `set_realm_global_object`, `set_default_global_bindings`
- `create_builtin_function`
- `evaluate_script`, `evaluate_module`
- `set_host_hooks`
- `allocate_array_buffer`, `allocate_shared_array_buffer`
- `clone_array_buffer`, `detach_array_buffer`

**`ExecutionContext<T>` (stateful runtime — the realm execution context, owned by `EnvironmentSettingsObject`):**
- All of §7.1 Type Conversion (`to_number`, `to_string`, `to_object`, etc.)
- All of §7.2 Testing and Comparison (`is_callable`, `same_value`, etc.)
- All of §7.3 Operations on Objects (`get`, `set`, `call`, `construct`,
  `define_property_or_throw`, `create_data_property`, etc.)
- All of §7.4 Iteration (`get_iterator`, `iterator_step_value`, etc.)
- `current_realm`, `realm_intrinsics`
- `enqueue_job`, `run_jobs`
- Value construction (`value_from_*`, `value_undefined`, `value_null`)
- `promise_resolve`, `new_promise_capability`, `perform_promise_then`
- `report_error`
- Buffer operations (`get_value_from_buffer`, `set_value_in_buffer`, etc.)
- `species_constructor`, `generator_start`

**`EcmascriptHost<T>` (subset of `ExecutionContext<T>`):**
- `get`, `is_callable`, `call`
- `perform_a_microtask_checkpoint`
- `report_exception`
- Value construction (shared with `ExecutionContext<T>`)

### What does NOT get abstracted

| Operation | Reason |
|---|---|
| Native function registration (`NativeFunction`) | Engine-specific API shape — but call sites can use a `native_fn_wrapper` helper to centralize the `context_as_ec` cast |
| Platform object construction | Uses Boa `ObjectInitializer` — needs realm's intrinsics table; passes through EC |
| Proxy creation | Boa's proxy builder not publicly creatable |
| `Context::eval` (script evaluation) | `JsEngine::evaluate_script` exists on the trait but callers use `Context::eval` directly; needs migration |
| `JsValue::to_json(&mut Context)` | Boa-specific JSON serialization; needs a trait method |
| `with_global_scope(&Context, ...)` | Boa GC heap traversal to access `GlobalScope`; needs a trait-level host-data accessor |
| `register_global_property`, `ObjectInitializer::new(ctx)`, `JsArray::from_iter(..., ctx)` | Boa object model construction APIs; need trait equivalents or centralized construction in `build_context` |

These are the blockers to `EnvironmentSettingsObject` owning a purely generic context
instead of `BoaContext`.  None are fundamental — they just aren't done yet.

### Platform object downcast without GC abstraction

`downcast_ref::<T>()` and `downcast_mut::<T>()` on `JsObject` are `&self`
methods — they do **not** require `Context`.  This means binding functions
that only downcast to a domain type and read/write fields can be fully
converted to use `&mut dyn ExecutionContext<T>` without any `ec_to_ctx` cast.

Rather than adding a generic `get_object_data<T>()` to the trait (which hits
Boa's `Ref<T>` GcCell borrow-guard lifetime problem — the guard must outlive
the returned reference), we keep `downcast_ref`/`downcast_mut` as the
retrieval mechanism and replace everything else in the binding function body
with EC trait methods:

| Old (Boa-concrete, needs `ctx`) | New (uses EC trait) |
|---|---|
| `this.as_object()` | `BoaTypes::value_as_object(this)` |
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `e.into_opaque(ctx)` | not needed — `new_type_error` already returns `JsValue` |
| `JsValue::new(n)` / `JsValue::from(...)` | `ec.value_from_number(n)` / `ec.value_from_bool(b)` / etc. |
| `v.to_boolean()` | `ec.to_boolean(v)` |
| `JsValue::undefined()` | `ec.value_undefined()` |

This eliminates `ec_to_ctx` from ~70% of binding function bodies (proven in
`html_media_element.rs`: 28 → 2 calls).  The remaining 30% need `ctx` for
string extraction (`to_std_string_escaped`) or object construction
(`ObjectInitializer`, `JsArray`).

Full GC abstraction (trait-level `get_object_data`) is blocked by Boa's
`GcCell` returning `Ref<T>` guards, not `&T`.  This is resolvable but not
on the critical path for eliminating most `ec_to_ctx` calls.

## Layout

```
src/
  lib.rs        Crate root
  types.rs      JsTypes — language types (§6.1) and object subtypes
  engine.rs     JsEngine<T>, EcmascriptHost<T>, Completion, HostHooks
  enums.rs      Numeric, PreferredType, IntegrityLevel, etc.
  records.rs    IteratorRecord, PromiseCapability, PropertyDescriptor
  gc.rs         Trace, Finalize, GcRootHandle (engine-specific)
  boa/          Boa backend (feature = "boa")
  jsc/          JSC backend (feature = "jsc")
```

## Feature flags

| Feature | Engine | Default |
|---|---|---|
| `boa` | Boa (git dep) | **default** |
| `jsc` | JavaScriptCore (macOS) | opt-in |

Mutually exclusive — only one engine at a time.

```bash
cargo check -p js_engine                          # Boa (default)
cargo check -p js_engine --no-default-features --features jsc  # JSC
```

## Spec documentation convention

Every method on `JsEngine<T>` and `ExecutionContext<T>` has **only** the
spec anchor URL as its doc comment.  Example:
`/// <https://tc39.es/ecma262/#sec-toboolean>`.
No prose, no summaries.  The spec IS the documentation.

Infrastructure traits (`Trace`, `Finalize`, etc.) carry no spec links —
they are not spec-defined operations.

## Design notes

### Why `downcast_ref` on `JsObject` doesn't need `Context`

`JsObject::downcast_ref::<T>()` and `JsObject::downcast_mut::<T>()` are
`&self` methods on the Boa object — they don't take `Context`.  This means
binding functions that only do: (a) value-as-object upcast, (b) downcast to
domain type, (c) read a field from the domain type, (d) return a value via
`ec.value_from_*()` — need zero `ec_to_ctx` casts.  `new_type_error` on
`ExecutionContext<T>` replaces `JsNativeError` for error construction.

This eliminates `ec_to_ctx` from ~70% of typical binding function bodies
(the simple getter/setter pattern).  The remaining ~30% need `ctx` for
string extraction from `JsValue` (`to_std_string_escaped`),
`ObjectInitializer`-based construction, `JsArray::from_iter`, or
`NativeFunction::from_closure` registration.

### Why `value_*` methods are `&mut self`

Boa's API requires `&mut Context` for value construction.  This leaks into
the trait even though constructing `undefined` or `null` is conceptually
pure.  Fixing this requires an engine-side change.

### `EcmascriptHost::get` takes `&str`, `ExecutionContext::get` takes `PropertyKey`

`EcmascriptHost` is the narrow interface Web IDL callback algorithms need.
Web IDL's "get a callback function" steps always use string property names
(e.g. "handleEvent"), so `&str` is sufficient.  `ExecutionContext::get` is
the full ECMA-262 `Get(O, P)` which takes an arbitrary property key.
Both map to the same spec operation (`#[sec-get-o-p]`).

### What does NOT belong on these traits

- **`report_exception`** has no ECMA-262 anchor — it's an HTML concept
  ("report an exception").  It lives on `EcmascriptHost` because Web IDL
  callback algorithms need it.
- **`perform_a_microtask_checkpoint`** is HTML, not ECMA-262.  Same
  rationale.
- **`js_string_from_str`** is pure convenience — no spec equivalent.
  Only needed because `T::JsString` is engine-opaque.
- **`report_error`** (default impl) is a logging convenience, not a
  spec operation.

### `ExecutionContext<T>` utility methods

Added to bridge engine-specific APIs without exposing `Context` to callers:

| Method | Replaces | Spec basis |
|---|---|---|
| `js_string_to_rust_string(&self, &T::JsString) -> String` | `s.to_std_string_escaped()` | Pure operation, no JS execution |
| `to_rust_string(&mut self, T::JsValue) -> Completion<String, T>` | `value.to_string(ctx)?.to_std_string_escaped()` | Combines `ToString` + extraction (default impl) |
| `create_empty_array(&mut self) -> T::JsObject` | `JsArray::new(ctx)?` | <https://tc39.es/ecma262/#sec-arraycreate> |
| `array_push(&mut self, &T::JsObject, T::JsValue) -> Completion<(), T>` | `arr.push(item, ctx)?` | <https://tc39.es/ecma262/#sec-array.prototype.push> |
| `create_plain_object(&mut self, Option<&T::JsObject>) -> T::JsObject` | `ObjectInitializer::new(ctx)` | <https://tc39.es/ecma262/#sec-objectcreate> |
| `object_set_property(&mut self, T::JsObject, &str, T::JsValue) -> Completion<(), T>` | `obj.set(js_string!(key), val, false, ctx)?` | Convenience for `set` with string key (default impl) |

These are implemented in the Boa backend (`js_engine/src/boa/engine.rs`) with
the `unsafe` cast contained internally.  JSC stubs (`todo!()`) are in place.

### `NativeFunction` barrier

`JsEngine::create_builtin_function` takes a closure receiving
`&mut dyn ExecutionContext<T>` — architecturally correct for a generic
layer.  But content code still uses Boa's `FunctionObjectBuilder` +
`NativeFunction::from_fn_ptr` because (a) `create_builtin_function`
requires `T: JsTypesWithRealm` and returns `T::Function`, which
creates type-erasure issues with the current interface registry, and
(b) converting all native function registrations is a large mechanical
change.  This is the P3 problem noted in the migration plan.

## Per-backend details

See module docs for implementation status and quirks:

| Backend | Module | Docs |
|---|---|---|
| Boa | `src/boa/mod.rs` | Hard problems, known quirks |
| JSC | `src/jsc/mod.rs` | FFI coverage, `todo!()` items |
| GC | `src/gc.rs` | The one engine-specific abstraction |

## Migration plan

### Phase 1: Split `ExecutionContext<T>` from `JsEngine<T>`

Move runtime operations into a new `ExecutionContext<T>` trait in
`engine.rs`.  `EcmascriptHost<T>` becomes a supertrait of
`ExecutionContext<T>`.  `BoaContext` implements both.  `EnvironmentSettingsObject`
stores the EC and passes it through domain code.

```rust
pub trait ExecutionContext<T: JsTypes>: EcmascriptHost<T> {
    // §7.1 Type Conversion, §7.2 Testing, §7.3 Object Operations,
    // §7.4 Iteration, §9.3 currentRealm, §9.6 jobs, value construction
}
```

### Phase 2: Make `OperationDef` and `AttributeDef` generic

Change the binding infrastructure so that method/attribute function pointers
receive `&mut dyn ExecutionContext<T>` instead of `&mut boa_engine::Context`.

```rust
pub struct OperationDef<T: JsTypes> {
    pub method: fn(
        &T::JsValue,
        &[T::JsValue],
        &mut dyn ExecutionContext<T>,
    ) -> Completion<T::JsValue, T>,
}
```

`WebIdlInterface::create_platform_object` and `register_interface_spec`
follow suit.  JS binding functions (in `content/src/js/bindings/<domain>/`)
change from `fn(this, args, ctx: &mut Context) -> JsResult<JsValue>` to
`fn(this, args, ec: &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>`.

### Phase 3: Thread `ExecutionContext<T>` through domain code

Every domain method that currently takes `&mut Context` takes
`&mut dyn ExecutionContext<T>` instead.  `EventDispatchHost` loses
`fn context()` — `ExecutionContext<T>` provides everything dispatch needs:
`EcmascriptHost<T>` operations, realm access for `create_interface_instance`,
`global_object()` through `current_realm`.

### Phase 4: Eliminate adapter structs

Remove the three duplicate adapters:
- `ContextEventDispatchHost` in `writablestreamdefaultcontroller.rs`
- `ContextEventDispatchHost` in `event_target.rs`
- `CtxHost` in `strategy.rs`

All dispatch/abort/write-algorithm call sites route through
`EnvironmentSettingsObject` which implements `ExecutionContext<T>` directly.

### Phase 5: GC derives

Make `Callback` generic over `T: JsTypes`. Requires abstracting GC trait
derives (`#[derive(Trace, Finalize)]`) — the one genuinely engine-specific
part.  Strategy: conditional compilation or a `GcBackend` trait.

## Migration status

| Phase | What | Status |
|---|---|---|
| 1. Trait split | `ExecutionContext<T>` split from `JsEngine<T>`. Added `global_object()`, `property_key_from_str()` to EC. EC requires `T: JsTypesWithRealm`. | ✅ |
| 2. Generic bindings | `OperationDef<T>`, `AttributeDef<T>`, `ConstantDef<T>`, `InterfaceDefinition<T>`, `WebIdlInterface<T>`, `WebIdlNamespace<T>` parameterized over `T: JsTypes`. | ✅ |
| 3. EC infrastructure | Host-defined data store (`store_host_any`, `get_host_any`, `remove_host_any`) on EC. `RegistryHost` wrapper for Context storage. `NativeDataWrapper` for any-to-NativeObject bridging. Boa/JSC backends updated. | ✅ |
| 4. Generic registry | `InterfaceRegistry<T: JsTypes>` stores `T::JsObject`. `InterfaceEntry<T>` generic. | ✅ |
| 5. Binding fn migration | All 26 binding files: signatures changed to `&mut dyn ExecutionContext<T>` → `Completion<T::JsValue, T>`. Bodies wrap with `ec_to_ctx` cast → `JsResult` bridge closure. `create_interface_instance` call sites in 14 domain files updated. `create_platform_object` trait updated. `register_interface_spec` takes `E: JsEngine<Ty> + ExecutionContext<Ty>`. | ✅ |
| 5b. BoaEngine → BoaContext | Renamed throughout codebase. `BoaContext` wraps `Context` and implements `ExecutionContext<BoaTypes>` — it is the runtime state (realm, heap, global object), not a "factory engine." `JsEngine<BoaTypes>` methods on it are a convenience. `EnvironmentSettingsObject` owns a `BoaContext`, not an `Engine` alias. `build_boa_engine` → `build_context`. | ✅ |
| 6a. CtxHost removal | `CtxHost` adapters in `strategy.rs` and `readablestreamsupport.rs` removed. `invoke_callback_function` and `call_user_objects_operation` take `&mut dyn EcmascriptHost<BoaTypes>` instead of `&mut impl EcmascriptHost<BoaTypes>`. `SourceMethod::call` and `SizeAlgorithm::size` use `context_as_ec` internally instead of local `CtxHost`. | ✅ |
| 6b. EDS context leak | `EventDispatchHost::context()` replaced with `ec()` returning `&mut dyn ExecutionContext<BoaTypes>`. `host.context()` call sites in dispatch/abort updated. | ✅ |
| 6c. EDS adapter removal | `ContextEventDispatchHost` × 2 removed. Stream objects route dispatch through `EnvironmentSettingsObject` directly. | ❌ |
| 7. Domain threading | Domain methods take `&mut dyn ExecutionContext<T>`. All domain files converted: `window.rs`, `window_or_worker_global_scope.rs`, `windowproxy.rs`, `location.rs`, `html_media_element.rs`, `safe_passing_of_structured_data.rs`, `environment_settings_object.rs`, `conversions.rs`, `namespace.rs`, `async_iterable.rs`. Streams done earlier. Internal helpers in structured-data and async-iterable remain as `&mut Context` (called via `ec_to_ctx` bridge). | ✅ |
| 8. Generic Callback | GC derives abstracted, `Callback<T>` | ❌ |
| 9. JSC parity | Missing JSC methods implemented | ❌ |

## Current state

### What works

- `js_engine` crate has the correct three-trait architecture: `JsEngine<T>` (factory) → `ExecutionContext<T>` (runtime) → `EcmascriptHost<T>` (Web IDL callbacks).
- All binding structs are parameterized over `T: JsTypes`: `OperationDef<T>`, `AttributeDef<T>`, `ConstantDef<T>`, `InterfaceDefinition<T>`.
- All 33 `WebIdlInterface<T>` impls and 1 `WebIdlNamespace<T>` impl are parameterized.
- `InterfaceRegistry<T>` stores engine-native `T::JsObject` types.
- Host-defined data store (`store_host_any`/`get_host_any`/`remove_host_any`) provides type-erased storage on `ExecutionContext<T>`.
- `NativeDataWrapper<T>` bridges arbitrary `'static` data to Boa's `NativeObject` trait for platform object creation.
- All binding function signatures are generic: `fn(&T::JsValue, &[T::JsValue], &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>`.
- `create_interface_instance<Ty, T>(data, ec)` takes `&mut dyn ExecutionContext<Ty>`.
- `register_interface_spec` takes `E: JsEngine<Ty> + ExecutionContext<Ty>`.
- `create_platform_object` takes `ec: &mut dyn ExecutionContext<T>`.
- Web IDL promise reaction helpers (`upon_fulfillment`, `upon_rejection`, `upon_settlement`) implement the \"react\" / \"upon fulfillment\" / \"upon rejection\" algorithms using `create_builtin_function` + `new_promise_capability` + `perform_promise_then` trait methods.  Replaces the `NativeFunction::from_copy_closure_with_captures` closure pattern.  Located in `webidl/promise.rs`.

### What's still Boa-concrete

- **Binding function bodies** (~198 `ec_to_ctx` sites, down from ~437 across 24 files).  The remaining blockers are now solely `NativeFunction::from_closure` registration and initialization-time APIs (`Context::eval`, `with_global_scope`).  String extraction, array construction, and object creation all have `ExecutionContext<T>` trait methods now (`js_string_to_rust_string`, `to_rust_string`, `create_empty_array`, `array_push`, `create_plain_object`, `object_set_property`).
- **Domain code** (HTML, WebAssembly, Web IDL) — public APIs take `ec`, internal helpers bridge via `ec_to_ctx`. `safe_passing_of_structured_data.rs` internal helpers and `async_iterable.rs` internal helpers still take `&mut Context` directly (called from entry points via `ec_to_ctx` bridge). Stream-domain, DOM, event dispatch all fully on `ec`.
- **`EnvironmentSettingsObject` still owns `BoaContext`** — not yet a generic context. Blockers: (1) `Context::eval` called directly instead of `JsEngine::evaluate_script`, (2) `with_global_scope(&Context)` for GC heap traversal, (3) `value.to_json(&mut Context)` for JSON serialization, (4) `register_global_property` for Boa global bindings.  Items 1 and 4 can move into `build_context`; items 2 and 3 need new trait methods.  (`ObjectInitializer::new(ctx)` and `JsArray::from_iter(..., ctx)` were solved by the new `create_plain_object` / `create_empty_array` / `array_push` trait methods.)
- **`Callback`** derives `boa_gc::Trace`/`Finalize` — blocks generic Web IDL callback algorithms.
- **`EventDispatchHost` trait** has `ec()` instead of `context()`, fixing the engine-type leak. The trait itself is still Boa-concrete (not parameterized over `T`), but this is by design — event dispatch is a DOM concept that doesn't need engine genericity.
- **`js_engine::boa::context_as_ec` at `NativeFunction::from_closure` sites** can be centralized with a `native_fn_wrapper` helper (Step C).

### Conversion helpers (`content/src/js/mod.rs`)

Three bridging helpers reduce boilerplate during the JsResult → Completion transition:

- **`js_result_to_completion(result, context)`** — wraps `JsResult<T>` → `Completion<T, Types>` by mapping `JsError` to its opaque `JsValue` form via `context`.
- **`native_error_to_js_value(error, context)`** — converts `JsNativeError` → `JsValue` for use as a `Completion` error value.
- **`completion_to_js_result(result)`** — wraps `Completion<T, Types>` → `JsResult<T>` by mapping the error `JsValue` into a `JsError`.

All three are temporary bridges.  They will be removed once all helpers and domain
code are converted to EC.

### Type alias (`content/src/js/mod.rs`)

```rust
pub(crate) type Types = js_engine::boa::BoaTypes;
```

The **only** place `BoaTypes` is imported in the content crate.  All other files
use `crate::js::Types` in their signatures and impls.  Switching to JSC means
changing this one line.

## Next steps (priority order)

> **⚠️ CONTENT MIGRATION FROZEN.** Steps B–G below are deferred.  No binding
> files, domain files, or adapter structs may be edited until the generic API
> surface is fully designed and exercised in `generic_js_test.rs`.
> See [CURRENT WORKFLOW](#current-workflow--do-not-touch-content-code) above.

### Current step: Build out the generic API surface in `generic_js_test.rs`

The generic API surface is not yet complete.  The following gaps must be
addressed **in the test module first** before touching any content code:

1. **`to_js_string` (ToString abstract op)** — ✅ exercised in `formatLabel` binding function.

2. **Property descriptor operations** — ✅ exercised in `exercise_generic_api`:
   `define_property_or_throw`, `has_property`.

3. **Iterator operations** — ✅ exercised in `exercise_generic_api`:
   `get_iterator`, `iterator_step_value`, `iterator_close`.

4. **Promise operations at the binding level** — ✅ exercised in `delayedTitle`
   binding function (`new_promise_capability` + `call` on resolve).
   `perform_promise_then` is on the trait but not yet exercised (type barrier:
   `PromiseCapability.promise` is `JsValue`, `perform_promise_then` expects
   `T::Promise`).

5. **`create_builtin_function`** — ❌ not yet exercised.  Requires `&mut dyn JsEngine<T>`
   access which `exercise_generic_api` doesn't currently receive.  May need a
   companion function `exercise_engine_api(engine: &mut dyn JsEngine<Types>)`.

6. **`Call` / `Construct` at the binding level** — ✅ exercised in `withCallback`
   binding function and in `exercise_generic_api` (invoking `toArray` on widget).

7. **Error-path patterns** — ✅ `new_type_error` and `new_range_error` exercised
   in binding functions and in `exercise_generic_api`.  The trait has no
   `new_error` method (only `new_type_error` / `new_range_error`).

8. **`call_user_objects_operation` through the trait** — ❌ not yet exercised.
   This is a Web IDL helper (in `content/src/webidl/`) that takes
   `&mut dyn EcmascriptHost<T>`.  Since `ExecutionContext<T>: EcmascriptHost<T>`,
   `ec` already satisfies the bound — but the test module should import and
   directly exercise the helper.

9. **Integration with `create_interface_instance`** — ✅ extended with error-path
   exercise in `exercise_generic_api`.

10. **Setter with type conversion** — ✅ exercised in `set_count` binding function
    (uses `ec.to_number`).

Each gap above gets a concrete test function or exercise path in
`generic_js_test.rs`.  The goal is that the test module exercises **every**
`ExecutionContext<T>` method in a binding-function-adjacent pattern before
we touch any content code.

Once all gaps are addressed and `cargo check -p content` passes, the freeze
is lifted and Steps B–G resume.

---

### Step A: Add `get_object_data` / `get_object_data_mut` to `ExecutionContext<T>` — SUPERSEDED

The practical conversion uses direct `obj.downcast_ref::<T>()` and
`obj.downcast_mut::<T>()` on `JsObject` — both are `&self` methods
that do NOT require `Context`.  This achieves the same goal (zero
unsafe, zero `ec_to_ctx` for simple getters) without adding a new
trait method.  The `get_object_data` approach is deferred as an
optional future clean-up.

### Step B: Convert binding function bodies — IN PROGRESS (~60%)

Replace the `js_engine::boa::ec_to_ctx` + `JsResult` closure bridge pattern with
direct `obj.downcast_ref::<T>()` + `ec.value_from_*()` + `ec.new_type_error()`.

The six new `ExecutionContext<T>` utility methods (`js_string_to_rust_string`,
`to_rust_string`, `create_empty_array`, `array_push`, `create_plain_object`,
`object_set_property`) now cover string extraction and object/array
construction — the two largest categories of remaining `ec_to_ctx` sites.
Binding code can use these directly on `ec`, eliminating the `ec_to_ctx` cast
for those operations entirely.  Actual conversion of callers is a mechanical
`s|ec_to_ctx|→ ec.method()|` replacement.

Each binding function today:
```rust
fn get_id(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<Types>)
    -> Completion<JsValue, Types>
{
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let obj = this.as_object().ok_or_else(|| JsNativeError::typ()...)?;
        let element = obj.downcast_ref::<Element>().ok_or_else(|| JsNativeError::typ()...)?;
        Ok(JsValue::from(JsString::from(element.id())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
```

Becomes:
```rust
fn get_id(this: &Types::JsValue, _: &[Types::JsValue], ec: &mut dyn ExecutionContext<Types>)
    -> Completion<Types::JsValue, Types>
{
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let element = obj.downcast_ref::<Element>()
        .ok_or_else(|| ec.new_type_error("expected Element"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(element.id().as_str())))
}
```

No `unsafe`, no `ec_to_ctx`, no closure bridge for getters/operations that
only read/write domain data.  Remaining `ec_to_ctx` sites (207 in bindings)
are concentrated in string-extraction setters, `ObjectInitializer`
construction, and `NativeFunction::from_closure` registration.

### Step C: Centralize `NativeFunction::from_closure` bridging

The cast functions (`ec_to_ctx`, `context_as_ec`, `context_as_engine`) now
live in `js_engine::boa::` — the Boa backend.  This step adds a
`native_fn_wrapper` helper that absorbs the `js_engine::boa::context_as_ec`
cast at `NativeFunction::from_closure` sites, eliminating the remaining
~200 scattered `context_as_ec` calls in `NativeFunction` closures.

### Step D: Remove remaining `ContextEventDispatchHost` adapters

Two adapters remain, in:
- `writablestreamdefaultcontroller.rs`
- `event_target.rs`

### Step E: GC abstraction for `Callback<T>`

`Callback` currently derives `boa_gc::Trace`/`Finalize`.  Abstract
these behind conditional compilation or a `GcBackend` trait.

### Step F: JSC feature parity

Bring the JSC backend up to parity with Boa — fill in `todo!()` stubs,
implement missing `ExecutionContext<T>` methods, and validate that the
JSC feature flag builds clean.

### Step G: Make `EnvironmentSettingsObject` own a generic context

Blockers to removing the `BoaContext` dependency from settings:
1. **`Context::eval`** — callers use it directly instead of `JsEngine::evaluate_script`.
   Fix: route through the trait method.
2. **`with_global_scope(&Context, ...)`** — Boa GC heap traversal to access
   `GlobalScope`.  Fix: add a host-data accessor to `ExecutionContext<T>`.
3. **`value.to_json(&mut Context)`** — Boa-specific JSON serialization.
   Fix: add `to_json` to `ExecutionContext<T>`.
4. **`ObjectInitializer::new(ctx)`, `register_global_property`, `JsArray::from_iter(..., ctx)`** —
   Boa object construction APIs used in `EnvironmentSettingsObject::new`.
   Fix: move setup into `build_context` so the settings object doesn't need
   `Context` for initialization.

Once these are resolved, `EnvironmentSettingsObject` can own
`Box<dyn ExecutionContext<BoaTypes>>` (or a generic parameter) instead of
`BoaContext`, and `ec_to_ctx` can be deleted entirely.

### Verification

Each sub-task requires `cargo check`.  Full verification (WPT + navigation)
has not yet been validated with all Phase 7 domain files converted.
Resume WPT and navigation verification at the next stable checkpoint.

### End-of-task flow (during this migration)

While working through this migration plan, the end-of-task flow is
simplified to avoid slow or failing verification runs on mid-migration
code:

1. **`cargo check -p content`** — must be clean.
2. **`cargo fmt`** — format any changed files.
3. **Update this README** — file counts, status summaries, next steps.
4. **Suggest a commit message** based on `git diff --stat HEAD`.

Skip `cargo clippy --workspace` (pre-existing errors in paint/embedder
crates hide migration-specific warnings) and skip WPT + navigation
verification (they are expected to fail mid-migration).  Resume full
verification after Step G when `ec_to_ctx` is deleted.

## CURRENT WORKFLOW — DO NOT TOUCH CONTENT CODE

**The content migration is frozen.**  All generic-API design work must happen
inside `content/src/generic_js_test.rs` first.  Do NOT edit any binding files,
domain files, adapter structs, or other content code until the full generic API
is proven through the test module.

### New workflow

The `content/src/generic_js_test.rs` module is a self-contained mini-integration-test
for the generic JS layer.  It defines a toy domain type (`TestWidget`), implements
`WebIdlInterface<Types>` for it with binding functions that use the full generic API,
and an `exercise_generic_api` function that calls every relevant `ExecutionContext<T>` /
`JsTypes` / `EcmascriptHost<T>` method.

**Workflow:**
1. Add new generic API methods to `js_engine` traits.
2. Exercise them in `generic_js_test.rs` — this gives fast compile-time feedback.
3. Once the **entire** generic API surface is designed and proven in the test
   module (covering every remaining bridging gap identified below), begin
   migrating content code.

The test module compiles as part of `cargo check -p content` and catches
borrow-checker issues, type mismatch, and design problems instantly.

### Binding conversion status: ~191 ec_to_ctx in bindings (down from ~437).

Binding file counts:
```
 33 readablestream.rs
 22 location.rs
 21 hyperlink_element_utils.rs
 18 writablestream.rs
 18 element.rs
 12 node.rs
 12 document.rs
 11 window.rs
  7 transformstream.rs
  7 html_iframe_element.rs
  6 html_anchor_element.rs
  4 event_target.rs
  4 abort_signal.rs
  3 html_video_element.rs
  2 wasm/interfaces.rs
  2 strategy.rs
  2 html_media_element.rs
  1 wasm/mod.rs
  1 html_input_element.rs
  1 html_element.rs
  1 ui_event.rs
  1 event.rs
  1 dom_exception.rs
  1 abort_controller.rs
```

Recent changes (this session):
- Added four new binding functions to TestWidget: `set_count` (numeric setter
  exercising `ec.to_number`), `formatLabel` (exercises `ec.to_js_string` in
  binding pattern), `delayedTitle` (promise creation via `new_promise_capability` +
  `call` on resolve), `withCallback` (exercises `ec.call` with user-provided
  callback).
- Added property descriptor ops (`define_property_or_throw`, `has_property`) to
  `exercise_generic_api`.
- Added iterator ops (`get_iterator`, `iterator_step_value`, `iterator_close`)
  to `exercise_generic_api`.
- Added promise ops (`new_promise_capability`, `call` on resolve) to
  `exercise_generic_api`.
- Added `Call` exercise at binding level (invoking `toArray` on widget object)
  to `exercise_generic_api`.
- Extended `create_interface_instance` error-path exercise.
- Fixed borrow-checker issue: all `ec.value_*()` calls must pre-compute args
  before passing to `ec.call()` / `ec.array_push()` / `ec.iterator_close()`
  since `ec: &mut dyn ExecutionContext` can't be borrowed twice.

Gaps resolved this session: 1 (to_js_string pattern), 2 (property descriptors),
3 (iterators), 4 (promises), 6 (Call), 7 (error paths), 9 (create_interface_instance
integration), 10 (numeric setter).

Gaps remaining: 5 (create_builtin_function — needs JsEngine trait access),
8 (call_user_objects_operation — needs WebIDL callback struct integration).

### Key architectural insight

`downcast_ref::<T>()` and `downcast_mut::<T>()` on `JsObject` are `&self`
methods — they do NOT take `Context`.  Simple getter/setter patterns that:
1. Cast `this` to `JsObject` via `BoaTypes::value_as_object(this)`
2. Downcast via `obj.downcast_ref::<DomainType>()`
3. Read/write a field from the domain type
4. Return via `ec.value_from_*()`

Need zero `ec_to_ctx` calls.  `ec.new_type_error(msg)` replaces
`JsNativeError::typ()`.  `ec.to_boolean(v)` replaces `v.to_boolean()`.

### What still needs `ec_to_ctx`

Before the trait methods were added:
1. **String extraction**: `args.first().and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped())` — `to_std_string_escaped` requires `&JsString` → `String` conversion that's Boa-specific (no trait equivalent yet).
2. **Object construction**: `ObjectInitializer::new(ctx)`, `JsArray::from_iter(..., ctx)`
3. **NativeFunction registration**: `NativeFunction::from_closure(...)` (Step C will centralize)

**After adding `js_string_to_rust_string`, `to_rust_string`, `create_empty_array`,
`array_push`, `create_plain_object`, and `object_set_property` to
`ExecutionContext<T>` (this session):**

Items 1 and 2 now have trait equivalents.  The remaining blockers are solely
`NativeFunction` / `FunctionObjectBuilder` construction (Step C) and the
initialization-time APIs (`Context::eval`, `with_global_scope`, `register_global_property`)
that live in `build_context` / `EnvironmentSettingsObject::new` (Step G).

### Conversion patterns for Step B (binding function bodies)

**Before** (bridging through `js_engine::boa::ec_to_ctx`):
```rust
fn get_id(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<Types>)
    -> Completion<JsValue, Types>
{
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let obj = this.as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
        let element = obj.downcast_ref::<Element>()
            .ok_or_else(|| JsNativeError::typ().with_message("expected Element"))?;
        Ok(JsValue::from(JsString::from(element.id())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
```

**After** (no `unsafe`, no `ec_to_ctx`):
```rust
fn get_id(this: &Types::JsValue, _: &[Types::JsValue], ec: &mut dyn ExecutionContext<Types>)
    -> Completion<Types::JsValue, Types>
{
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let element = obj.downcast_ref::<Element>()
        .ok_or_else(|| ec.new_type_error("expected Element"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(element.id().as_str())))
}
```

Key replacements:
| Old (Boa-concrete) | New (generic) |
|---|---|
| `this.as_object()` | `Types::value_as_object(this)` |
| `let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };` | (remove — no longer needed) |
| `(\|\| -> JsResult<...> { ... })() .map_err(...)` | (remove — flat `Completion` return) |
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `obj.downcast_ref::<T>()` | `obj.downcast_ref::<T>()` (already `&self`, no `Context` needed) |
| `obj.downcast_mut::<T>()` | `obj.downcast_mut::<T>()` (already `&self`, no `Context` needed) |
| `JsValue::from(JsString::from(s))` | `ec.value_from_string(ec.js_string_from_str(s))` |
| `JsString::from(s)` | `ec.value_from_string(ec.js_string_from_str(s))` |
| `s.to_std_string_escaped()` | `ec.js_string_to_rust_string(&s)` |
| `v.to_string(ctx)?.to_std_string_escaped()` | `ec.to_rust_string(v)?` |
| `JsArray::new(ctx)?` | `ec.create_empty_array()` |
| `arr.push(item, ctx)?` | `ec.array_push(&arr, item)?` |
| `ObjectInitializer::new(ctx).build()` | `ec.create_plain_object(None)` |
| `obj.set(js_string!(key), val, false, ctx)?` | `ec.object_set_property(obj, key, val)?` |
| `JsValue::new(n)` | `ec.value_from_number(n)` |
| `JsValue::undefined()` | `ec.value_undefined()` |
| `JsValue::null()` | `ec.value_null()` |
| `JsValue::from(bool)` | `ec.value_from_bool(b)` |
| `e.into_opaque(ctx).unwrap_or(value_undefined)` | (not needed — `ec.new_type_error` already returns `JsValue`) |

**After `downcast_ref`, still need `ctx`**: Some sites use `ObjectInitializer`
or other Boa-native helpers.  Those keep `js_engine::boa::ec_to_ctx` temporarily.
