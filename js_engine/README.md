# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Architecture

> **Principle:** The architecture is defined by the standards.  We don't
> invent new layers — we follow the spec chain exactly and make it generic.

**End state:** All content code (domain, webidl, bindings) operates
 exclusively on the generic JS API — `ExecutionContext<T>`,
 `EcmascriptHost<T>`, `JsTypes`.  Zero `boa_engine::*` imports in
 production code.  Zero `ec_to_ctx` / `context_as_ec` bridges.  Zero
 `_ec`-suffixed wrappers.  Backend-specific code lives only inside
 `js_engine/src/{boa,jsc}/`.  Every intermediate step — converting a
 closure, deleting a wrapper, removing a bridge — is judged by whether it
 moves toward this end state.

### 0. Migration methodology — spec-first, not Boa-first

When converting Boa-specific code to the generic layer, **follow the spec
chain**, not the Boa API shape.

#### Core rules

1. **Go deep, not broad.**  When converting a function to take
   `&mut dyn ExecutionContext<T>`, trace its ENTIRE call chain — across
   files if needed — and convert every function it calls.  Never leave
   bridges (`context_as_ec`, `_ec` wrappers, `ec_to_ctx`,
   `completion_to_js_result`) at the boundaries.  If a called function
   still needs `Context`, convert it too.  This is **call-chain
   migration**, not file-by-file migration.

2. **Zero `ec_to_ctx` anywhere in domain code.**  Every `ec_to_ctx`,
   `context_as_ec`, and `completion_to_js_result` call is a bridge that
   must eventually be deleted.  Instead of adding one, convert the function
   being called to take `&mut dyn ExecutionContext<T>` directly.  When
   that function's signature changes, convert ITS callers too — follow
   the chain until everything compiles with `Completion` throughout.
   **No bridging at any level — not in fn pointers, not in wrappers,
   not at call sites.**  The only place bridges exist is inside
   `js_engine/src/` (the engine backend), where `ec_to_ctx` is an
   internal implementation detail of the Boa adapter.

3. **No `_ec` suffix anywhere.**  Do not create methods or functions named
   `foo_ec()`.  The EC-taking version IS the real function.  If both
   Context and EC variants coexist during migration, name the legacy
   Context version `foo_ctx()` and the real one `foo()`.  End state:
   everything takes EC, no `_ec` suffix exists.

#### Spec chain reference

4. **Read the spec algorithm.** Identify every ECMA-262 abstract operation
   it calls (Call, Get, PerformPromiseThen, NewPromiseCapability,
   CreateBuiltinFunction, etc.).

5. **Use the `ExecutionContext<T>` trait methods** that implement those
   ECMA-262 operations — never reach for Boa APIs when a generic equivalent
   exists.

#### Concrete patterns

6. **For promise chaining**, use `ec.perform_promise_then(promise, on_fulfilled,
   on_rejected, None)` — not `JsPromise::from_object(p)?.then(...)`.
```
   // ❌  Boa-specific (bypasses EC trait)
   let result = JsPromise::from_object(promise)?.then(Some(on_fulfilled), None, context)?;

   // ✅  Generic (spec: ECMA-262 PerformPromiseThen)
   let js_promise = Types::object_as_promise(&promise).ok_or_else(...)?;
   ec.perform_promise_then(js_promise, Some(on_fulfilled), None, None)?;
```

7. **For creating promises**, use `ec.new_promise_pending()` — not
   `JsPromise::new_pending(context)`.
```
   // ❌  Boa-specific
   let (promise, resolvers) = JsPromise::new_pending(context);

   // ✅  Generic (spec: ECMA-262 NewPromiseCapability)
   let (promise, resolvers) = ec.new_promise_pending()?;
```

8. **For domain functions that take `&mut Context`**: convert them
   to take `&mut dyn ExecutionContext<T>` directly.  Do NOT create
   standalone `_ec` wrapper functions that bridge Context→EC.
   Convert the real function.

9. **For `ResolvingFunctions::resolve/reject.call(_, _, ctx)`**: use
   `ec.call()` directly.  `ResolvingFunctions.resolve` is a
   `JsFunction` which converts to `JsObject` via `.into()`.
```
   // ❌  Needs Context
   resolvers.resolve.call(&JsValue::undefined(), &[value], context)?;

   // ✅  Uses EC directly — zero bridges
   let resolve: JsObject = resolvers.resolve.into();
   let undefined = ec.value_undefined();
   ec.call(&resolve, &undefined, &[value])?;
```

