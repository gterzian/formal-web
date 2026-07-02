# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Architecture

> **Principle:** The architecture is defined by the standards.  We don't
> invent new layers — we follow the spec chain exactly and make it generic.

### 0. Migration methodology — spec-first, not Boa-first

When converting Boa-specific code to the generic layer, **follow the spec
chain**, not the Boa API shape:

1. **Read the spec algorithm.** Identify every ECMA-262 abstract operation
   it calls (Call, Get, PerformPromiseThen, NewPromiseCapability,
   CreateBuiltinFunction, etc.).

2. **Use the `ExecutionContext<T>` trait methods** that implement those
   ECMA-262 operations — never reach for Boa APIs when a generic equivalent
   exists.

3. **For promise chaining**, use `ec.perform_promise_then(promise, on_fulfilled,
   on_rejected, None)` — not `JsPromise::from_object(p)?.then(...)`.
```
   // ❌  Boa-specific (bypasses EC trait)
   let result = JsPromise::from_object(promise)?.then(Some(on_fulfilled), None, context)?;

   // ✅  Generic (spec: ECMA-262 PerformPromiseThen)
   let js_promise = Types::object_as_promise(&promise).ok_or_else(...)?;
   ec.perform_promise_then(js_promise, Some(on_fulfilled), None, None)?;
```

4. **For creating promises**, use `ec.new_promise_pending()` — not
   `JsPromise::new_pending(context)`.
```
   // ❌  Boa-specific
   let (promise, resolvers) = JsPromise::new_pending(context);

   // ✅  Generic (spec: ECMA-262 NewPromiseCapability)
   let (promise, resolvers) = ec.new_promise_pending()?;
```

5. **For domain struct field access**, use the `_ec`-suffixed methods
   (e.g., `stream.readable_ec(ec)`, `stream.controller_slot_ec(ec)`).
   These ARE the proper generic methods — the non-`_ec` variants
   (`stream.readable()`) are the legacy Boa wrappers.

6. **For domain functions that still take `&mut Context`**: convert them
   to take `&mut dyn ExecutionContext<T>` directly.  Do NOT create
   standalone `_ec` wrapper functions that bridge Context→EC.

7. **The ONLY place `ec_to_ctx` belongs** is inside fn pointer bodies
   (used with `builtin_with_captures`) where the fn pointer calls
   domain functions that haven't been converted to EC yet.  Each fn
   pointer should have at most ONE `ec_to_ctx` call at the top.

8. **Never create `_ec` wrapper functions** for standalone domain functions.
   These are indirection that just moves the bridge one level up.
   Convert the real function instead.

**Anti-patterns (do NOT do these):**
- Creating `xxx_ec()` wrapper functions that call `ec_to_ctx` internally
- Using `JsPromise::then()` when `perform_promise_then` exists on the trait
- Using `JsPromise::new_pending(context)` when `ec.new_promise_pending()` exists
- Using `JsNativeError::typ().with_message(msg)` when `ec.new_type_error(msg)` exists
- Using `completion_to_js_result` + `js_result_to_completion` bridges inside
  functions that should take EC directly

**Example — converting `transform_stream_default_sink_abort_algorithm`:**
```
   // ❌  Old: takes &mut Context, uses JsPromise::then, JsNativeError
   fn sink_abort(stream: TransformStream, reason: JsValue, ctx: &mut Context) -> JsResult<JsObject> {
       let controller = stream.controller_slot()?;        // JsResult
       let (promise, resolvers) = JsPromise::new_pending(ctx);
       let cancel_promise = ...;
       let on_fulfilled = builtin_with_captures(ctx, captures, fn_ptr, 0);
       let _ = JsPromise::from_object(cancel_promise)?.then(Some(on_fulfilled), Some(on_rejected), ctx)?;
       Ok(finish_promise)
   }

   // ✅  New: takes &mut dyn EC, uses perform_promise_then, new_promise_pending
   fn sink_abort(stream: TransformStream, reason: JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
       let controller = stream.controller_slot_ec(ec)?;   // Completion
       let (promise, resolvers) = ec.new_promise_pending()?;
       let cancel_promise = ...; // uses ec, not ctx
       let ctx = unsafe { ec_to_ctx(ec) };  // ONE bridge for builtin_with_captures
       let on_fulfilled = builtin_with_captures(ctx, captures, fn_ptr, 0);
       let on_rejected = builtin_with_captures(ctx, captures2, fn_ptr2, 1);
       drop(ctx);
       let js_promise = Types::object_as_promise(&cancel_promise).ok_or_else(...)?;
       ec.perform_promise_then(js_promise, Some(on_fulfilled), Some(on_rejected), None)?;
       Ok(finish_promise)
   }
```

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
(See Design notes → "Why `downcast_ref` on `JsObject` doesn't need `Context`" for more context.)

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

