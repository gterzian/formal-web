# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.  Two categories of abstraction:

### 1. Standard: `JsEngine<T>` mirrors ECMA-262 operations

Web standards already define their behavior in terms of ECMA-262 operations:
`Call`, `Get`, `ToNumber`, `NewPromiseCapability`, `PerformPromiseThen`,
`CreateRealm`, etc.  The trait exposes them generically.  No new abstractions.

### 2. Weird: `gc.rs` abstracts engine-specific GC

GC has no ECMA-262 equivalent.  This module is deliberately the one
engine-specific part of the crate.

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

Every method on `JsEngine<T>` has **only** the spec anchor URL as its doc
comment.  Example: `/// <https://tc39.es/ecma262/#sec-toboolean>`.
No prose, no summaries.  The spec IS the documentation.

Infrastructure traits (`Trace`, `Finalize`, etc.) carry no spec links —
they are not spec-defined operations.

## Design

| Component | File | Role |
|---|---|---|
| `JsTypes` | `types.rs` | Associated-type bundle: all ECMAScript language types |
| `JsEngine<T>` | `engine.rs` | Engine factory — creates realms, built-in functions, evaluates scripts. Used at initialization only. |
| `ExecutionContext<T>` | `engine.rs` | Running execution context (§9.4) — provides ECMA-262 operations that implicitly reference the surrounding agent. Threaded through all call sites. |
| `EcmascriptHost<T>` | `engine.rs` | Subset of `ExecutionContext<T>`: ops Web IDL callback algorithms need. |
| `Completion<T, Ty>` | `engine.rs` | `Result<T, Ty::JsValue>` — isomorphic to spec Completion Record (§6.2.4). |
| `HostHooks<T>` | `engine.rs` | HTML-specified host hooks (promise rejection, etc.). |

### What does NOT get abstracted

| Operation | Reason |
|---|---|
| GC heap traversal (`downcast_ref`) | Engine-specific — no ECMA-262 equivalent |
| Native function registration (`NativeFunction`) | Engine-specific API shape |
| Platform object construction | Uses Boa `ObjectInitializer` |
| Proxy creation | Boa's proxy builder not publicly creatable |

These are handled by `#[repr(transparent)]` casts in the `CreateBuiltinFunction`
shim (see `boa/engine.rs` module docs).

## Per-backend details

See module docs for implementation status and quirks:

| Backend | Module | Docs |
|---|---|---|
| Boa | `src/boa/mod.rs` | Hard problems, known quirks |
| JSC | `src/jsc/mod.rs` | FFI coverage, `todo!()` items |
| GC | `src/gc.rs` | The one engine-specific abstraction |

## Architecture: engine factory + execution context

The ECMA-262 spec (§9.4) defines an **execution context** as the device that
tracks runtime evaluation — it carries the Realm, the code evaluation state,
the ScriptOrModule, and is pushed/popped from the execution context stack.
The **running execution context** (§9.4) is the top of this stack; all
implicit ECMA-262 operations (`Call`, `Get`, `ToNumber`, `SameValue`,
`currentRealm`, etc.) reference it through the **surrounding agent**.

The HTML spec defines a **realm execution context**
(§8.1.3.3, [realm-execution-context]) as the execution context stored on an
environment settings object.  `prepare to run script` (§8.1.4.4) pushes it
onto the stack; `clean up after running script` pops it.

Two traits model this split:

| Component | Role | ECMA-262 anchor |
|---|---|---|
| `JsEngine<T>` | Factory — creates realms, built-in functions, evaluates scripts. Used at initialization only. | `CreateRealm` (§9.3), `CreateBuiltinFunction` (§10.3), `ScriptEvaluation` (§16.1) |
| `ExecutionContext<T>` | Runtime — the running execution context. Provides all operations that implicitly reference the surrounding agent. Threaded through every binding function, domain method, and dispatch call. | Running execution context (§9.4) → all of §7, §9.3 (currentRealm), §9.6 (jobs), value construction |
| `EcmascriptHost<T>` | Subset of `ExecutionContext<T>` covering only Web IDL callback algorithms (`Get`, `IsCallable`, `Call`, `report_exception`, value construction). | §3 of Web IDL |