10. **For `builtin_with_captures`** (the only operation that still needs
   `&mut Context`): the parent function should keep its `&mut Context`
   parameter if possible.  The fn pointer itself takes `ec` directly
   with zero bridges.  When the parent function has already been
   converted to EC, use `ec_to_ctx` once at the top — this is the
   one remaining bridge pattern, and it exists because
   `create_builtin_function_with_captures` lives on `JsEngine<T>`
   (factory trait), not `ExecutionContext<T>` (the runtime trait
   that domain code receives).  Once that method moves to the EC
   trait, even this bridge is eliminated.

#### Anti-patterns (do NOT do these)

- Creating `xxx_ec()` wrapper functions that call `ec_to_ctx` internally
- Creating `xxx_ctx()` wrapper functions that call `context_as_ec` + `completion_to_js_result`
- Using `resolvers.resolve.call(&undefined, &[value], ctx)` when `ec.call()` is available
- Using `JsPromise::then()` when `perform_promise_then` exists on the trait
- Using `JsPromise::new_pending(context)` when `ec.new_promise_pending()` exists
- Using `JsNativeError::typ().with_message(msg)` when `ec.new_type_error(msg)` exists
- Using `completion_to_js_result` or `context_as_ec` at call sites — convert the caller to EC instead
- Converting one file at a time while leaving bridges at its edges
- Adding `_ec` suffix to struct methods — just rename the real method

### 1. The ownership model

<https://html.spec.whatwg.org/#environment-settings-objects> (§8.1.3.2)
defines the **environment settings object**, which owns a **realm execution
context** — a JavaScript execution context shared by all scripts in a given
realm.  When we <https://html.spec.whatwg.org/#prepare-to-run-script>
(§8.1.4.4), this context becomes the top of the execution context stack.

Our `EnvironmentSettingsObject` (`content/src/html/environment_settings_object.rs`)
owns a `BoaContext` which implements `ExecutionContext<T>`.  The
`ExecutionContext<T>` trait **is** the generic interface to that realm
execution context.  The migration end state is for the EDS to own the
generic trait type instead of the concrete `BoaContext` — the ownership
boundary is already correct, only the type needs to become generic.

### 2. The two paths into JavaScript

Every web standard reaches JavaScript through one of two paths.
We follow the exact spec call chain in each case.

#### Path 1: Domain → Web IDL → ECMA-262

Most web-exposed APIs (Streams, DOM) call Web IDL, which calls ECMA-262.

**Example — `readableStream.cancel(reason)`:**

| Layer | Spec | Our code |
|---|---|---|
| Domain | <https://streams.spec.whatwg.org/#readable-stream-cancel> | `content/src/streams/readablestream.rs` → `readable_stream_cancel_ec()` |
| Web IDL | <https://webidl.spec.whatwg.org/#a-promise-resolved-with> | `content/src/webidl/promise.rs` → `resolved_promise()` |
| Web IDL | <https://webidl.spec.whatwg.org/#a-promise-rejected-with> | `content/src/webidl/promise.rs` → `rejected_promise()` |
| Web IDL | <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled> ("react") | `content/src/webidl/promise.rs` → `transform_promise_to_undefined()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-createbuiltinfunction> | `js_engine` → `create_builtin_function()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-newpromisecapability> | `js_engine` → `new_promise_capability()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-performpromisethen> | `js_engine` → `perform_promise_then()` |

**Example — `eventTarget.addEventListener(type, callback)`:**

| Layer | Spec | Our code |
|---|---|---|
| Domain | <https://dom.spec.whatwg.org/#dom-eventtarget-addeventlistener> | `content/src/js/bindings/dom/event_target.rs` → `add_event_listener()` |
| Web IDL | <https://webidl.spec.whatwg.org/#call-a-user-objects-operation> | `content/src/webidl/callback.rs` → `call_user_objects_operation()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-call> | `js_engine` → `ExecutionContext::call()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-get-o-p> | `js_engine` → `ExecutionContext::get()` |

#### Path 2: Domain → ECMA-262 (bypasses Web IDL)

Some HTML algorithms call ECMA-262 directly — realm creation, script
evaluation.

| Layer | Spec | Our code |
|---|---|---|
| HTML | <https://html.spec.whatwg.org/#creating-a-new-javascript-realm> | `content/src/html/` → calls `js_engine::create_realm()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-createrealm> | `js_engine` → `JsEngine::create_realm()` |
| HTML | <https://html.spec.whatwg.org/#run-a-classic-script> | `content/src/html/` → calls `js_engine::evaluate_script()` |
| ECMA-262 | <https://tc39.es/ecma262/#sec-runtime-semantics-scriptevaluation> | `js_engine` → `JsEngine::evaluate_script()` |