### `with_object_any` / `with_object_any_mut` are object-safe

Earlier versions took a generic closure parameter (`fn f: impl FnOnce(&dyn Any) -> R`)
which made them non-object-safe, requiring `Self: Sized`.  The current API returns
`Option<&dyn Any>` / `Option<&mut dyn Any>` directly — the caller downcasts.
This enables calling them on `&dyn ExecutionContext<T>`.

The Boa backend uses an unsafe lifetime extension because the `NativeDataWrapper`
lives inside the `JsObject` (GC heap rooted by `self`), not in `self` directly.

### Why `downcast_ref` on `JsObject` doesn't need `Context`

`JsObject::downcast_ref::<T>()` and `JsObject::downcast_mut::<T>()` are
`&self` methods on the Boa object — they don't take `Context`.  This means
binding functions that only do: (a) value-as-object upcast, (b) downcast to
domain type, (c) read a field from the domain type, (d) return a value via
`ec.value_from_*()` — need zero `ec_to_ctx` casts.  `new_type_error` on
`ExecutionContext<T>` replaces `JsNativeError` for error construction.

This eliminates `ec_to_ctx` from ~70% of typical binding function bodies
(the simple getter/setter pattern).  The remaining ~30% need `ctx` for
string extraction (`to_std_string_escaped`) or object construction
(`ObjectInitializer`, `JsArray`).

Full GC abstraction (trait-level `get_object_data`) is blocked by Boa's
`GcCell` returning `Ref<T>` guards, not `&T`.  This is resolvable but not
on the critical path for eliminating most `ec_to_ctx` calls.
(Duplicated above under "Platform object downcast without GC abstraction"
for discoverability — keep both in sync.)

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

### `create_builtin_function` barrier (resolved — Phase C complete)

Phase C moved `create_builtin_function` from `JsEngine<T>` to
`ExecutionContext<T>`.  The Web IDL bindings infra (`operation.rs`,
`attribute.rs`) and production binding files now call it on `ec` directly.
This eliminated all `NativeFunction::from_closure` + `FunctionObjectBuilder`
sites in the bindings layer.

**JSC backend:** implemented via a custom JSClass (`FormalWebBuiltin`) with
`callAsFunction` + `finalize` callbacks.  The behaviour closure is wrapped
to capture a raw engine pointer, boxed, and stored as private data on the
JSObject.  The `finalize` callback frees the Box on GC.

### `with_object_any_mut` and `with_object_any_mut_with`

`with_object_any_mut` returns `Option<&mut dyn Any>` — the returned
reference's (unsafely extended) lifetime borrows from `ec`, so no `ec`
method can be called while it's alive.  This is fine for simple get/set
patterns.

For patterns like `set_onload`, `play()`, `pause()`, `set_src()` where
mutation needs to call ECMA-262 operations, use **`with_object_any_mut_with`**
which passes both `&mut dyn Any` and `&mut dyn ExecutionContext<T>` to
a closure.  Both backends implement this safely via raw-pointer decoupling
(the native data lives in the GC heap / side-table, separate from `ec`).
Validated in `with_object_any_mut_with_ec_inside_closure` test.

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

