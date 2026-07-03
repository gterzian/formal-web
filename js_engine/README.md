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

### Migration methodology — spec-first, not Boa-first

When converting Boa-specific code to the generic layer, **follow the spec
chain**, not the Boa API shape.

**Core rules:**
1. **Go deep, not broad.** Convert a function's ENTIRE call chain, not file by file.
2. **Zero bridges.** `ec_to_ctx`, `context_as_ec`, `_ec` wrappers, `completion_to_js_result`
   are ALL bridges — never leave them at boundaries. Convert every called function too.
   The only file where `ec_to_ctx` may appear is `js_engine/src/boa/engine.rs`.
3. **Migrate the original function in place.** Do NOT create `foo_ec` wrappers —
   change the real function's signature and fix all callers. No `_ec` suffix in final code.
4. **Read the spec.** Identify every ECMA-262 operation (Call, Get, PerformPromiseThen, etc.)
   and use the corresponding `ExecutionContext<T>` trait method.

**Replacement table (old → new):**

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

**Anti-patterns (do NOT do):**
- Creating `xxx_ec()` wrappers — convert the real function
- Using `completion_to_js_result` or `context_as_ec` at call sites
- Using `JsPromise::then()`/`new_pending()` when EC trait methods exist
- Converting one file while leaving bridges at its edges

### Ownership model

<https://html.spec.whatwg.org/#environment-settings-objects> defines the
**environment settings object**, which owns a **realm execution context**.
Our `EnvironmentSettingsObject` owns a `BoaContext` which implements
`ExecutionContext<T>`.  The migration end state is for the EDS to own
the generic trait type — the boundary is already correct.

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

### Not yet abstracted (known blockers to EDS owning a generic type)

| Operation | Blocked on |
|---|---|
| Proxy creation (`JsProxyBuilder::build(context)`) | `create_proxy` trait method not yet needed by any EC path |
| `Context::eval` (script evaluation) | `evaluate_script` on `JsEngine<T>` exists; callers haven't migrated |
| `with_global_scope(&Context, ...)` | GC heap traversal; `realm_global_object()` partially covers this |
| `ObjectInitializer`, `register_global_property` | Boa object-model construction; needs centralized `build_context` path |
| `queue_internal_stream_microtask` | Deep byte-tee chain uses `PromiseJob::with_realm` |
| Structured clone, async iterable creation | Boa-internal APIs (realm access, data clone) |
| `downcast_ref::<Window>()` on global object | Content-owned accessor needed (wasm, html_media_element) |

None are fundamental — they just aren't done yet.

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

The `generic_js_test.rs` POC (81/81 tests) proves every content pattern works
through the generic API with zero `boa_engine::*` imports.  See the test file
for working examples.  Key trait methods:

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
Types::value_from_object(o), Types::value_as_object(&v)