**The rule:** read the spec, follow its call chain exactly.  Route through
`content/src/webidl/` only when the spec calls Web IDL.  Call `js_engine`
directly when the spec calls ECMA-262 directly.  Never insert an artificial
intermediary layer that doesn't exist in the spec.

### 3. Crate layering

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
   into `content/src/webidl/` when the spec calls Web IDL (§3 type
   conversion, promise manipulation), or into the `js_engine` trait
   when the spec calls ECMA-262 directly.  The Boa/JSC backend is
   invisible above `js_engine/src/{boa,jsc}/`.

2. **The js_engine trait only exposes ECMA-262 operations.**  Operations
   like "report an exception" or "perform a microtask checkpoint" are
   HTML concepts, not ECMA-262 — they live on `EcmascriptHost` because
   Web IDL needs them.  The trait never defines "convenience" methods
   that don't correspond to a spec algorithm.

3. **The webidl/ layer implements Web IDL §3.**  Type conversion
   algorithms ("convert a JavaScript value to DOMString", "convert a
   JavaScript value to Promise<T>"), promise manipulation ("react",
   "a new promise", "upon fulfillment"), and the binding
   infrastructure (interface prototypes, operation/attribute
   definitions) all live in `content/src/webidl/`.  This layer calls
   `js_engine` for the actual ECMA-262 operations.

4. **The js/bindings/ layer defines which members exist.**  Each
   `WebIdlInterface` impl in `content/src/js/bindings/` registers
   operations and attributes via the Web IDL binding infrastructure.
   The binding functions themselves are thin: extract JS args, call
   domain, wrap result.

5. **Ad-hoc Boa patterns must be replaced by spec algorithms.**  For
   example, `NativeFunction::from_closure` → `create_builtin_function`,
   `JsArray::from_iter` → `create_empty_array` + `array_push`, and
   `JsNativeError::syntax()` → `new_syntax_error`.  If a Boa pattern
   doesn't have a spec equivalent, it's a gap to fill, not a wrapper
   to build.

6. **Test the full chain end-to-end.**  The generic test file
   (`content/src/generic_js_test.rs`) is a miniature version of the
   full `content/` crate.  It demonstrates both paths: realm creation
   (HTML → ECMA-262 directly, tested via `create_realm_and_set_bindings`)
   and promise reaction (Streams → Web IDL "react" → ECMA-262, tested
   via `upon_settlement_full_chain`).  No Boa-specific APIs appear in
   any test body.

The `js_engine` crate exposes **only** the ECMA-262 operations that other
standards call into (usually via Web IDL).  This is a mechanical mapping:
read the spec call chain, expose the JS spec operation on the trait,
implement it per engine.  No new abstractions beyond what the JS spec
already defines.

### Two categories of abstraction

- **Standard**: `JsEngine<T>` / `ExecutionContext<T>` mirror ECMA-262 operations.
- **Engine-specific**: `gc.rs` abstracts GC (no ECMA-262 equivalent).

### Design principle: engine-specific code stays inside the backend

Domain code and Web IDL helpers call ECMA-262 operations through the
generic `ExecutionContext<T>` trait — never through Boa or JSC APIs.
`ec_to_ctx` exists only in `js_engine/src/` and is an internal
implementation detail of the engine adapters.

### Concrete realization

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

### What does NOT get abstracted (yet)

| Operation | Reason |
|---|---|
| Native function registration (`NativeFunction::from_closure`) | `create_builtin_function_with_captures` on `JsEngine<T>` accepts a traceable captures struct + fn pointer instead of an opaque boxed closure.  Boa backend uses the safe `from_copy_closure_with_captures`.  Domain code (transformstream.rs, readablestreamdefaultcontroller.rs, etc.) still uses `NativeFunction::from_copy_closure_with_captures` directly — needs migration to the new trait method. |
| Platform object construction | Uses Boa `ObjectInitializer` — needs realm's intrinsics table; passes through EC |
| Proxy creation | Boa's proxy builder not publicly creatable |
| `Context::eval` (script evaluation) | `JsEngine::evaluate_script` exists on the trait but callers use `Context::eval` directly; needs migration |
| `JsValue::to_json(&mut Context)` | Boa-specific JSON serialization; needs a trait method |
| `with_global_scope(&Context, ...)` | Boa GC heap traversal to access `GlobalScope`; partially resolved by `realm_global_object()` on `ExecutionContext` — `platform_objects.rs` `_ec` wrappers now use only trait methods. Non-`_ec` callers (`main.rs`, `environment_settings_object.rs`, `html_media_element.rs`) still use `with_global_scope` via `&Context`. |
| `register_global_property`, `ObjectInitializer::new(ctx)`, `JsArray::from_iter(..., ctx)` | Boa object model construction APIs; need trait equivalents or centralized construction in `build_context` |