[realm-execution-context]: https://html.spec.whatwg.org/#realm-execution-context

### What moves where

**`JsEngine<T>` (factory — stays):**
- `create_realm`, `set_realm_global_object`, `set_default_global_bindings`
- `create_builtin_function`
- `evaluate_script`, `evaluate_module`
- `set_host_hooks`
- `allocate_array_buffer`, `allocate_shared_array_buffer`
- `clone_array_buffer`, `detach_array_buffer`

**`ExecutionContext<T>` (runtime — split from current `JsEngine<T>`):**
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

**`EcmascriptHost<T>` (subsume into `ExecutionContext<T>`):**
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

### Phase 5: GC derives (ongoing)

Make `Callback` generic over `T: JsTypes`. Requires abstracting GC trait
derives (`#[derive(Trace, Finalize)]`) — the one genuinely engine-specific
part.  Strategy: conditional compilation or a `GcBackend` trait.

## Dependency graph

```
Phase 1 (ExecutionContext<T> split)
    │
    ├── Phase 2 (generic OperationDef/AttributeDef)
    │       │
    │       ├── Phase 3 (thread through domain code)
    │       │       │
    │       │       └── Phase 4 (remove adapter structs)
    │       │
    │       └── (binding functions become generic)
    │
    └── Phase 5 (generic Callback — GC derives)
```

## Migration status

| Phase | What | Status |
|---|---|---|
| 1. Trait split | `ExecutionContext<T>` split from `JsEngine<T>`. Added `global_object()`, `property_key_from_str()` to EC. EC requires `T: JsTypesWithRealm`. | ✅ |
| 2. Generic bindings | `OperationDef<T>`, `AttributeDef<T>`, `ConstantDef<T>`, `InterfaceDefinition<T>`, `WebIdlInterface<T>`, `WebIdlNamespace<T>` parameterized over `T: JsTypes`. Fn pointer types remain Boa-concrete (Phase 3). | ✅ |
| 3. EC infrastructure | Host-defined data store (`store_host_any`, `get_host_any`, `remove_host_any`) on EC. `RegistryHost` wrapper for Context storage. `NativeDataWrapper` for any-to-NativeObject bridging. Boa/JSC backends updated. | ✅ |
| 4. Generic registry | `InterfaceRegistry<T: JsTypes>` stores `T::JsObject`. `InterfaceEntry<T>` generic. | ✅ |
| 5. Adapter removal | `ContextEventDispatchHost` × 2, `CtxHost` removed | ❌ |
| 6. Generic Callback | GC derives abstracted, `Callback<T>` | ❌ |
| 7. JSC parity | Missing JSC methods implemented | ❌ |

## Current state

### What works

- `js_engine` crate has the correct three-trait architecture: `JsEngine<T>` (factory) → `ExecutionContext<T>` (runtime) → `EcmascriptHost<T>` (Web IDL callbacks).
- All binding structs are parameterized over `T: JsTypes`: `OperationDef<T>`, `AttributeDef<T>`, `ConstantDef<T>`, `InterfaceDefinition<T>`.
- All 33 `WebIdlInterface<T>` impls and 1 `WebIdlNamespace<T>` impl are parameterized.
- `InterfaceRegistry<T>` stores engine-native `T::JsObject` types.
- Host-defined data store (`store_host_any`/`get_host_any`/`remove_host_any`) provides type-erased storage on `ExecutionContext<T>`.
- `NativeDataWrapper<T>` bridges arbitrary `'static` data to Boa's `NativeObject` trait for platform object creation.

### What's still Boa-concrete