// Binding function signature (the standard pattern)
fn binding_fn(
    this: &Types::JsValue,
    args: &[Types::JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Types::JsValue, Types>
```

**Note on `process_items` in POC:** Uses array-like length+indexing (`Get`
for `"length"` then `Get` for `0..length`).  This is NOT the Web IDL
`sequence<T>` conversion (which is iterator-based).  If using this pattern
for `sequence<T>`, rewrite on `get_iterator`/`iterator_step_value`.

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
| **Phase S** ✅ 🔶 | `clone_as_uint8_array` converted to pure EC (uses `clone_array_buffer` + `construct` + `uint8_array` intrinsics). Byte tee closures and transform sink algorithms still use Context. |
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
| **B1** | `Behaviour<T>` trait; `create_builtin_function_from_behaviour` on `ExecutionContext<T>` — object-safe EC method for captures; `builtin_with_captures_ec` now zero bridges (no `ec_to_ctx`, no unsafe); 81/81 POC tests pass |
| A-C | GC derive conversion; binding body conversion; `create_builtin_function` on EC |
| **S-promise** | `PromiseState<T>` enum in js_engine; `promise_state()` method on `ExecutionContext<T>` trait; Boa + JSC backend impls. Replaces `JsPromise::from_object(x)?.state()` (Boa-specific) with `ec.promise_state(&obj)?`. |
| **S1a** | PipeToState EC wrappers (18 methods); `pipe_to_on_promise_settled_ec`; `pipe_reaction_fn` + `pipe_reaction_function_ec`; `pipe_read_result_done_ec`; `queue_internal_stream_microtask_ec`; 3 ReadableStreamPipeTo closures converted to EC path |

### Next session: recommended order

### Current state (updated 2026-07-03)

**Phases A–D, S1–S10, T1–T2, W1–W2, G1–G3, C2–C3, B1, R1, R2 complete.**
All binding files at 0 ec_to_ctx. All 34 struct/enum definitions use `#[gc_struct]`.
All domain field types use `GcCell<T>`. Generic POC: 81/81 tests pass on Boa.
Phase E (compile-time Types/Engine aliases) is landed — `#[cfg(feature = "jsc")]`
selects between BoaTypes and JscTypes.

**ec_to_ctx count: ~11** (was ~34 before this session; 5 eliminated)

**Phase S — Byte tee closures fully converted (this session):**
- `readablestreamdefaultcontroller.rs` — all 4 byte tee pull/cancel ec_to_ctx eliminated.
  PullAlgorithm/CancelAlgorithm now call EC-converted functions directly.
- `readablestreamsupport.rs` — `queue_internal_stream_microtask` converted to EC:
  closure type changed from `FnOnce(&mut Context) -> JsResult<()>` to
  `FnOnce(&mut dyn ExecutionContext<T>) -> Completion<(), T>`; uses
  `ec.current_realm()` + `ec.enqueue_job_with_realm()` instead of
  `context.realm()` + `PromiseJob::with_realm`. 1 ec_to_ctx eliminated.
- `readablestream.rs` — 6 functions converted from `&mut Context` to
  `&mut dyn ExecutionContext<T>`: `readable_byte_stream_tee_pull1/pull2/cancel1/cancel2_algorithm`,
  `readable_byte_stream_tee_pull_with_byob/default_reader`.
  6 helpers converted: `byte_tee_enqueue_to_branch`, `byte_tee_ignore_pull_completion`,
  `byte_tee_switch_to_default_reader`, `byte_tee_switch_to_byob_reader`,
  `readable_byte_stream_tee_default_reader_chunk_steps`.
  `NativeFunction::from_copy_closure_with_captures` → `builtin_with_captures` (EC).
  Inner `queue_internal_stream_microtask` closures converted to EC-taking.
  5 ec_to_ctx eliminated across these files.

**Phase P — WindowProxy conversion (this session):**
- Added `create_proxy(target, handler)` to `ExecutionContext<T>` — ProxyCreate
  (§10.5.14). Boa backend uses Proxy constructor via intrinsics. JSC stub.
- Added `get_prototype_of` to `ExecutionContext<T>` — needed by WindowProxy
  traps. Boa: delegates to `JsObject::prototype()`. JSC stub.
- Added `to_property_descriptor` to `ExecutionContext<T>` — reads descriptor
  fields from a descriptor object. Boa: implemented via `EcmascriptHost::get`.
  JSC stub.
- Converted `content/src/html/windowproxy.rs` — all 10 trap functions changed
  from `NativeFunctionPointer` (Boa fn pointer taking `&mut Context`) to
  EC-compatible signatures. Follows the same recipe as Web IDL observable
  arrays (<https://webidl.spec.whatwg.org/#js-observable-arrays>):
  `OrdinaryObjectCreate(null)` → CreateBuiltinFunction for each trap →
  CreateDataPropertyOrThrow on handler → ProxyCreate(target, handler).
  Uses `ec.create_plain_object(None)` + `ec.create_builtin_function()` for
  each trap in a loop + `ec.set()` → `ec.create_proxy()`.
  1 ec_to_ctx eliminated.

**Remaining ~10 ec_to_ctx by file:**
- `wasm/namespace.rs`: 6 — gated behind `boa` feature, lowest priority.
- `html/html_media_element.rs`: 1 — `with_global_scope` (GC heap traversal)
- `html/safe_passing_of_structured_data.rs`: 1 — structured clone (Boa-internal)
- `webidl/async_iterable.rs`: 1 — async iterator creation (`ObjectInitializer`)
- `js/mod.rs`: 1 — `js_result_to_completion_ec` bridge helper

### Next session: recommended order

1. **Phase P — Remaining singleton ec_to_ctx** — `html/html_media_element.rs`,
   `webidl/async_iterable.rs`, `html/safe_passing_of_structured_data.rs`,
   `js/mod.rs` (4 total).

2. **Phase E validation** — `cargo check -p content --no-default-features --features jsc`.
   Note: `content/src/wasm/` is not yet gated behind `boa` feature. To make Phase E
   clean, gate `pub mod wasm` in `main.rs` behind `#[cfg(feature = "boa")]` and
   conditionally compile the `wasmtime` dep. The wasm module can be left as a
   Boa-only feature since JSC handles WebAssembly internally.

### Working notes

**`builtin_with_captures` / `builtin_callback`:** Use `crate::js::builtin_with_captures(ec, ...)`
for EC-taking closures (zero bridges). The Context-taking `builtin_with_captures_ctx`
bridges through `context_as_engine` — prefer the EC variant.

**Test-file-first:** Validate new generic patterns in `generic_js_test.rs`
on both backends before production code. 81/81 tests pass on Boa.

**`Behaviour<T>` trait design note:** `dyn Behaviour<BoaTypes>` uses no-op
Trace/Finalize — captures are GC-managed objects rooted by their parent.

**Migrate the original function, not a copy.** When converting, change the real
function's signature in place and fix all callers. No `_ec` suffix in final code.

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