These are the blockers to `EnvironmentSettingsObject` owning a purely generic context
instead of `BoaContext`.  None are fundamental — they just aren't done yet.

### HostMakeJobCallback — design direction

<https://tc39.es/ecma262/#sec-hostmakejobcallback>

This is a separate concern from `perform_promise_then` (which is already
correctly generic).  `HostMakeJobCallback` / `HostCallJobCallback` wrap
callbacks with `[[HostDefined]]` data (incumbent settings object, active
script) so the HTML spec's "prepare to run a callback" steps happen
automatically when the engine invokes a callback.

**What already works:** `perform_promise_then` on the EC trait handles
promise reactions (streams reacting to promises).  Boa internally calls
its own `HostEnqueuePromiseJob` hook — no action needed from content.

**What's future work:** Implement the engine's native host hooks trait
(`boa_engine::context::HostHooks`) so `[[HostDefined]]` data is captured
at `HostMakeJobCallback` time and restored at `HostCallJobCallback` time.
This replaces manual threading of settings objects through closures.
JSC has no equivalent C API; we'd simulate with our own layer.

**What `enqueue_job_with_realm` does:** Explicit content-initiated
microtask queueing (HTML's `queue a microtask`, streams'
`queue_internal_stream_microtask`).  Separate from promise reactions.

### Platform object downcast without GC abstraction

`downcast_ref::<T>()` and `downcast_mut::<T>()` on `JsObject` do not
require `Context`.  EC trait methods replace all other Boa APIs:

| Old (needs `ctx`) | New (EC trait) |
|---|---|
| `JsNativeError::typ().with_message(msg)` | `ec.new_type_error(msg)` |
| `JsValue::undefined()` | `ec.value_undefined()` |
| `v.to_boolean()` | `ec.to_boolean(v)` |
| `JsValue::new(n)` | `ec.value_from_number(n)` |

## Layout

```
src/
  lib.rs        Crate root
  types.rs      JsTypes — language types (§6.1) and object subtypes
  engine.rs     JsEngine<T>, EcmascriptHost<T>, Completion, HostHooks
  enums.rs      Numeric, PreferredType, IntegrityLevel, etc.
  records.rs    IteratorRecord, PromiseCapability, PropertyDescriptor
  gc.rs         Trace, Finalize, GcRootHandle, GcCell<T>, gc_cell_new()
  boa/          Boa backend (feature = "boa")
  jsc/          JSC backend (feature = "jsc")

`js_engine_macros/` — proc-macro crate providing `#[gc_struct]`.
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

## Generic API surface (proven in POC)

The `generic_js_test.rs` POC proves every content pattern can be expressed
through the generic API.  See the test file for working examples of each.

### Platform object lifecycle

| Operation | Trait method | POC example |
|---|---|---|
| Create object with native data | `ec.create_object_with_any(prototype, Box::new(data))` | `create_test_widget` |
| Get realm's global object | `ec.realm_global_object() -> T::JsObject` | `realm_global_object_returns_valid_js_object` |
| Read native data (immutable) | `ec.with_object_any(obj) -> Option<&dyn Any>` | `widget_data::with_ref` |
| Read native data (mutable) | `ec.with_object_any_mut(obj) -> Option<&mut dyn Any>` | `widget_data::with_mut` |

`with_object_any` and `with_object_any_mut` are object-safe — callable on
`&dyn ExecutionContext<T>`.  They return typed references that the caller
downcasts via `dyn Any::downcast_ref::<T>()` / `downcast_mut::<T>()`.

### GC integration

| Operation | Mechanism | POC example |
|---|---|---|
| GC trait derivation | `#[gc_struct]` attribute macro | `TestWidget` struct |
| GC-managed cell | `GcCell<T>` (Boa: `Gc<GcRefCell<T>>`, JSC: `Rc<RefCell<T>>`) | Domain struct fields |
| Store a JS callback | `Option<GcRootHandle<Types>>` field | `on_change` field |
| Root a JS value | `ec.create_root(&value) -> GcRootHandle<T>` | `store_callback` |

`#[gc_struct]` replaces the old `impl_gc_traits!` declarative macro.  It emits:
- Boa: `#[derive(Clone, boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)]` (structs)
  or `#[derive(Clone, boa_gc::Finalize, boa_gc::Trace)]` (enums, no JsData)
- JSC: `#[derive(Clone)]` + no-op `Trace` and `Finalize` impls

