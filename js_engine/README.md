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
| GC heap traversal (`downcast_ref`) | Engine-specific — no ECMA-262 equivalent |
| Native function registration (`NativeFunction`) | Engine-specific API shape |
| Platform object construction | Uses Boa `ObjectInitializer` — needs realm's intrinsics table; passes through EC |
| Proxy creation | Boa's proxy builder not publicly creatable |

These are handled by `#[repr(transparent)]` casts in the `CreateBuiltinFunction`
shim (see `boa/engine.rs` module docs).

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
| 7. Domain threading | Domain methods take `&mut dyn ExecutionContext<T>` instead of `&mut Context`. Promise helpers, buffer_source, dispatch/abort code use EC trait methods. | 🔄 (abort helpers: `create_abort_signal`, `initialize_dependent_abort_signal`; `writablestreamdefaultwriter.rs` converted) |
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

### What's still Boa-concrete

- **Binding function bodies** use `ec_to_ctx(ec)` to cast back to `&mut Context` for Boa-specific operations (`JsObject::get`, `JsValue::to_number`, `JsNativeError::into_opaque`, etc.). The bodies are in the new signature but internally bridge to Boa.
- **Domain code** (streams, DOM, HTML, WebAssembly) still takes `&mut Context` directly — hasn't been threaded with `ExecutionContext<T>` yet.
- **`Callback`** derives `boa_gc::Trace`/`Finalize` — blocks generic Web IDL callback algorithms.
- **`promise.rs`** helpers are now converted to take `ec: &mut dyn ExecutionContext<BoaTypes>` and return `Completion<_, BoaTypes>`. Callers use `context_as_ec(context)` bridge until their files are converted to EC.
- **`buffer_source.rs`** uses `JsArrayBuffer::from_object`, `JsTypedArray::from_object` directly.
- **`EventDispatchHost` trait** has `ec()` instead of `context()`, fixing the engine-type leak. The trait itself is still Boa-concrete (not parameterized over `T`), but this is by design — event dispatch is a DOM concept that doesn't need engine genericity.

### Conversion helpers (`content/src/js/mod.rs`)

Three bridging helpers reduce boilerplate during the JsResult → Completion transition:

- **`js_result_to_completion(result, context)`** — wraps `JsResult<T>` → `Completion<T, BoaTypes>` by mapping `JsError` to its opaque `JsValue` form via `context`.
- **`native_error_to_js_value(error, context)`** — converts `JsNativeError` → `JsValue` for use as a `Completion` error value.

`completion_to_js_result` is used at unconverted call sites that call `Completion`-returning
helpers from `JsResult`-returning functions. `js_result_to_completion` is used in the
reverse direction. Both will be removed once all helpers and domain code are converted to EC.

## Next steps (priority order)

### Step 1: Thread `ExecutionContext<T>` through domain code

**Pattern established**: domain entry points take `&mut dyn ExecutionContext<BoaTypes>`
and return `Completion<_, BoaTypes>`. Binding functions pass `ec` directly without
`ec_to_ctx`. Domain-internal callers bridge via `context_as_ec(context)` until they're
converted.

**Completed**:
- `dom/abort.rs`: `create_abort_signal`, `initialize_dependent_abort_signal`
- `streams/writablestreamdefaultwriter.rs`: all 19 functions + constructors
- `streams/strategy.rs`: already uses EC trait methods

**Recommended order** (smallest/most self-contained first):

1. **Foundational helpers** — convert these first to eliminate the most bridge boilerplate:
   - ~~`webidl/promise.rs`~~ — ✅ 9 functions + all ~40 call sites converted. Callers use
     `context_as_ec(context)` bridge; already-converted domain files pass `ec` directly.
   - `webidl/buffer_source.rs` — 2 functions, ~6 call sites (all in WASM binding files
     that already have `ec`).

2. **Streams** — convert the remaining stream files:
   - `writablestreamdefaultcontroller.rs` (~20 functions)
   - `readablestreamdefaultreader.rs` (~14 functions)
   - `readablestreambyobreader.rs` (~10 functions)
   - `readablestreamdefaultcontroller.rs` (largest: ~40 instances)
   - `writablestream.rs` (~25 functions)
   - `readablestream.rs` (largest: ~80 instances)
   - `transformstream.rs` (~20 functions)
   - `readablestreamsupport.rs` / `writablestreamsupport.rs`
   - `readablebytestreamcontroller.rs`
   - `readablestreamasynciterator.rs`

3. **DOM**: `dispatch.rs`, `event.rs` (few Context-taking functions)

4. **HTML**: `window.rs`, `location.rs`, `html_media_element.rs`, `environment_settings_object.rs`,
   `window_or_worker_global_scope.rs`, `windowproxy.rs`, `safe_passing_of_structured_data.rs`

5. **WebAssembly**: `namespace.rs`, `conversions.rs`

6. **Web IDL**: `async_iterable.rs`

Each conversion follows the same mechanical pattern:
1. Change signature from `&mut Context` → `&mut dyn ExecutionContext<BoaTypes>`
2. Return `Completion<_, BoaTypes>` instead of `JsResult<_>`
3. At the top of the function body, add `let context = unsafe { crate::js::ec_to_ctx(ec) };`
   for calls to helpers that still take `&mut Context`.
4. Wrap `JsResult`-returning helper calls with
   `js_result_to_completion(call(context), context)?`
   or the equivalent `.map_err(|e| e.into_opaque(context).unwrap_or_else(|_| JsValue::undefined()))?`.
5. Wrap `JsNativeError` creation with `native_error_to_js_value(...)` or the
   equivalent `into()` + `into_opaque(context).unwrap_or_else(...)`.
6. Update callers:
   - Binding function callers: pass `ec` directly, drop `ec_to_ctx`
     (update the JsResult closure pattern).
   - Domain callers with `&mut Context`: bridge via `context_as_ec(context)`,
     convert the `Completion` result back to `JsResult` with
     `.map_err(JsError::from_opaque)`.

**Note**: The `writablestreamdefaultwriter.rs` conversion serves as the reference
implementation — it demonstrates all the patterns above.

### Step 2: Remove remaining `ContextEventDispatchHost` adapters

Two adapters remain, in:
- `writablestreamdefaultcontroller.rs`
- `event_target.rs`

These implement `EventDispatchHost` on a `&mut Context` wrapper.  After
Step 1 threads EC through domain code, the adapter usage can be replaced
by passing `EnvironmentSettingsObject` (which implements both
`ExecutionContext<T>` and `EventDispatchHost`) through stream objects.

Options:
- Store an `EnvironmentSettingsObject` reference on stream objects
- Store the settings object in the EC's host-defined data store
- Refactor engine/settings ownership so settings can be retrieved from EC

### Step 3: GC abstraction for `Callback<T>`

The only non-mechanical step.  `Callback` currently derives
`boa_gc::Trace`/`Finalize`.  Abstract these behind conditional compilation
(`cfg(feature = "boa")` / `cfg(feature = "jsc")`) or a `GcBackend` trait.

This is the one genuinely engine-specific part of the crate — GC has no
ECMA-262 equivalent, so there is no spec to follow.

### Step 4: JSC feature parity

Bring the JSC backend up to parity with Boa — fill in `todo!()` stubs,
implement missing `ExecutionContext<T>` methods, and validate that the
JSC feature flag builds clean.

### Verification

Until the generic JS engine migration is complete, each sub-task only requires
`cargo check` (no WPT or navigation verification).  Full verification resumes
when the migration reaches a stable checkpoint (all Phase 7 files converted).

Each sub-task ends with a commit message suggestion covering the current diff.