- **Fn pointer types:** `OperationDef<T>::method`, `AttributeDef<T>::getter`/`setter` still have Boa-specific signatures (`fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>`). These need `&mut dyn ExecutionContext<T>` parameters (Phase 5).
- **`register_interface_spec`:** Still takes `&mut Context` and uses Boa `NativeFunction`, `FunctionObjectBuilder`, `PropertyDescriptor` builder, `JsObject::from_proto_and_data`.
- **`create_interface_instance`:** Still takes `&mut Context` and uses `JsObject::from_proto_and_data` with `NativeObject` bound.
- **`Callback`:** Derives `boa_gc::Trace`/`Finalize` — blocks generic Web IDL callback algorithms.
- **`promise.rs`:** All promise helpers take `&mut Context`. Need to use `ec.new_promise_capability()`, `ec.promise_resolve()`, etc.
- **`buffer_source.rs`:** Uses `JsArrayBuffer::from_object`, `JsTypedArray::from_object` directly.

## Next steps (priority order)

### Phase 5: Thread `ExecutionContext<T>` through binding function signatures

Change `OperationDef<T>::method`, `AttributeDef<T>::getter`/`setter` from:
```rust
fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>
```
to:
```rust
fn(&T::JsValue, &[T::JsValue], &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>
```

This requires: (a) adding the repr(transparent) cast in each binding function body to recover `&mut Context` for Boa-specific operations, and (b) changing the `define_*` functions in `operation.rs`, `attribute.rs`, `constant.rs` to use `ec.define_property_or_throw()`, `ec.create_builtin_function()`, etc. instead of Boa-specific APIs.

### Phase 6: Thread `ExecutionContext<T>` through domain code

Change every domain method that currently takes `&mut Context` to take `&mut dyn ExecutionContext<T>`:
- `EventDispatchHost` loses `fn context()`
- `dispatch.rs`, `abort.rs` use EC for `create_interface_instance` and `global_object()`
- `promise.rs` helpers use `ec.new_promise_capability()`, `ec.promise_resolve()`
- All domain code uses `ec.to_number()`, `ec.to_js_string()`, `ec.get()`, etc.

### Phase 7: GC abstraction for `Callback<T>`

Make `Callback<T>` generic over `T: JsTypes`. Requires abstracting `boa_gc::Trace`/`Finalize` derives — the one genuinely engine-specific part. Strategy: conditional compilation with `cfg(feature = "boa")` vs `cfg(feature = "jsc")`, or a `GcBackend` trait.

## Phase 2 scope

The following components need `T: JsTypes` parameterization:

| File | What changes | Count |
|---|---|---|
| `bindings/operation.rs` | `OperationDef<T>`, `define_operations_on_target<T>` | 1 struct, 5 fns |
| `bindings/attribute.rs` | `AttributeDef<T>`, `define_attributes_on_target<T>` | 1 struct, 5 fns |
| `bindings/constant.rs` | `ConstantDef<T>` (uses `T::PropertyKey`), `define_constants<T>` | 1 struct, 1 fn |
| `bindings/interface.rs` | `InterfaceDefinition<T>`, `WebIdlInterface<T>`, `WebIdlNamespace<T>`, `register_interface_spec`, `create_interface_instance` | 2 traits, 6 fns |
| `bindings/registry.rs` | `get_prototype_from_host_defined<T>`, `register_in_host_defined<T>` | 3 fns |
| `bindings/mod.rs` | Re-exports | 1 file |
| `js/bindings/<domain>/*.rs` | All 33 `WebIdlInterface` impls + 1 `WebIdlNamespace` | 34 files |

All binding functions change from:
```rust
fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>
```
to:
```rust
fn(&T::JsValue, &[T::JsValue], &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>
```

`register_interface_spec` and `register_namespace_spec` stay monomorphized
for `BoaTypes` because they call Boa `NativeFunction`, `FunctionObjectBuilder`,
and `JsObject::from_proto_and_data` internally.  The engine-specific
registration bridge is the `repr(transparent)` cast from
`&mut boa_engine::Context` → `&mut BoaEngine` → `&mut dyn ExecutionContext<BoaTypes>`,
handled by an adapter function in the boas engine's registration helper.