`GcCell<T>` is a backend-abstracted type alias for GC-managed interior
mutability.  Construct with `gc_cell_new(val)`, access with `.borrow()` /
`.borrow_mut()`.  On Boa it maps to `Gc<GcRefCell<T>>` so the GC traces
through it; on JSC it maps to `Rc<RefCell<T>>`.

`GcRootHandle<T>` is an RAII guard:
- Boa: no-op (GC traces through `boa_gc::Trace` on the handle itself)
- JSC: stores the value as a non-enumerable property on the global object
  to keep it alive in JSC's GC graph; deletes the property on drop.
  (Avoids `JSValueProtect` which SIGSEGVs on eval-created values on
  some macOS versions.)

**Tests:**
- `gc_root_survives_throwaway_pressure`: allocates 1000 throwaway arrays,
  then verifies a `GcRootHandle`-rooted callback still calls correctly.
- `nested_struct_gc_root_propagates`: `TestButton` wraps `TestWidget` which
  holds `Option<GcRootHandle<Types>>` — verifies `Trace` propagates through
  nested `impl_gc_traits!` structs and the rooted callback survives round-trip
  through the outer object.

### Value construction and conversion

| Operation | Trait method |
|---|---|
| `undefined` | `ec.value_undefined()` |
| `null` | `ec.value_null()` |
| Boolean | `ec.value_from_bool(b)` |
| Number | `ec.value_from_number(n)` |
| String | `ec.value_from_string(ec.js_string_from_str(s))` |
| BigInt | `ec.value_from_bigint(n)` |
| Object from native data | `ec.create_object_with_any(...)` |
| Plain object | `ec.create_plain_object(prototype)` |
| Empty array | `ec.create_empty_array()` |
| Array push | `ec.array_push(&arr, val)?` |
| Set property | `ec.object_set_property(obj, key, val)?` |
| TypeError | `ec.new_type_error(msg)` |
| RangeError | `ec.new_range_error(msg)` |
| Upcast: `JsValue` from `JsObject` | `Types::value_from_object(o)` |
| Downcast: `JsObject` from `JsValue` | `Types::value_as_object(&v)` |
| Downcast: rust String from JsValue | `ec.to_rust_string(v)?` |
| Extract: rust String from JsString | `ec.js_string_to_rust_string(&s)` |

### Binding function signature

```rust
fn binding_fn(
    this: &Types::JsValue,
    args: &[Types::JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Types::JsValue, Types>
```

No `ec_to_ctx`, no `JsResult`, no `Context`.  The EC provides everything.

### Content pattern → generic equivalent

| Content pattern | POC function | Key API calls |
|---|---|---|
| Simple getter | `get_title` | `Types::value_as_object`, `with_ref`, `ec.value_from_string` |
| String setter | `set_title` | `ec.to_rust_string` |
| Numeric setter | `set_count` | `ec.to_uint32` |
| Method | `increment` | `with_mut` |
| Constructor with args | `from_args` | `ec.to_rust_string`, `ec.to_boolean` |
| Static factory | `create_static` | `create_object_with_any` |
| Plain-object return | `to_object` | `ec.create_plain_object`, `ec.object_set_property` |
| Array return | `to_array` | `ec.create_empty_array`, `ec.array_push` |
| Promise-returning | `delayed_title` | `ec.new_promise_capability`, `ec.call` |
| Callback invocation | `with_callback` | `ec.call`, `ec.is_callable` |
| Callback storage | `store_callback` | `ec.create_root` |
| Array-like length+indexing | `process_items` | `ec.property_key_from_index`, `ExecutionContext::get` |

**Note on `process_items`:** `process_items` uses array-like length+indexing
(`Get` for `"length"` then `Get` for `0..length`).  This is **not** the
Web IDL `sequence<T>` conversion algorithm, which is iterator-based
(`GetIterator` + `IteratorStep`/`IteratorValue`).  As written, it would
mis-convert iterable-but-not-array-like arguments (`Set`, generator, custom
iterable).  Either rewrite on `get_iterator`/`iterator_step_value` to match
`sequence<T>`, or rename/re-comment to make clear it models array-like
access, not WebIDL sequence conversion.

## Spec documentation convention

Every method on `JsEngine<T>` and `ExecutionContext<T>` has **only** the
spec anchor URL as its doc comment.  Example:
`/// <https://tc39.es/ecma262/#sec-toboolean>`.
No prose, no summaries.  The spec IS the documentation.

Infrastructure traits (`Trace`, `Finalize`, etc.) carry no spec links —
they are not spec-defined operations.

## Design notes

### `with_object_any` / `with_object_any_mut`

