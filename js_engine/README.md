# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Architecture

> **Principle:** The architecture is defined by the standards.  We don't
> invent new layers — we follow the spec chain exactly and make it generic.

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
| Native function registration (`NativeFunction::from_closure`) | `create_builtin_function` on `JsEngine<T>` is the spec-correct equivalent, but binding functions only have `&mut dyn ExecutionContext<T>`. Phase C will either move it to `ExecutionContext<T>` or add an `engine()` accessor. |
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
  gc.rs         Trace, Finalize, GcRootHandle, impl_gc_traits! macro
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

## Generic API surface (proven in POC)

The `generic_js_test.rs` POC proves every content pattern can be expressed
through the generic API.  See the test file for working examples of each.

### Platform object lifecycle

| Operation | Trait method | POC example |
|---|---|---|
| Create object with native data | `ec.create_object_with_any(prototype, Box::new(data))` | `create_test_widget` |
| Read native data (immutable) | `ec.with_object_any(obj) -> Option<&dyn Any>` | `widget_data::with_ref` |
| Read native data (mutable) | `ec.with_object_any_mut(obj) -> Option<&mut dyn Any>` | `widget_data::with_mut` |

`with_object_any` and `with_object_any_mut` are object-safe — callable on
`&dyn ExecutionContext<T>`.  They return typed references that the caller
downcasts via `dyn Any::downcast_ref::<T>()` / `downcast_mut::<T>()`.

### GC integration

| Operation | Mechanism | POC example |
|---|---|---|
| GC trait derivation | `impl_gc_traits!` declarative macro | `TestWidget` struct |
| Store a JS callback | `Option<GcRootHandle<Types>>` field | `on_change` field |
| Root a JS value | `ec.create_root(&value) -> GcRootHandle<T>` | `store_callback` |

`impl_gc_traits!` expands to:
- Boa: `#[derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)]`
- JSC: no-op `Trace` and `Finalize` impls

`GcRootHandle<T>` is an RAII guard:
- Boa: no-op (GC traces through `boa_gc::Trace` on the handle itself)
- JSC: calls `JSValueProtect` on construction, `JSValueUnprotect` on drop
  (**currently SIGSEGVs on eval-created values — release blocker**)

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
| JSC | `src/jsc/mod.rs` | 🔶 Trait surface complete. `create_builtin_function` implements behaviour closures via JSClass + private data. `create_root` uses global-object properties instead of `JSValueProtect`. `get` handles Symbol keys via eval fallback. 1 remaining ignore: `SharedArrayBuffer` (may not be available). |
| GC | `src/gc.rs` | ✅ Complete — `impl_gc_traits!` macro, `GcRootHandle<T>` with Boa trace impl, `create_root` on EC trait. GC-pressure testing gap: no test forces a collection to prove rooted values survive. |

## Migration status

POC is **complete** — 70/70 tests pass on Boa (content JSC blocked on Phase E).
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
| 9. JSC backend | All trait methods implemented. 15/16 js_engine tests pass. | ✅ |
| D. Dispatch host cleanup | `ContextEventDispatchHost` deleted from both locations. `EcDispatchHost` is sole dispatch host. | ✅ |
| S1. writablestream.rs bindings | 10 of 14 binding functions zero ec_to_ctx (8 remain). | ✅ |
| S2. readablestream.rs bindings | 33 → 2 ec_to_ctx. 26 of 28 functions zero ec_to_ctx. Only create_platform_object remains (construct_readable_stream takes &mut Context). | ✅ |
| S3. writablestream.rs bindings | 18 → **0 ec_to_ctx**. Fully converted. | ✅ |

### Remaining phases

Six architectural blockers remain.  The phases below map to them.
**Every phase that introduces a
new generic interface must validate it in `content/src/generic_js_test.rs`
first** (see test-file-first discipline above).