| Phase | What | Status |
|---|---|---|
| 1. Trait split | `ExecutionContext<T>` split from `JsEngine<T>`. | ✅ |
| 2. Generic bindings | `OperationDef<T>`, `AttributeDef<T>`, `InterfaceDefinition<T>` generic. | ✅ |
| 3. EC infrastructure | `store_host_any`/`get_host_any`, `NativeDataWrapper`, `create_object_with_any`. | ✅ |
| 4. Generic registry | `InterfaceRegistry<T>` stores `T::JsObject`. | ✅ |
| 5. Binding fn signatures | All 26 binding files: `fn(..., &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>`. | ✅ |
| 6a. CtxHost removal | Adapters in `strategy.rs` and `readablestreamsupport.rs` removed. | ✅ |
| 6b. EDS context leak | `EventDispatchHost::context()` → `ec()`. | ✅ |
| 7. Domain threading | Domain methods take `&mut dyn ExecutionContext<T>`. | ✅ |
| 8. GC abstraction | `impl_gc_traits!` macro, `GcRootHandle<T>`, `create_root`. POC proven. | ✅ |
| 9. JSC backend | All trait methods implemented. 15/16 js_engine crate tests pass (JSC). Content generic_js_test: 70/70 pass on Boa, 1 `#[ignore]` on JSC. | ✅ |
| D. Dispatch host cleanup | `ContextEventDispatchHost` deleted from both locations. `EcDispatchHost` is sole dispatch host. | ✅ |
| S1. writablestream.rs bindings | 10 of 14 binding functions zero ec_to_ctx (8 remain). | ✅ |
| S2. readablestream.rs bindings | 33 → 2 ec_to_ctx. 26 of 28 functions zero ec_to_ctx. Only create_platform_object remains (construct_readable_stream takes &mut Context). | ✅ |
| S3. writablestream.rs bindings | 18 → **0 ec_to_ctx**. Fully converted. | ✅ |
| S4. transformstream.rs bindings | 7 → **0 ec_to_ctx**. Fully converted. Added `_ec` downcast helpers (`with_transform_stream_ref_ec`, `with_transform_stream_default_controller_ref_ec`), `_ec` JsResult wrappers (`readable_object_ec`, `writable_object_ec`, `desired_size_ec`), and `_ec` domain wrappers for `enqueue_ec`, `error_ec`, `terminate_ec`, `construct_transform_stream_ec`. | ✅ |
| S5. readablestream.rs constructor | 2 → **0 ec_to_ctx**. Added `construct_readable_stream_ec`. `create_platform_object` now a single-line delegation. | ✅ |
| S6. window.rs bindings | 11 → **0 ec_to_ctx**. Converted all 11 functions. `set_onload` uses inline `with_object_any_mut` + `try_with_event_target_mut`; `get_computed_style` uses `with_object_any` + `style_declaration_object_ec`. Added `downcast_window_ec`, used `callback_function_value_ec`, `nullable_value_ec`, `document_creation_url_ec`, `location_object_ec`, `store_location_object_ec`. | ✅ |
| S7. event_target.rs bindings + EcDispatchHost | 10 → **0 ec_to_ctx**. Converted `addEventListener`, `removeEventListener`, `dispatchEvent`, plus `document_object`, `resolve_element_object`, `resolve_existing_node_object` in EcDispatchHost. Added `current_event_target_object_ec`, `flatten_more_ec`. | ✅ |
| **S8. Controller JsResult → Completion** | Added `_ec` versions of `stream_slot`, `controller_object`, `invalidate_byob_request`, `should_call_pull`, `can_close_or_enqueue`, `get_desired_size`, `has_backpressure` across `readablestreamdefaultcontroller.rs`, `writablestreamdefaultcontroller.rs`, `transformstream.rs` (and `signal_ec` for writable). Converted callers in `close()`, `enqueue()`, `close_steps()`, `enqueue_steps()`, `error_steps()`, `pull_steps()`, `call_pull_if_needed()`, `error()`, `signal_abort()`, `signal_value_ec()`, `error_controller()`, `error_if_needed()`, `update_byob_request_view()`. Eliminated ~14 ec_to_ctx across controller files. `desired_size_ec` and `readable_object_ec`/`writable_object_ec`/`readable_ec`/`writable_ec`/`controller_slot_ec` on TransformStream now direct `Completion` (no `ec_to_ctx` bridge). | ✅ |
| **S9. PromiseResolvers&lt;T&gt; in js_engine** | Added `PromiseResolvers<T>` record type with `resolve(value, ec)` and `reject(reason, ec)` methods that call `ec.call()` directly. Added `new_promise_pending()` trait method on `ExecutionContext<T>` returning `(T::JsValue, PromiseResolvers<T>)`. Implemented for Boa (wraps `JsPromise::new_pending` + `.into()` conversion) and JSC (reuses `new_promise_capability`). | ✅ |
| **S10. PromiseResolvers content migration** | Replaced `ResolvingFunctions` with `PromiseResolvers<crate::js::Types>` in `WritableStreamDefaultWriter` (ready_resolvers, closed_resolvers), `ReadableStreamDefaultReader` (closed_resolvers), `ReadableStreamBYOBReader` (closed_resolvers), `TransformStream` (backpressure_change_resolvers), `TransformStreamDefaultController` (finish_resolvers), `ReadRequest::DefaultReaderRead`, `ReadIntoRequest`, `WriteRequest`, `PendingAbortRequest`, and the `ReadableStreamGenericReader` trait. Converted `JsPromise::new_pending(context)` → `ec.new_promise_pending()?` and `resolvers.resolve.call(...)` → `resolvers.resolve(value, ec)?`. Eliminated ~6 ec_to_ctx (writablestreamdefaultwriter: 2→0, readablestreamdefaultreader: 2→0, writablestream: 4→1). | ✅ |
| P1. platform_objects `_ec` wrappers | Added `location_object_ec`, `store_location_object_ec`. | ✅ |
| P2. readablebytestreamcontroller.rs | Added `_ec` wrappers for `stream_slot`, `controller_object`, `invalidate_byob_request`, `should_call_pull`. Eliminated ec_to_ctx from `cancel_steps`. | ✅ |
| P3. platform_objects ec_to_ctx consolidation | `platform_objects.rs`: 8 → **0 ec_to_ctx**. Added `realm_global_object()` trait method on `ExecutionContext` (§8.1.3 `[[GlobalObject]]`). `with_global_scope_ec` uses `ec.realm_global_object()` → `ec.with_object_any()` → `downcast_ref::<Window>()` — pure trait-method access, no `ec_to_ctx`. Simple `_ec` wrappers pre-create errors to avoid borrow conflicts; complex wrappers use block scoping. Tested: `realm_global_object_returns_valid_js_object`, `host_any_stored_object_downcast_via_with_object_any`. | ✅ |
| **T1. Typed array trait methods** | Added 11 new trait methods to `ExecutionContext<T>`: `typed_array_buffer`, `typed_array_byte_offset`, `typed_array_byte_length`, `typed_array_element_type`, `construct_typed_array_view`, `data_view_buffer`, `data_view_byte_offset`, `data_view_byte_length`, `construct_data_view_from_buffer`, `array_buffer_data`. Boa backend full, JSC stubs. 3 new tests. | ✅ |
| **T2. Typed array caller conversion** | Converted all streams callers to use new trait methods: `ArrayBufferViewDescriptor::from_value`, `create_result_view`, `create_remaining_view`, `create_view_object`, `create_typed_array_view_object`, `create_uint8_view_object`, `clone_as_uint8_array`. Eliminated ~13 ec_to_ctx from `readablebytestreamcontroller.rs` and `readablestreambyobreader.rs`. | ✅ |
| **W1. WebIDL promise conversion** | Converted `resolved_promise`, `rejected_promise`, `promise_from_value`, `transform_promise_to_undefined`, `mark_promise_as_handled` to use existing trait methods (`new_promise_capability` + `Call`, `create_builtin_function` + `perform_promise_then`). Deleted dead `a_new_promise`. 9→3 ec_to_ctx in `promise.rs`. | ✅ |
| **W2. Streams helpers conversion** | Converted `create_read_result` (ObjectInitializer→`create_plain_object`+`object_set_property`), `type_error_value`, `range_error_value` in `readablestreamsupport.rs`. Converted `get_callable_method` in `writablestreamdefaultcontroller.rs`. | ✅ |
| **G1. `#[gc_struct]` proc-macro attribute** | Created `js_engine_macros` proc-macro crate with `#[gc_struct]` attribute.  Replaces `impl_gc_traits!` across all 34 struct/enum definitions in `content/`.  Re-exported as `js_engine::gc_struct`. | ✅ |
| **G2. `GcCell<T>` type alias** | Added `GcCell<T>` (Boa: `Gc<GcRefCell<T>>`, JSC: `Rc<RefCell<T>>`) with `gc_cell_new()` constructor.  `Trace` made a supertrait of `boa_gc::Trace` on Boa so the type alias interoperates with GC trait bounds. | ✅ |
| **C2. `create_builtin_function_with_captures`** | Added to `JsEngine<T>` — accepts a traceable captures struct + fn pointer.  Boa uses the safe `from_copy_closure_with_captures`; JSC moves captures into `StoredBehaviour`.  Two tests. | ✅ |
| **C3. NativeFunction → captures migration** | 16 NativeFunction sites converted across `writablestreamdefaultcontroller.rs` (6), `readablestreamdefaultcontroller.rs` (4), `readablebytestreamcontroller.rs` (4).  Shared `crate::js::builtin_with_captures` helper.  24 remaining. | ✅ |
| **G3. `#[gc_struct]` emits `Clone`** | `#[gc_struct]` always emits `derive(Clone)`; 23 redundant `#[derive(Clone)]` lines removed; doc comments reordered above the attribute.  `GcRootHandle<T>`, `GlobalScope`, `PendingRequest`, `CachedNodeObject`, `AnimationFrameCallback` made `Clone`. | ✅ |