Return `Option<&dyn Any>` / `Option<&mut dyn Any>` — the caller downcasts.
Object-safe on `&dyn ExecutionContext<T>`.  Boa backend uses unsafe lifetime
extension (data lives in GC heap).

### `with_object_any_mut_with`

For patterns where mutation needs to call ECMA-262 operations, use
`with_object_any_mut_with` which passes both `&mut dyn Any` and
`&mut dyn ExecutionContext<T>` to a closure.

### What does NOT belong on the EC trait

- **`js_string_from_str`** — convenience, no spec equivalent
- **`report_error`** (default impl) — logging convenience
- **`report_exception`**, **`perform_a_microtask_checkpoint`** — HTML concepts, live on `EcmascriptHost`

## Per-backend details

See module docs for implementation status and quirks:

| Backend | Module | Status |
|---|---|---|
| Boa | `src/boa/mod.rs` | ✅ Full parity — all trait methods implemented, all generic_js_test tests pass |
| JSC | `src/jsc/mod.rs` | 🔶 Trait surface complete. `create_builtin_function` implements behaviour closures via JSClass + private data. `create_root` uses global-object properties instead of `JSValueProtect`. `get` handles Symbol keys via eval fallback. 1 remaining ignore: `SharedArrayBuffer` (may not be available). `exercise_context_lifecycle` (registry init + interface registration end-to-end) is Boa-only — no JSC counterpart yet. |
| GC | `src/gc.rs` | ✅ Complete — `#[gc_struct]` attribute macro, `GcCell<T>` type alias, `GcRootHandle<T>` with Boa trace impl, `create_root` on EC trait. `Trace` is a supertrait of `boa_gc::Trace` on Boa. GC-pressure tests pass. |

## Migration status

POC is **complete** — 70/70 tests pass on Boa in `content/src/generic_js_test.rs`
(content JSC blocked on Phase E).
(see JSC backend status for details).  The test file
(`content/src/generic_js_test.rs`) proves every content pattern can be
expressed through the generic API with zero structural `#[cfg]`.

### Practical end state

**Minimum shippable:**
- No `ec_to_ctx`, `context_as_ec`, `context_as_ec_ref`, or `context_as_engine` calls outside `js_engine` backend code.
- No `boa_engine::*` imports in production bindings, domain algorithms, Web IDL helpers, or Wasm-facing content code.
- Backend selection happens through compile-time aliases (`crate::js::Types`, `crate::js::Engine`).
- Generic POC remains green.
- Content crate compiles against both backend configurations.
- Any backend-specific code still present is isolated to bootstrap or engine-construction boundaries only.

**What the remaining work does NOT require:**
- A large expansion of `ExecutionContext<T>` with DOM or HTML methods.
- A second generic JS abstraction layer on top of `js_engine`.
- An immediate trait-object rewrite of all engine ownership.
- Backend-agnostic replacement of every bootstrap detail before the main content logic can be considered generic.

The actual missing abstractions are smaller and more local than that.

### Test-file-first discipline (applies to all remaining phases)

**Never add a new generic pattern directly to production code.**
Every new generic interface, downcast helper, host-data abstraction,
or subsystem entry-point signature must first be validated in
`content/src/generic_js_test.rs` with compilation and passing unit tests
on **both backends** (Boa and JSC) before it can be applied to any
real production file.

This means: before implementing Phase P's `platform_object_store(ec)`,
add a test that exercises the full lifecycle (store → retrieve → mutate).
Before converting Phase W's `structured_clone` to take `ExecutionContext<T>`,
add a test that clones a value through the generic entry point.  The POC
test file is the gate — no pattern enters production without passing through it first.

Concrete per-phase validation requirements:

| Phase | What to validate in `generic_js_test.rs` |
|---|---|
| **Phase D** ✅ | Return-type change only (trait methods `JsResult` → `Completion`). No new generic interface — validated by `cargo check` passing. |
| **Phase S** ✅ 🔶 | No new generic interface — streams domain methods already call only `ExecutionContext` trait methods. |
| **Phase P** | `store_host_any` / `get_host_any` already validated. New content-owned helpers (`platform_object_store(ec)`) must be validated: store a document handle, retrieve by key, mutate. |
| **Phase W** | Each subsystem entry point that changes signature must be exercised: structured clone round-trip, promise helper usage, Wasm namespace access. |
| **Phase E** | `cargo check -p content` with both `--features boa` and `--no-default-features --features jsc`. No new generic interface — configuration-only change. |

### Completed phases

