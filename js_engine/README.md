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
| Sequence iteration | `process_items` | `ec.property_key_from_index`, `ExecutionContext::get` |

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

### `create_builtin_function` barrier

`JsEngine::create_builtin_function` takes a closure receiving
`&mut dyn ExecutionContext<T>` — architecturally correct for a generic
layer, and the Web IDL bindings infra (`operation.rs`, `attribute.rs`)
already uses it successfully with `Types::object_from_function()` to
convert `T::Function` → `T::JsObject`.  But binding files can't use it
because they receive `&mut dyn ExecutionContext<T>`, not `&mut dyn JsEngine<T>`.
Two options: (a) move `create_builtin_function` to `ExecutionContext<T>`
(spec-justified: its realm parameter defaults to the current Realm Record,
which EC provides via `current_realm()`), or (b) add `fn engine(&self) ->
&dyn JsEngine<T>` to `ExecutionContext<T>`.  Either way, all
`NativeFunction::from_closure` + `FunctionObjectBuilder` + `context_as_ec`
sites get replaced with `create_builtin_function` + Web IDL callback
invocation (<https://webidl.spec.whatwg.org/#invoke-a-callback-function>).
This is Phase C.

## Per-backend details

See module docs for implementation status and quirks:

| Backend | Module | Status |
|---|---|---|
| Boa | `src/boa/mod.rs` | ✅ Full parity — all trait methods implemented, 12 unit tests pass |
| JSC | `src/jsc/mod.rs` | ✅ Full parity — all trait methods implemented, 15 unit tests pass. Complex ops (promises, BigInt, JSON) use `JSEvaluateScript` fallbacks. 1 known crash (`JSObjectSetProperty` on eval-created plain objects). |
| GC | `src/gc.rs` | ✅ Complete — `impl_gc_traits!` macro, `GcRootHandle<T>` with Boa trace impl, `create_root` on EC trait. |

## Migration status

POC is **complete** — 50/50 tests pass on Boa.  The test file
(`content/src/generic_js_test.rs`) proves every content pattern can be
expressed through the generic API with zero structural `#[cfg]`.

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

### Remaining phases

| # | Phase | Effort | Status |
|---|---|---|---|
| **A. GC derive conversion** | Replace Boa derives with `impl_gc_traits!` on 34 types | Small | ✅ DONE |
| **B. Binding body conversion** | Replace ~197 `ec_to_ctx` across binding files with `ec.with_object_any()` + `ec.to_rust_string()` patterns | Medium | 🔶 ~75% done. document.rs fully generic (18→0). 49 ec_to_ctx eliminated this session. ~115 remain — most blocked on missing trait methods (ObjectInitializer) or deeper refactors (DOMException helpers, NativeFunction). |
| **C. NativeFunction bridging** | Add a `native_fn_wrapper` helper, replace ~200 scattered `context_as_ec` casts | Medium | Not started |
| **D. Remove remaining adapters** | Two `ContextEventDispatchHost` adapters in `writablestreamdefaultcontroller.rs` and `event_target.rs` | Small | Not started |
| **E. Conditional Types alias** | Switch `Types` between `BoaTypes`/`JscTypes` via `#[cfg]` | Large | Blocked on B, C, D |
| **F. Generic EnvironmentSettingsObject** | Make EDS own `dyn ExecutionContext<T>` | Medium | Blocked on E |
| **G. Delete ec_to_ctx bridge functions** | Delete `ec_to_ctx`, `context_as_ec`, `context_as_ec_ref`, `context_as_engine` | Small | Blocked on F |
| **H. JSC content tests** | Enable 5 `#[ignore]` tests | Medium | Blocked on E |

### Dependency order

```
A (GC derives) ─┐ ✅
B (binding bodies) ─┤ 🔶 ~70%
C (NativeFunction) ─┤ ⬜
D (remaining adapters) ─┤ ⬜
                        ├──► E (conditional Types) ──► F (generic EDS) ──► G (delete ec_to_ctx)
                                                            │
                                                            └──► H (JSC tests)
```

Phases A–D are independent and can be done in any order.
Phase E requires A–D complete (no Boa-specific code outside `#[cfg(feature = "boa")]` gates).
Phases F and H depend on E.

### Next session plan

**⚠️ Before converting any more binding files: add missing trait methods and
validate them in `generic_js_test.rs` first.**  The test file must cover every
generic pattern before it can be applied to production code.  Several
remaining `ec_to_ctx` calls are blocked on Boa APIs that have no generic
equivalent yet — not on mechanical conversion work.

| Step | What | Details |
|---|---|---|
| **B1. Add `new_syntax_error` to trait** | `ExecutionContext::new_syntax_error(&str) -> JsValue` | Same pattern as `new_type_error` / `new_range_error`. Unblocks ~9 ec_to_ctx in `document.rs` + `element.rs`. Validate in test file, implement for Boa + JSC. | ✅ DONE — 53/53 pass, document.rs now 0 ec_to_ctx |
| **B2. Refactor DOMException helpers** | `dom_exception_error`, `map_location_value`, `map_location_result` in `location.rs` take `&mut Context` but `create_interface_instance` already takes `ec`. Refactor them to take `ec` directly, add `entry_settings_object_ec` wrapper + `try_with_location_ref` helper. | Unblocks all 22 ec_to_ctx in `location.rs`. Pattern already validated (create_interface_instance + value_from_object). |
| **B3. Convert remaining binding files** | After B1+B2 unblock SyntaxError + DOMException: convert the remaining simple getter/setter functions in `document.rs`, `element.rs`, `location.rs`, `node.rs` | Mechanical: `ec.to_rust_string()`, `try_with_*` helpers, `_ec` wrappers. All patterns validated. |
| **C. create_builtin_function access from EC** | Binding functions receive `&mut dyn ExecutionContext<T>` but `create_builtin_function` lives on `JsEngine<T>` (the factory trait). To eliminate `NativeFunction::from_closure` / `FunctionObjectBuilder` (last Boa-specific API in binding files), we need one of: (a) move `create_builtin_function` to `ExecutionContext<T>` (spec-justified: realm defaults to current Realm Record, which EC provides), or (b) add `fn engine(&self) -> &dyn JsEngine<T>` to `ExecutionContext<T>`. Then replace all `NativeFunction::from_closure` + `FunctionObjectBuilder` + `context_as_ec` with `ec.create_builtin_function(...)` + `Types::object_from_function(...)`, following the Web IDL callback invocation algorithm (<https://webidl.spec.whatwg.org/#invoke-a-callback-function>) for call-time dispatch (already uses `EcmascriptHost::call`). Validate in test file. | Unblocks `strategy.rs`, `element.rs` class_list, `abort_signal.rs`, `hyperlink_element_utils.rs` link_property, event handler setters (~36 call sites across 8 files). |
| **D. Remove adapters** | Two `ContextEventDispatchHost` instances | Unblocks `event_target.rs` (3 ec_to_ctx) |
| **E. Conditional Types** | `#[cfg]` gate all Boa imports | Large mechanical change; blocked on B, C, D |

### Current state

**Phase A — DONE.** 34 platform types converted to `impl_gc_traits!`.

**Phase B — ~70% done.** 49 ec_to_ctx eliminated across 5 files this session.
Shared infrastructure ready:
- `downcast.rs`: generic `try_with_event_target_mut/ref`,
  `try_with_abort_signal_mut/ref`, `try_with_event_mut` — all using
  `ec.with_object_any()` / `ec.with_object_any_mut()`
- `platform_objects.rs`: `_ec` wrappers for `document_object`,
  `resolve_element_object`, `object_for_existing_node`,
  `invalidate_cached_node_ids`, `take_animation_frame_callbacks`,
  `resolve_or_create_text_node_object`
- `hyperlink_element_utils.rs`: `document_creation_url_ec` wrapper +
  `try_with_hyperlink_element_utils_ref` helper
- `generic_js_test.rs`: 53/53 pass

**Remaining ec_to_ctx categorized by blocker:**

| Blocker | Files | Count | Fix |
|---|---|---|---|
| No `new_syntax_error` on trait | ~~document.rs~~, element.rs | ~1 | ✅ Added to trait, validated, document.rs fully converted |
| `dom_exception_error` takes `&mut Context` | location.rs | 22 | Refactor helpers to take `ec` |
| `ObjectInitializer` no generic eq | element.rs (class_list, getBoundingClientRect) | 8 | `create_plain_object` + `object_set_property` on trait — can build property-by-property |
| `FunctionObjectBuilder` / `NativeFunction` | strategy.rs, abort_signal.rs, link_property | ~9 | Phase C |
| `ContextEventDispatchHost` | event_target.rs | 3 | Phase D |
| `with_*_mut` borrow limitation | html_media_element.rs, html_iframe_element.rs | 4 | Phase F |
| `JsArray::from_iter` | node.rs (get_child_nodes) | 1 | `create_empty_array` + `array_push` on trait — mechanical |
| `appendable_node` takes `&JsValue` | node.rs | 3 | Check if refactorable to not need ctx |
| Structured clone, timers, etc. | window.rs | 11 | Mixed deep blockers |
| Streams domain calls | readablestream.rs, writablestream.rs, transformstream.rs | ~58 | Separate session |
| Misc (abort_controller, wasm, etc.) | various | 5 | Mixed |

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
| Platform object creation | `create_test_widget` | `create_interface_instance` (already uses `create_object_with_any`) |
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

**`with_object_any_mut` borrow-limitation:**
`with_object_any_mut` returns a reference whose (unsafe-extended)
lifetime borrows from `ec`, not from the JS object.  This means you
cannot call an `ec` method while the returned reference is alive.
When a domain method itself takes `&mut dyn ExecutionContext` (e.g.
`play()`, `pause()`, `set_src()`), use the old
`JsObject::downcast_mut::<T>()` + `ec_to_ctx` pattern instead:
`JsObject::downcast_mut` borrows from the object, leaving `ec` free.

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
- **Platform object creation**: `ec.create_object_with_any(prototype,
  Box::new(data))`
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

53/53 tests pass on Boa.  5 tests are `#[ignore]` on JSC due to known
backend gaps (`get_iterator`, `create_builtin_function`, `SharedArrayBuffer`).

## Working during migration

**End-of-task override:**  While working on Phase B–G migration (binding
bodies, NativeFunction bridging, adapter removal, conditional Types), the
standard end-of-task verification steps (WPT, navigation verification,
clippy, fmt) are **skipped**.  Only `cargo check -p content` is required
to validate each change.  Full verification resumes after Phase G is
complete.
