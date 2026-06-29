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
- `ec_to_ctx` (cast from `dyn ExecutionContext` back to `&mut Context`)
  belongs **only** inside `js_engine/src/boa/` — the Boa backend
  implementing the trait methods.
- Domain code that currently says `let context = ec_to_ctx(ec);` and then
  calls Boa-specific APIs is bypassing the trait.  The right fix is to
  call the equivalent trait method instead, or if one does not exist, to
  add it to the trait and implement it for each backend.
- The goal is that every `.rs` file outside `js_engine/src/boa/` (and
  `js_engine/src/jsc/`) contains **zero** calls to `ec_to_ctx`.

### Three-trait model

The ECMA-262 spec (§9.4) defines an **execution context** as the device
that tracks runtime evaluation — it carries the Realm, the code evaluation
state, the ScriptOrModule, and is pushed/popped from the execution context
stack.  The **running execution context** (§9.4) is the top of this stack;
all implicit ECMA-262 operations (`Call`, `Get`, `ToNumber`, `SameValue`,
`currentRealm`, etc.) reference it through the **surrounding agent**.

The HTML spec defines a **realm execution context**
(§8.1.3.3, [realm-execution-context]) as the execution context stored on an
environment settings object.  `prepare to run script` (§8.1.4.4) pushes it
onto the stack; `clean up after running script` pops it.

Three traits model this split:

| Trait | Role | Spec basis |
|---|---|---|
| `JsEngine<T>` | Factory — creates realms, built-in functions, evaluates scripts. Used at initialization only. | `CreateRealm` (§9.3), `CreateBuiltinFunction` (§10.3), `ScriptEvaluation` (§16.1) |
| `ExecutionContext<T>` | Runtime — the running execution context. Provides all operations that implicitly reference the surrounding agent. Threaded through every binding function, domain method, and dispatch call. | Running execution context (§9.4) → all of §7, §9.3 (`currentRealm`), §9.6 (jobs), value construction |
| `EcmascriptHost<T>` | Subset of `ExecutionContext<T>` covering only Web IDL callback algorithms (`Get`, `IsCallable`, `Call`, `report_exception`, value construction). A supertrait of `ExecutionContext<T>`. | §3 of Web IDL |

[realm-execution-context]: https://html.spec.whatwg.org/#realm-execution-context

### What moves where

**`JsEngine<T>` (factory — stays on the engine):**
- `create_realm`, `set_realm_global_object`, `set_default_global_bindings`
- `create_builtin_function`
- `evaluate_script`, `evaluate_module`
- `set_host_hooks`
- `allocate_array_buffer`, `allocate_shared_array_buffer`
- `clone_array_buffer`, `detach_array_buffer`

**`ExecutionContext<T>` (runtime — separate from `JsEngine<T>`):**
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
`ExecutionContext<T>`.  `BoaEngine` implements both.  `EnvironmentSettingsObject`
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

- **Binding function bodies** (~411 sites, down from ~437) use `ec_to_ctx(ec)` to cast back to `&mut Context`.  Key insight: `obj.downcast_ref::<T>()` on `JsObject` does NOT need `Context` — it's `&self`.  Simple getters that downcast + read a field + return via `ec.value_from_*()` need zero bridges.  The conversion pattern proven in `html_media_element.rs` (28→2 `ec_to_ctx` calls).  Remaining bridges are in functions that extract Rust strings from `JsValue` (`to_std_string_escaped`), construct `JsArray`/`ObjectInitializer`, or register `NativeFunction::from_closure`.
- **Domain code** (HTML, WebAssembly, Web IDL) — public APIs take `ec`, internal helpers bridge via `ec_to_ctx`. `safe_passing_of_structured_data.rs` internal helpers and `async_iterable.rs` internal helpers still take `&mut Context` directly (called from entry points via `ec_to_ctx` bridge). Stream-domain, DOM, event dispatch all fully on `ec`.
- **`Callback`** derives `boa_gc::Trace`/`Finalize` — blocks generic Web IDL callback algorithms.
- **`EventDispatchHost` trait** has `ec()` instead of `context()`, fixing the engine-type leak. The trait itself is still Boa-concrete (not parameterized over `T`), but this is by design — event dispatch is a DOM concept that doesn't need engine genericity.
- **`context_as_ec` at `NativeFunction::from_closure` sites** can be centralized with a `native_fn_wrapper` helper that absorbs the cast: `NativeFunction::from_closure(Box::new(move |this, args, ctx| { let ec = context_as_ec(ctx); f(this, args, ec) }))`.

### Conversion helpers (`content/src/js/mod.rs`)

Three bridging helpers reduce boilerplate during the JsResult → Completion transition:

- **`js_result_to_completion(result, context)`** — wraps `JsResult<T>` → `Completion<T, BoaTypes>` by mapping `JsError` to its opaque `JsValue` form via `context`.
- **`native_error_to_js_value(error, context)`** — converts `JsNativeError` → `JsValue` for use as a `Completion` error value.