| Blocker | Phase | What | Effort | Status |
|---|---|---|---|---|
| **Blocker 1** — Dispatch result-model mismatch | **Phase D** | Convert `EventDispatchHost` trait methods from `JsResult` to `Completion`. Delete `ContextEventDispatchHost` (both copies). Eliminate `js_result_to_completion` bridges from the dispatch path. | Small | ✅ Done — `EcDispatchHost` is the sole dispatch host; `ContextEventDispatchHost` deleted from both locations. |
| **Blocker 4** — Streams domain exposes `Context` | **Phase S** | Convert streams domain methods from `&mut Context` to `&mut dyn ExecutionContext<T>`. ~208 remaining `ec_to_ctx` calls across all files (down from ~300 at start). | Large | 🔶 In progress — `_ec` downcast helpers for all 9 streams types. `_ec` domain wrappers for pipe_through, pipe_to, tee, from_iterable. `_ec` JsResult wrappers for desired_size, closed, ready, signal_value. writablestream.rs: **0 ec_to_ctx** ✅. readablestream.rs: 33→2. |
| **Blocker 2** — Platform-object state through Boa access paths | **Phase P** | Create content-owned host-data-backed store for platform-object bookkeeping (document object, node caches, animation-frame queues). Uses `store_host_any` / `get_host_any` (already validated). New helpers must be validated in test file first. | Medium | Not started |
| **Blocker 5** — Subsystem entry points assume Boa | **Phase W** | Convert structured clone, Web IDL promise helpers, async iterable helpers, and Wasm to take `ExecutionContext<T>`. Each entry point must be validated in test file first. | Medium | Not started |
| **Blocker 3** — Engine ownership is structurally Boa-specific | **Phase E** | Land compile-time `Types` / `Engine` aliases. Backend selection becomes a `#[cfg]` choice. Validated by `cargo check` with both feature sets. | Large | Blocked on D, S, P, W |
| **Blocker 6** — Global-scope helpers are implicitly Boa | **Phase G** | Move `document_creation_url`, `with_global_scope`, etc. behind content-owned query helpers. | Small | Part of Phase P |

**Completed phases:**

| # | Phase | Effort | Status |
|---|---|---|---|
| **A. GC derive conversion** | Replace Boa derives with `impl_gc_traits!` on 34 types | Small | ✅ DONE |
| **B. Binding body conversion** | Replace ~197 `ec_to_ctx` across binding files with `ec.with_object_any()` + `ec.to_rust_string()` patterns | Medium | 🔶 ~90% done. ~130 ec_to_ctx eliminated across 14 files. ~65 remaining in binding files. Domain helpers (`timer_handler_ec`, `timeout_ms_ec`) now use generic EC trait. |
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
2. Streams is the largest concentration of remaining backend coupling — in progress (bindings near-complete).
3. Platform-object state is the main content-owned state hole — then content's own state is generic.
4. Remaining subsystem entry points (structured clone, Web IDL promise, Wasm) — the long tail.
5. Backend alias lands only once most direct backend dependencies are already gone.

### Current state (updated 2026-07-01)

**Phases A–D, S (bindings) near-complete.**  `create_builtin_function` on
`ExecutionContext`, `_ec` downcast helpers for all 9 streams types, `_ec`
domain wrappers for pipe_through/pipe_to/tee/from_iterable, `_ec` JsResult
wrappers for desired_size/closed/ready/signal_value.

**POC test suite: 70/70 pass on Boa.**  All production-relevant patterns are
now validated in `content/src/generic_js_test.rs`.

**~208 ec_to_ctx remain across all files** (down from ~245 at start of Phase D/S, ~300 originally).

**Streams binding files converted:**

| File | Before | After | Status |
|---|---|---|---|
| `writablestream.rs` | 18 | **0** | ✅ Fully converted |
| `readablestream.rs` | 33 | 2 | Only `create_platform_object` (construct_readable_stream takes &mut Context) |
| `transformstream.rs` | 14 | 14 | Not yet started |
| `strategy.rs` | 0 | 0 | Already generic |

**Other fully converted binding files (0 ec_to_ctx):** `document.rs`,
`location.rs`, `node.rs`, `element.rs`, `html_anchor_element.rs`,
`html_media_element.rs`, `html_iframe_element.rs`, `abort_controller.rs`,
`html_element.rs`, `dispatch.rs`, `html/window.rs`,
`window_or_worker_global_scope.rs`.

**Domain helpers converted to generic EC trait:** `timer_handler_ec`,
`timeout_ms_ec`, `signal_abort_ec`.