### Remaining phases

Six architectural blockers remain.  The phases below map to them.
**Every phase that introduces a
new generic interface must validate it in `content/src/generic_js_test.rs`
first** (see test-file-first discipline above).

| Blocker | Phase | What | Effort | Status |
|---|---|---|---|---|
| **Blocker 1** — Dispatch result-model mismatch | **Phase D** | Convert `EventDispatchHost` trait methods from `JsResult` to `Completion`. Delete `ContextEventDispatchHost` (both copies). Eliminate `js_result_to_completion` bridges from the dispatch path. | Small | ✅ Done — `EcDispatchHost` is the sole dispatch host; `ContextEventDispatchHost` deleted from both locations. |
| **Blocker 4** — Streams domain exposes `Context` | **Phase S** | Convert streams domain methods from `&mut Context` to `&mut dyn ExecutionContext<T>`. **Bindings complete** — all streams binding files at 0 ec_to_ctx. **Typed array operations converted** — 11 new trait methods added, all callers converted. ~70 domain-internal calls remain (stream_slot/controller_object JsResult methods, NativeFunction closures, microtask/tee); require per-function borrow-checker analysis (call `_ec` methods before `ec_to_ctx`). | Large | 🔶 Bindings + typed arrays done. Domain-internal in progress. |
| **Blocker 2** — Platform-object state through Boa access paths | **Phase P** | Create content-owned host-data-backed store for platform-object bookkeeping, OR add `_ec` wrappers for remaining `&Context`-taking functions. `store_host_any` / `get_host_any` already validated. `realm_global_object()` trait method on `ExecutionContext` provides generic access to the global object (§8.1.3). `with_global_scope_ec` in `platform_objects.rs` combines `realm_global_object()` + `with_object_any` + `downcast_ref::<Window>()` — zero `ec_to_ctx`. WindowProxy needs `JsProxyBuilder` which has no trait equivalent yet — may need `create_proxy` on `ExecutionContext`. | Medium | 🔶 platform_objects.rs 8→0 ec_to_ctx. Remaining: abort.rs (3), windowproxy.rs (2), singletons (2). |
| **Blocker 5** — Subsystem entry points assume Boa | **Phase W** | Convert structured clone, Web IDL promise helpers, async iterable helpers, and Wasm to take `ExecutionContext<T>`. Same `_ec` wrapper pattern as Phase S/P — no new generic interfaces needed. `buffer_source.rs` now covered by typed array trait methods (T1). | Medium | 🔶 promise.rs 9→3. Remaining: JsError helpers (3), structured clone (1), async iterable (1), wasm (6), windowproxy (2). |
| **Blocker 3** — Engine ownership is structurally Boa-specific | **Phase E** | Land compile-time `Types` / `Engine` aliases. Backend selection becomes a `#[cfg]` choice. Validated by `cargo check` with both feature sets. | Large | Blocked on D, S, P, W |
| **Blocker 6** — Global-scope helpers are implicitly Boa | **Phase G** | Move `document_creation_url`, `with_global_scope`, etc. behind content-owned query helpers. | Small | Part of Phase P |