`completion_to_js_result` is used at unconverted call sites that call `Completion`-returning
helpers from `JsResult`-returning functions. `js_result_to_completion` is used in the
reverse direction. Both will be removed once all helpers and domain code are converted to EC.

## Next steps (priority order)

### Step A: Add `get_object_data` / `get_object_data_mut` to `ExecutionContext<T>`

Add generic platform object data retrieval to the trait.  The Boa
implementation goes through `NativeDataWrapper` → `downcast_ref` →
inner `dyn Any` downcast.  The JSC implementation is stubbed.

This unblocks removing `ec_to_ctx` + `obj.downcast_ref::<T>()` from
binding function bodies, because `get_object_data` + `new_type_error`
(already on the trait) are the only two Boa-specific operations in the
hot path of every binding function.

### Step B: Convert binding function bodies to use `get_object_data` + `new_type_error`

Each binding function today:
```rust
fn get_id(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>)
    -> Completion<JsValue, BoaTypes>
{
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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
fn get_id(this: &BoaTypes::JsValue, _: &[BoaTypes::JsValue], ec: &mut dyn ExecutionContext<BoaTypes>)
    -> Completion<BoaTypes::JsValue, BoaTypes>
{
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let element = ec.get_object_data::<Element>(&obj)
        .ok_or_else(|| ec.new_type_error("expected Element"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(element.id().as_str())))
}
```

No `unsafe`, no `ec_to_ctx`, no closure bridge.  The 437 `ec_to_ctx`
calls drop to near zero (some remain for `ObjectInitializer`-based
construction until platform object creation is generic).

### Step C: Centralize `context_as_ec` with a `NativeFunction` wrapper

Create a helper that wraps `NativeFunction::from_closure` and absorbs
the `context_as_ec` cast.  Eliminates ~200 scattered cast sites.

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

### Verification

Each sub-task requires `cargo check`.  Full verification (WPT + navigation)
has not yet been validated with all Phase 7 domain files converted.
Resume WPT and navigation verification at the next stable checkpoint.

## Session-resume guide

**Current step: Step B — convert binding function bodies.**

### Status: Step A complete. Step B in progress.

`html_media_element.rs` converted as proof-of-concept: 28→2 `ec_to_ctx`
calls.  The two remaining are `set_src` and `set_preload` which extract
Rust strings from `JsValue` (requires `Context::to_std_string_escaped`).

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

1. **String extraction**: `args.first().and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped())` — `to_std_string_escaped` requires `&JsString` → `String` conversion that's Boa-specific (no trait equivalent yet).
2. **Object construction**: `ObjectInitializer::new(ctx)`, `JsArray::from_iter(..., ctx)`
3. **NativeFunction registration**: `NativeFunction::from_closure(...)` (but can be centralized with wrapper)

### Conversion patterns for Step B (binding function bodies)

**Before** (bridging through `ec_to_ctx`):
```rust
fn get_id(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>)
    -> Completion<JsValue, BoaTypes>
{
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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

**After** (no `ec_to_ctx`, fully generic):
```rust
fn get_id(this: &BoaTypes::JsValue, _: &[BoaTypes::JsValue], ec: &mut dyn ExecutionContext<BoaTypes>)
    -> Completion<BoaTypes::JsValue, BoaTypes>
{
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let element = ec.get_object_data::<Element>(&obj)
        .ok_or_else(|| ec.new_type_error("expected Element"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(element.id().as_str())))
}
```

Key replacements:
| Old (Boa-concrete) | New (generic) |
|---|---|
| `this.as_object()` | `BoaTypes::value_as_object(this)` |
| `let ctx = ec_to_ctx(ec)` | (remove — no longer needed) |
| `(\|\| -> JsResult<...> { ... })() .map_err(...)` | (remove — flat `Completion` return) |
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `obj.downcast_ref::<T>()` | `ec.get_object_data::<T>(&obj)` |
| `obj.downcast_mut::<T>()` | `ec.get_object_data_mut::<T>(&obj)` |
| `JsValue::from(JsString::from(s))` | `ec.value_from_string(ec.js_string_from_str(s))` |
| `JsValue::new(n)` | `ec.value_from_f64(n)` or `ec.value_from_number(n)` |
| `JsValue::undefined()` | `ec.value_undefined()` |
| `JsValue::null()` | `ec.value_null()` |
| `JsValue::from(bool)` | `ec.value_from_bool(b)` |
| `e.into_opaque(ctx).unwrap_or(value_undefined)` | (not needed — `ec.new_type_error` already returns `JsValue`) |

**`&self` domain methods**: When calling a domain method that takes `&self`
(read-only), you can pass `&*element` after dereferencing the `&T` from
`get_object_data`.  Methods that need `&mut self` use `get_object_data_mut`.

**After `get_object_data`, still need `ctx`**: Some sites use `ObjectInitializer`
or other Boa-native helpers.  Those keep `ec_to_ctx` temporarily.

**Verification**: `cargo check -p content` after each file group.