`call_user_objects_operation` now takes `&mut dyn ExecutionContext<T>` and
returns `Completion`.
`the_rules_for_choosing_a_navigable` now takes `Option<JsObject>` instead of
`Option<&mut Context>`.

### Remaining ec_to_ctx blockers (prioritized)

| Blocker | Files | ~Count | What's needed |
|---|---|---|---|
| **Streams domain methods** | streams/*.rs | ~170 | Add `_ec` entry points returning `Completion` for internal domain-to-domain calls. Bindings layer is near-complete. |
| **Streams bindings — transformstream** | js/bindings/streams/transformstream.rs | 14 | Convert to use `_ec` helpers and `_ec` domain wrappers (pattern proven in writablestream.rs and readablestream.rs). |
| **Streams bindings — readablestream constructor** | js/bindings/streams/readablestream.rs | 2 | `create_platform_object` needs `construct_readable_stream_ec`. |
| **Window bindings** | window.rs (bindings) | 11 | `set_timeout`, `clear_timeout`, `location_object` — need `_ec` domain helpers. |
| **WebIDL promise** | promise.rs | 10 | Internal `ec_to_ctx` bridges for Boa promise APIs (Phase W). |
| **Wasm** | namespace.rs | 12 | Internal `ec_to_ctx` bridges (Phase W). |
| **platform_objects bridges** | platform_objects.rs | 7 | `_ec` wrappers use ec_to_ctx internally; need Phase P content-owned store. |
| **EventTarget bindings** | event_target.rs (bindings) | 3 | `addEventListener`, `removeEventListener` need `_ec` domain helpers. |
| **Structured clone** | safe_passing_of_structured_data.rs | 1 | 500-line function takes `&mut Context` (Phase W). |
| **document_creation_url** | hyperlink_element_utils.rs | 1 | Needs Phase P global-scope accessor. |
| **with_global_scope** | html_media_element.rs | 1 | Needs Phase P host-data accessor. |
| **PipeToState** | abort.rs | 1 | Streams `run_abort_algorithm` takes `&mut Context` (Phase S). |

### Next session: recommended order

1. **Phase S — transformstream.rs bindings (14 ec_to_ctx)** — Convert the
   last streams binding file using the proven pattern: `_ec` downcast helpers
   + `_ec` domain wrappers for JsResult methods.  Should be straightforward
   given the pattern is now proven in `writablestream.rs` (0 ec_to_ctx) and
   `readablestream.rs` (2 ec_to_ctx).

2. **Phase S — readablestream constructor** — Add `construct_readable_stream_ec`
   (wraps the Context-taking original) to eliminate the last 2 ec_to_ctx in
   `readablestream.rs`.  Same pattern as `readable_stream_from_iterable_ec`.

3. **Phase S — Domain-internal ec_to_ctx (~170)** — For each `js_result_to_completion`
   call in streams domain files, replace with `_ec` domain wrappers or direct
   `ec`-based ECMA-262 calls.  This is mechanical but large volume.  Focus on
   the files with the highest counts: `readablebytestreamcontroller.rs` (24),
   `readablestreamsupport.rs` (16), `writablestreamdefaultcontroller.rs` (15),
   `readablestreamdefaultcontroller.rs` (14), `readablestream.rs` domain (4),
   and the reader/writer files (~10 each).

4. **Phase P — Platform-object host-data store** — Replace
   `platform_objects.rs` with content-owned helpers on `store_host_any` /
   `get_host_any`.  Validate in `generic_js_test.rs` first.  Also covers
   Blocker 6 (global-scope query helpers).

5. **Phase W — Remaining subsystem entry points** — Structured clone, Web IDL
   promise helpers, async iterable helpers, and Wasm.  Validate each in
   `generic_js_test.rs` first.

6. **Phase E — Conditional Types alias** — `#[cfg]` gate all Boa imports.
   Blocked on near-zero ec_to_ctx in binding/domain files.

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

- **Struct with GC fields**: `impl_gc_traits! { struct ... }` with
  `GcRootHandle<Types>` for JS references
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

70/70 tests pass on Boa.  1 test is `#[ignore]` on JSC:

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