**Completed phases:**

| # | Phase | Effort | Status |
|---|---|---|---|
| **A. GC derive conversion** | Replace Boa derives with `impl_gc_traits!` on 34 types | Small | ✅ DONE |
| **B. Binding body conversion** | Replace ~197 `ec_to_ctx` across binding files with `ec.with_object_any()` + `ec.to_rust_string()` patterns | Medium | ✅ DONE — all binding files at 0 ec_to_ctx. ~197 eliminated across 14+ files. |
| **C. create_builtin_function on EC** | Moved `create_builtin_function` from `JsEngine` to `ExecutionContext`, replaced `NativeFunction::from_closure` + `FunctionObjectBuilder` in `strategy.rs`. All Web IDL infra callers updated. | Medium | ✅ DONE |
| **H. JSC content tests** | Enable 5 `#[ignore]` tests | Medium | Blocked on E |

### Dependency order

```
Phase S (streams domain) ──► Phase P (platform-object store)
                                  │
                                  ├──► Phase G (global-scope helpers)
                                  │
                          Phase W (subsystem entry points)
                                  │
                                  └──► Phase E (conditional Types) ──► Phase H (JSC tests)
```

**Why this order:**
1. ~~The dispatch mismatch is the smallest remaining cross-cutting blocker — fix it first.~~ ✅ DONE.
2. ~~Streams is the largest concentration of remaining backend coupling — bindings are done.~~ Domain-internal ~136 remain but are now the long tail rather than the blocker.
3. Platform-object state (Phase P) and subsystem entry points (Phase W) are the next blockers — unblock the remaining ~33 non-streams ec_to_ctx.
4. Backend alias lands once Phases P, W, and S are complete.

### Current state (updated 2026-07-02 — NativeFunction closures converted across controller files)

**Phases A–D, S1–S10, T1–T2, W1–W2, G1–G3, C2–C3 complete.** All binding files at 0 ec_to_ctx. All 34 struct/enum definitions use `#[gc_struct]` (now emits `Clone` automatically). All domain field types use `GcCell<T>`.

**POC test suite: 79/79 pass on Boa.**

**New this session:**

| Addition | What |
|---|---|
| `#[gc_struct]` emits `Clone` | All structs and enums get `Clone` from the attribute; 23 redundant `#[derive(Clone)]` lines removed; doc comments now above `#[gc_struct]` |
| `GcCell<T>` migration | All 13 domain files: `boa_gc::{Gc, GcRefCell}` → `js_engine::gc::{GcCell, gc_cell_new}` |
| `crate::js::builtin_with_captures` | Shared helper in `content/src/js/mod.rs` wrapping `create_builtin_function_with_captures` |
| NativeFunction → captures | 16 sites converted across 3 controller files (writable, readable, byte). 24 remaining in transformstream + readablestream (SourceMethod-wrapped). |