| Phase | What |
|---|---|
| 1-9, D | Trait split, generic bindings, EC infrastructure, generic registry, binding fn signatures, CtxHost removal, EDS context leak, domain threading, GC abstraction, JSC backend, dispatch host cleanup |
| S1-S10 | Streams bindings at 0 ec_to_ctx; Controller JsResult→Completion; PromiseResolvers<T> in js_engine and content |
| P1-P3 | Platform objects `_ec` wrappers; `realm_global_object()` trait method; `platform_objects.rs` 8→0 ec_to_ctx |
| T1-T2 | Typed array trait methods (11 methods); all callers converted |
| W1-W2 | WebIDL promise conversion; streams helpers conversion |
| G1-G3 | `#[gc_struct]` proc-macro; `GcCell<T>` type alias; `Clone` emitted |
| C2-C3 | `create_builtin_function_with_captures`; 16 NativeFunction → captures migrated |
| A-C | GC derive conversion; binding body conversion; `create_builtin_function` on EC |
| **S-promise** | `PromiseState<T>` enum in js_engine; `promise_state()` method on `ExecutionContext<T>` trait; Boa + JSC backend impls. Replaces `JsPromise::from_object(x)?.state()` (Boa-specific) with `ec.promise_state(&obj)?`. |
| **S1a** | PipeToState EC wrappers (18 methods); `pipe_to_on_promise_settled_ec`; `pipe_reaction_fn` + `pipe_reaction_function_ec`; `pipe_read_result_done_ec`; `queue_internal_stream_microtask_ec`; 3 ReadableStreamPipeTo closures converted to EC path |

### Remaining phases

| Blocker | Phase | What | Effort | Status |
|---|---|---|---|---|
| **Blocker 1** — Dispatch result-model mismatch | **Phase D** | Convert `EventDispatchHost` trait methods from `JsResult` to `Completion`. Delete `ContextEventDispatchHost` (both copies). Eliminate `js_result_to_completion` bridges from the dispatch path. | Small | ✅ Done — `EcDispatchHost` is the sole dispatch host; `ContextEventDispatchHost` deleted from both locations. |
| **Blocker 4** — Streams domain exposes `Context` | **Phase S** | Convert streams domain methods from `&mut Context` to `&mut dyn ExecutionContext<T>`. **Bindings complete** — all streams binding files at 0 ec_to_ctx. **Typed array operations converted** — 11 new trait methods added, all callers converted. **NativeFunction closures in progress** — 7 of 14 converted to fn pointers with zero ec_to_ctx; 7 remain blocked on deeper function conversions (see Current state table). ~70 domain-internal calls remain (stream_slot/controller_object JsResult methods, microtask/tee). | Large | 🔶 Bindings + typed arrays done. Closures in progress. |
| **Blocker 2** — Platform-object state through Boa access paths | **Phase P** | Create content-owned host-data-backed store for platform-object bookkeeping, OR add `_ec` wrappers for remaining `&Context`-taking functions. `store_host_any` / `get_host_any` already validated. `realm_global_object()` trait method on `ExecutionContext` provides generic access to the global object (§8.1.3). `with_global_scope_ec` in `platform_objects.rs` combines `realm_global_object()` + `with_object_any` + `downcast_ref::<Window>()` — zero `ec_to_ctx`. WindowProxy needs `JsProxyBuilder` which has no trait equivalent yet — may need `create_proxy` on `ExecutionContext`. | Medium | 🔶 platform_objects.rs 8→0 ec_to_ctx. Remaining: abort.rs (3), windowproxy.rs (2), singletons (2). |
| **Blocker 5** — Subsystem entry points assume Boa | **Phase W** | Convert structured clone, Web IDL promise helpers, async iterable helpers, and Wasm to take `ExecutionContext<T>`. Same `_ec` wrapper pattern as Phase S/P — no new generic interfaces needed. `buffer_source.rs` now covered by typed array trait methods (T1). | Medium | 🔶 promise.rs 9→3. Remaining: JsError helpers (3), structured clone (1), async iterable (1), wasm (6), windowproxy (2). |
| **Blocker 3** — Engine ownership is structurally Boa-specific | **Phase E** | Land compile-time `Types` / `Engine` aliases. Backend selection becomes a `#[cfg]` choice. Validated by `cargo check` with both feature sets. | Large | Blocked on D, S, P, W |
| **Blocker 6** — Global-scope helpers are implicitly Boa | **Phase G** | Move `document_creation_url`, `with_global_scope`, etc. behind content-owned query helpers. | Small | Part of Phase P |

### Current state (updated 2026-07-03)

**Phases A–D, S1–S10, T1–T2, W1–W2, G1–G3, C2–C3 complete.** All binding files
at 0 ec_to_ctx.  All 34 struct/enum definitions use `#[gc_struct]`.  All domain
field types use `GcCell<T>`.

**POC test suite: 79/79 pass on Boa.**

**1 closure remaining in `readablestream.rs`** (from_copy_closure):

| Closure | Blocker |
|---|---|
| byte_tee_pull_byob on_fulfilled | Deeper tee algorithm conversions — `clone_as_uint8_array`, `byte_tee_enqueue_to_branch`, `readable_stream_cancel` (all Context) |

`pipe_reaction` is now fully addressed: `pipe_reaction_fn` (fn pointer) + `pipe_reaction_function_ec` (using `builtin_with_captures_ec`) provide the EC alternative. The old `NativeFunction::from_copy_closure_with_captures` in `pipe_reaction_function` remains for the Context code path.

**PipeToState EC wrappers:** ✅ All 18 methods have `_ec` wrappers. `pipe_to_on_promise_settled_ec`, `pipe_reaction_fn`, `pipe_reaction_function_ec`, `pipe_read_result_done_ec` all exist.

**`queue_internal_stream_microtask_ec`:** ✅ Added with `enqueue_job_with_realm`. All 3 `ReadableStreamPipeTo` closures in `readablestreamsupport.rs` now use it + `on_read_request_settled_ec`, eliminating 3 ec_to_ctx bridges.

**Remaining _ec wrappers blocked on deeper conversion:** `pipe_through_ec`, `pipe_to_ec`, `cancel_ec`, `get_reader_ec` — all delegate to Context-based counterparts that need conversion first. `readable_ec` — has a Context-based namesake (`readable()`) that would collide.

### Next session: recommended order

1. **Convert PipeToState impl to EC** — With `promise_state()` now available on the EC trait, convert all ~20 PipeToState methods from `&mut Context` → `&mut dyn ExecutionContext<T>`, replacing:
   - `JsPromise::from_object(x)?.state()` → `ec.promise_state(&x)?`
   - `JsPromise::from_object(x)?.then(...)` → `ec.perform_promise_then(...)`
   - `JsNativeError::typ().with_message(msg)` → `ec.new_type_error(msg)`
   - `result_object.has_property(js_string!("k"), ctx)` → `ec.has_property(obj, "k")`
   - `result_object.get(js_string!("k"), ctx)?.to_boolean()` → `ec.to_boolean(&ec.get(obj, "k")?)`
   - `resolvers.resolve.call(&undefined, &[v], ctx)` → `ec.call(&resolve.into(), &ec.value_undefined(), &[v])`
   - `JsPromise::new_pending(ctx)` → `ec.new_promise_pending()`
   - `NativeFunction::from_copy_closure_with_captures(...)` → fn pointer + `builtin_with_captures_ec`

2. **Convert `readable_stream_pipe_to` to EC** — uses PipeToState methods + `acquire_readable_stream_default_reader`, `acquire_writable_stream_default_writer`, `readable_stream_default_reader_release` (all have EC variants or already take EC).

3. **Convert entry points** (`ReadableStream::pipe_to`, `pipe_through`) — call `readable_stream_pipe_to`, `normalize_pipe_options`, `extract_abort_signal`.

4. **Update callers** — `abort.rs` (`run_abort_algorithm_ec` → `run_abort_algorithm`), `readablestreamsupport.rs` (ReadRequest closures), JS bindings (`pipe_to_ec` → `pipe_to`), `queue_internal_stream_microtask` → EC closure variant.

5. **Phase E — Conditional Types alias**.

### Working notes

**`builtin_with_captures` / `builtin_callback`:** Use
`crate::js::builtin_with_captures(context, captures, fn_ptr, length)` for
promise `.then()` handlers, `builtin_callback` for `SourceMethod`-wrapped
closures.  These are the only remaining sites that need `&mut Context` —
they use `context_as_engine(context)` internally because
`create_builtin_function_with_captures` lives on `JsEngine<T>`.

**Test-file-first:** Validate new generic patterns in
`content/src/generic_js_test.rs` on both backends before production code.
79/79 tests pass on Boa.

## Working during migration

**End-of-task override:** While working on Phase D–E migration, standard
verification steps (WPT, navigation verification, clippy, fmt) are
**skipped**.  Only `cargo check -p content` is required.  Full verification
resumes after Phase E.

**Update this README at end of every task.**  The remaining-phases table,
next-session order, ec_to_ctx counts, and phase status markers must reflect
current state.  This file is the canonical plan — it must never be stale.

**Prune the README.**  After every few sessions, remove or compress outdated
sections (completed phase details, stale examples, duplicated design notes,
dependency-order diagrams).  The README is a living plan, not a log.