**~62 ec_to_ctx remain in streams/** (no change — captures conversion eliminates Boa imports but fn bodies still use `ec_to_ctx`).

**New this session:**

| Addition | What |
|---|---|
| Deep call-chain conversion | Converted 7 domain functions to take `&mut dyn ExecutionContext<T>` directly: `transform_stream_set_backpressure`, `transform_stream_unblock_write`, `transform_stream_error`, `transform_stream_error_writable_and_unblock_write`, `transform_stream_default_controller_error`, `transform_stream_default_sink_abort_algorithm`, `transform_stream_default_controller_terminate`. Deleted corresponding `_ec` wrapper functions. |
| Struct method conversion | `TransformStreamDefaultController::error()` and `::terminate()` now take EC directly (deleted `error_ec`/`terminate_ec` bridges). Updated all callers across bindings and streams. |
| `perform_promise_then` usage | `transform_stream_default_sink_abort_algorithm` now uses `ec.perform_promise_then()` + `ec.new_promise_pending()` instead of `JsPromise::then()` + `JsPromise::new_pending(context)`. |
| Methodological docs | Added §0 "Migration methodology — spec-first, not Boa-first" to this README. |

**Key lesson — deep, not broad:** When converting a function to EC, trace the ENTIRE call chain (across files if needed) and convert everything that function calls. Never leave bridges (`context_as_ec`, `_ec` wrappers, `completion_to_js_result` bridges) at the boundaries. If a called function still needs Context, convert it too. This is deep-call-chain migration, not file-by-file migration.

### Next session: recommended order

1. **Convert remaining NativeFunction closures** — 24 sites in `transformstream.rs` and `readablestream.rs`.  These are `SourceMethod`/`Callback`-wrapped; need a `builtin_callback` helper that returns `Callback` directly.
2. **Add `queue_microtask` trait method** — wraps `ec.enqueue_job()` with a closure that receives `&mut dyn ExecutionContext<T>` instead of `&mut Context`.  Eliminates ~24 more ec_to_ctx from tee and queue operations.
3. **Phase E — Conditional Types alias** — once remaining ~62 ec_to_ctx are covered, `#[cfg]` gate all Boa imports.

### Working note: `create_builtin_function_with_captures` pattern

For plain promise `.then()` handlers, use `crate::js::builtin_with_captures(context, captures, fn_ptr, length)`.  For `SourceMethod`-wrapped closures, add a similar `builtin_callback` helper that wraps the fn in `Callback::from_object(fn.into())`.

### Working note: `ec_to_ctx` after `ec_to_ctx`

When a function has `let context = unsafe { ec_to_ctx(ec) };`, you CANNOT call `ec.method()` afterward because `context` is a borrow of the underlying pointer. But you CAN call `ec.method()` BEFORE `ec_to_ctx` — the borrow from `ec.method()` ends at the semicolon. Pre-create errors and call `ec.call()` before the `ec_to_ctx` line.

### Borrow-checker conflict: `ec_to_ctx` vs `_ec` methods

`ec_to_ctx` takes `&mut dyn ExecutionContext`, and so does every `_ec`
method.  In a function that already has `let context = unsafe { ec_to_ctx(ec) };`,
calling `self.something_ec(ec)` afterwards is a second mutable borrow of `ec`
— the Rust borrow checker rejects it.  This is the fundamental blocker for
the ~62 domain-internal conversions.

**Three workarounds, in order of preference:**

1. **Reorder — call `_ec` before `ec_to_ctx`.**  The borrow from
   `self.something_ec(ec)?` ends at the semicolon.  Then `ec_to_ctx(ec)`
   starts a fresh borrow.  Works when the `_ec` result doesn't depend on
   `context`-derived values.

2. **Pre-create error values.**  `let err = ec.new_type_error("msg");` is a
   `Completion<!, T>` — an owned value that doesn't borrow `ec`.  Store it
   before calling `ec.with_object_any(&obj)`, then use it in the `None`
   branch as `return Err(err)`.  Used in `set_onload` and `get_computed_style`.

3. **Block-scope the `ec` borrow.**  Wrap `ec.with_object_any(&obj)` in a
   block, extract what you need (clone, compute owned data), end the block
   (releasing the borrow), then use `ec` again.  Used in
   `get_computed_style` to extract element properties before calling
   `style_declaration_object_ec`.

None of these requires new generic interfaces — they're all established
patterns already validated in `generic_js_test.rs`.

### Working note: `ec_to_ctx` after `ec_to_ctx`

When a function has `let context = unsafe { ec_to_ctx(ec) };`, you CANNOT call `ec.method()` afterward because `context` is a borrow of the underlying pointer. But you CAN call `ec.method()` BEFORE `ec_to_ctx` — the borrow from `ec.method()` ends at the semicolon. Pre-create errors and call `ec.call()` before the `ec_to_ctx` line.

### Borrow-checker conflict: `ec_to_ctx` vs `_ec` methods

`ec_to_ctx` takes `&mut dyn ExecutionContext`, and so does every `_ec`
method.  In a function that already has `let context = unsafe { ec_to_ctx(ec) };`,
calling `self.something_ec(ec)` afterwards is a second mutable borrow of `ec`
— the Rust borrow checker rejects it.  This is the fundamental blocker for
the ~136 domain-internal conversions.

**Three workarounds, in order of preference:**

1. **Reorder — call `_ec` before `ec_to_ctx`.**  The borrow from
   `self.something_ec(ec)?` ends at the semicolon.  Then `ec_to_ctx(ec)`
   starts a fresh borrow.  Works when the `_ec` result doesn't depend on
   `context`-derived values.

2. **Pre-create error values.**  `let err = ec.new_type_error("msg");` is a
   `Completion<!, T>` — an owned value that doesn't borrow `ec`.  Store it
   before calling `ec.with_object_any(&obj)`, then use it in the `None`
   branch as `return Err(err)`.  Used in `set_onload` and `get_computed_style`.

3. **Block-scope the `ec` borrow.**  Wrap `ec.with_object_any(&obj)` in a
   block, extract what you need (clone, compute owned data), end the block
   (releasing the borrow), then use `ec` again.  Used in
   `get_computed_style` to extract element properties before calling
   `style_declaration_object_ec`.

None of these requires new generic interfaces — they're all established
patterns already validated in `generic_js_test.rs`.

### Phase B strategy: test-file-first workflow

**Never add a new generic pattern directly to production code.**
Every downcast helper, binding-function signature, or data-access
abstraction must first be validated in `content/src/generic_js_test.rs`
with compilation and passing unit tests on **both backends** (Boa and
JSC) before it can be applied to any real binding file.

This means: before converting a binding file, check whether the generic
test file already covers the patterns that file needs.  If not, add a
minimal test first (compiles + passes), then apply the proven pattern.

**Patterns already validated in the test file:**

| Pattern | Test file reference | Production equivalent |
|---|---|---|
| Single-type downcast (immutable) | `widget_data::with_ref` | `try_with_*_ref` in `downcast.rs` or local helpers |
| Single-type downcast (mutable) | `widget_data::with_mut` | `try_with_*_mut` in `downcast.rs` |
| Multi-type downcast chain (immutable) | `widget_or_button_with_ref` | `try_with_node_ref`, `try_with_html_element_ref`, etc. |
| Multi-type downcast chain (mutable) | `widget_or_button_with_mut` | `try_with_event_target_mut` (future) |
| Platform object creation | `create_test_widget`, `create_interface_instance_roundtrip` | `create_interface_instance` |
| Mutable downcast + ec call | `with_object_any_mut_with_ec_inside_closure` | `set_onload`, `set_src`, `play`, `pause` |
| PropertyDescriptor with getter | `property_descriptor_with_builtin_getter` | `get_class_list` length getter |
| PropertyDescriptor with getter+setter | `property_descriptor_with_builtin_getter_and_setter` | Accessor pattern |
| String extraction | `ec.to_rust_string(v)` | Direct use in binding functions |
| Value construction | `ec.value_from_string(...)`, etc. | Direct use in binding functions |
| Error construction | `ec.new_type_error(msg)` | Direct use in binding functions |

**Conversion recipe for a binding file:**

1. Rewrite its local `try_with_*` helpers to use `ec.with_object_any()` /
   `ec.with_object_any_mut()` + `dyn Any::downcast_ref()` /
   `downcast_mut()`, following the proven multi-type-chaining pattern
   from the test file.
2. Replace `JsNativeError::typ().with_message(...)` with
   `ec.new_type_error(...)`.
3. Replace `.to_string(ctx)?.to_std_string_escaped()` with
   `ec.to_rust_string(v)?`.
4. Replace `JsValue::undefined()` with `ec.value_undefined()`, etc.
5. Functions that still need `ctx` for Boa-specific APIs
   (`ObjectInitializer`, `NativeFunction`, `FunctionObjectBuilder`,
   `document_creation_url`, etc.) keep
   `let ctx = unsafe { ec_to_ctx(ec) };` but flatten the
   `(|| -> JsResult<...> { ... })() .map_err(...)` bridge — unwrap the
   body and add explicit
   `.map_err(|e| e.into_opaque(ctx).unwrap_or(undefined))?`
   at each `JsResult`-returning call.
6. Delete the old `with_*` helper if no callers remain.

**`with_object_any_mut` borrow-limitation (resolved):**
Use `with_object_any_mut_with` (closure-based) for patterns where
mutation needs to call ECMA-262 operations.  It passes both
`&mut dyn Any` and `&mut dyn ExecutionContext<T>` to the closure,
eliminating the borrow conflict.  Validated in
`with_object_any_mut_with_ec_inside_closure`.

**What NOT to do:**

- Do NOT add new `try_with_*` helpers that use Boa's
  `JsObject::downcast_ref::<T>()` / `downcast_mut::<T>()`.  Use
  `ec.with_object_any()` / `ec.with_object_any_mut()` instead — that is
  the generic, cross-engine equivalent validated in the test file.
- Do NOT convert a file without first checking that the test file covers
  the patterns it needs.  Gaps in test coverage must be filled first.
- Do NOT add new Boa-specific bridge functions when a generic equivalent
  exists.  For platform-object downcast, the generic equivalent is
  `ec.with_object_any()` / `ec.with_object_any_mut()`.  For
  document-scope helpers (`document_object`, `object_for_existing_node`,
  etc.) no generic equivalent exists yet — `_ec` wrappers in
  `platform_objects.rs` are acceptable bridges until Phase F makes
  `EnvironmentSettingsObject` generic.

### POC test file — reference implementation

`content/src/generic_js_test.rs` is the **reference implementation** for the
generic layer.  Every generic pattern must be validated here before being
applied to production binding files.  When converting real code, use the
test file as the template:

- **Struct with GC fields**: `#[gc_struct]` with `GcRootHandle<Types>` for JS references, `GcCell<T>` for GC-managed interior mutability
- **Binding function**: `fn(&Types::JsValue, &[Types::JsValue], &mut dyn
  ExecutionContext<Types>) -> Completion<Types::JsValue, Types>` with
  `widget_data::with_ref`/`with_mut` for domain access
- **Platform object creation**: `create_interface_instance` (canonical
  path) or `ec.create_object_with_any(prototype, Box::new(data))` (direct)
- **Callback storage**: `ec.create_root(&callback_val)` → store as
  `GcRootHandle<Types>`
- **Multi-type downcast chain**: `widget_or_button_with_ref` /
  `widget_or_button_with_mut` — tries `TestButton` first, falls back to
  `TestWidget`, demonstrating the same pattern as `try_with_node_ref`
  (tries `Document`, `Element`, `HTMLElement`, …, `Node`) or
  `try_with_event_target_mut` (tries 12 types including `Window`,
  `Document`, …, `EventTarget`).  Uses
  `ec.with_object_any()` / `ec.with_object_any_mut()` + Rust's
  `dyn Any::downcast_ref()` / `downcast_mut()` — no Boa-specific APIs.

`create_test_widget` / `create_test_button` delegate to
`create_interface_instance` — the same canonical path used by
DOMException, Event, and Location in production.

**Split recommendation:** The file currently serves two roles:
(a) binding-pattern reference implementation via `TestWidget`/`TestButton`,
and (b) standalone ECMA-262 operation smoke tests (`json_stringify_roundtrip`,
`bigint_roundtrip`, array-buffer tests, iterator tests, `species_constructor`,
etc.).  These should be split into `generic_js_test.rs` (binding patterns
only — the template for other binding files) and `ecma_ops_test.rs`
(standalone ECMA-262 operation smoke tests).  No behavior change — just
keeps the reference file legible as a template.

70/70 tests pass on Boa in `content/src/generic_js_test.rs`.
1 test is `#[ignore]` on JSC:

| Test | JSC blocker |
|---|---|
| `allocate_shared_array_buffer` | May not be available on current macOS |

## Working during migration

**End-of-task override:**  While working on Phase D–E migration (dispatch
result-model, streams domain, platform-object store, subsystem entry points,
conditional Types), the standard end-of-task verification steps (WPT,
navigation verification, clippy, fmt) are **skipped**.  Only
`cargo check -p content` is required to validate each change.  Full
verification resumes after Phase E is complete.

**Test-file-first gate:**  Phases P and W introduce new generic interfaces.
Before those phases can mark complete, each new interface must have a passing
test in `content/src/generic_js_test.rs` on the Boa backend.  Phases D, S,
and E are return-type-only or configuration-only changes — validated by
`cargo check` passing.

**Update this README at end of every migration task.**  The remaining-phases
table, next-session order, ec_to_ctx counts, and phase status markers must
reflect current state after every session.  This file is the canonical plan;
it must never be stale.
