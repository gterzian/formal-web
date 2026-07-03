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
3. **Migrate the original function in place.** No `_ec` suffix in final code.
   During migration, if a non-EC bridge must be kept for unconverted callers,
   the migrated function takes a `_ec` suffix. When the bridge is removed,
   drop the `_ec` suffix. See the `_ec` convention section below.
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
- Creating `xxx_ec()` wrappers for new code (functions that start with generic API)
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
| Domain | <https://streams.spec.whatwg.org/#readable-stream-cancel> | `content/src/streams/readablestream.rs` → `readable_stream_cancel()` |
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


`BoaContext` (was `BoaEngine`) wraps `boa_engine::Context` and implements
`ExecutionContext<BoaTypes>`.  The `JsEngine<BoaTypes>` impl on the same
struct is a convenience — in a clean split the factory would be a standalone
global.  For now they co-reside because Boa's `Context` serves both roles.

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

### Not yet abstracted (known blockers)

| Operation | Blocked on |
|---|---|
| `Context::eval` (script evaluation) | `evaluate_script` on `JsEngine<T>` exists; callers haven't migrated |
| `ObjectInitializer`, `register_global_property` | Boa object-model construction; needs centralized `build_context` path |
| Structured clone | Boa-internal APIs (realm access, data clone) |

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

POC is **complete** — 81/81 tests pass on Boa in `content/src/generic_js_test.rs`
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

### Test-file-first discipline

**Never add a new generic pattern directly to production code.**
Every new generic interface, downcast helper, host-data abstraction,
or subsystem entry-point signature must first be validated in
`content/src/generic_js_test.rs` with compilation and passing unit tests
on **both backends** (Boa and JSC) before it can be applied to any
real production file.

### Current state (updated 2026-07-04)

**Phases A–D, S1–S10, T1–T2, W1–W2, G1–G3, C2–C3, B1, R1, R2, S, P complete.**
All binding files at 0 ec_to_ctx. All 34 struct/enum definitions use `#[gc_struct]`.
All domain field types use `GcCell<T>`. Generic POC: 81/81 tests pass on Boa.
Phase E (compile-time Types/Engine aliases) is landed — `#[cfg(feature = "jsc")]`
selects between BoaTypes and JscTypes.

**`_ec` suffix count: ZERO — all `_ec` bridge functions eliminated.**

**`completion_to_js_result` bridges: ELIMINATED.** The function definition and
all 15 call sites have been removed. The `AsyncValueIterable` trait is now
generic (takes `&mut dyn ExecutionContext<Types>` instead of `&mut Context`),
enabling `readablestreamasynciterator.rs` to implement it with zero bridges.
`main.rs` wasm promise resolution also uses `Completion` directly.

**`RealmIntrinsics` extended** with `async_iterator_prototype` field
(`T::JsObject`) for the generic async iterable infrastructure.

**Remaining `context_as_ec` calls outside `js_engine/src/` (6 total):**
- `js/bindings/streams/readablestream.rs` (2) — NativeFunction binding-function adapters
- `wasm/namespace.rs` (2) — gated behind `#[cfg(feature = "boa")]`
- `html/safe_passing_of_structured_data.rs` (1) — `data_clone_error` helper
- `js/bindings/wasm/mod.rs` (1) — NativeFunction adapter

**Eliminated in this session:** `webidl/async_iterable.rs` (10) — fully converted to
`&mut dyn ExecutionContext<Types>` with zero `boa_engine::*` imports.
Replaced `NativeFunction`, `FunctionObjectBuilder`, `ObjectInitializer`,
`JsPromise::then`, `JsObject::downcast_ref`, and `create_iter_result_object`
with generic EC trait methods (`create_builtin_function_from_behaviour`,
`create_plain_object` + `object_set_property`, `perform_promise_then`,
`with_object_any`, `create_object_with_any`). All 10 `context_as_ec`
bridge calls eliminated.

**Eliminated previously:** `main.rs` (10), `environment_settings_object.rs` (1),
`host_hooks.rs` (3), `registry.rs` (2), `document.rs` (1) — 17 total.

### Next session: recommended order

1. **Eliminate remaining `context_as_ec` bridges** — 6 calls remaining:
   - `html/safe_passing_of_structured_data.rs` (1) — convert `data_clone_error` to EC
   - `wasm/namespace.rs` (2) — convert function signatures from Context to EC
   - `js/bindings/wasm/mod.rs` (1) — NativeFunction adapter pattern
   - `js/bindings/streams/readablestream.rs` (2) — NativeFunction adapter pattern

2. **Phase E** — Content crate does not yet compile for JSC.
   Blockers: GC trait bounds (`boa_engine::Trace`/`#[gc_struct]`) and
   `unsafe_ignore_trace` attribute on non-wasm structs. Wasm is gated behind
   `#[cfg(feature = "boa")]`.

## `_ec` suffix convention

The migration is staged: a function that takes `Context` is converted to take
`&mut dyn ExecutionContext<Types>`.  When its callers cannot all be converted in
one pass (because some callers chain through deeper Boa APIs), the old function
is kept as a one-line bridge:

```rust
// OLD — bridge, to be deleted when all callers are converted
pub(crate) fn get_reader(
    &mut self, options: &JsValue, context: &mut Context,
) -> JsResult<JsObject> {
    self.get_reader_ec(options, js_engine::boa::context_as_ec(context))
        .map_err(|e| JsError::from_opaque(e))
}

// NEW — the real implementation, temporary _ec suffix
pub(crate) fn get_reader_ec(
    &mut self, options: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // ... implementation ...
}
```

**Completion: delete the bridge, drop the `_ec` suffix.**  When every caller has
been converted to pass `&mut dyn ExecutionContext<Types>` directly, the bridge
function is deleted and the `_ec`-suffixed function is renamed in place to its
original name.  The same algorithm lives at the same conceptual location — the
`_ec` is not a permanent part of the name.

### Rules

1. **Temporary, not structural.** The `_ec` suffix is a marker that says "this
   function is already migrated but still needs a bridge while unconverted
   callers remain."  It carries zero semantic meaning about the function's role.

2. **One bridge per `_ec` function.** Every `_ec` function MUST have a non-EC
   bridge that delegates through `context_as_ec`.  When that bridge has zero
   callers, delete it and rename the `_ec` function.

3. **Never add `_ec` to new code.** Functions that start with the generic API
   (no Boa-specific heritage) must NEVER get an `_ec` suffix — there is nothing
   to bridge.  They take `&mut dyn ExecutionContext<Types>` directly with no
   suffix.

4. **Never introduce a `_ec` function without a bridge.** Every migration
   creates exactly two functions for a short time: the old bridge and the new
   `_ec` variant.  If there are no remaining non-EC callers, convert the
   function in place — no `_ec` suffix at all.

### End state

**Zero `_ec` function definitions remaining.**
All migration bridges and `_ec` suffixes have been eliminated.
Every function uses its original name with an EC parameter.

### How to remove `_ec` from a function

1. Ensure every caller of the non-EC bridge (`get_reader` in the example above)
   has been converted to call the `_ec` variant directly.
2. Delete the non-EC bridge function.
3. Rename `get_reader_ec` → `get_reader`.
4. Update all call sites to use the new name.
5. `cargo check -p content` passes.

## Working notes

**`builtin_with_captures` / `builtin_callback`:** Use `crate::js::builtin_with_captures(ec, ...)`
for EC-taking closures (zero bridges). The Context-taking `builtin_with_captures_ctx`
bridges through `context_as_engine` — prefer the EC variant.

**Test-file-first:** Validate new generic patterns in `generic_js_test.rs`
on both backends before production code. 81/81 tests pass on Boa.

**Do not introduce new `_ec` functions unnecessarily.** If the function has no
non-EC callers, convert it in place without a suffix.  The `_ec` suffix is only
acceptable when a corresponding non-EC bridge must be kept for unconverted
callers.

## Working during migration

**End-of-task override:** While working on Phase D–E migration, standard
verification steps (WPT, navigation verification, clippy, fmt) are
**skipped**.  Only `cargo check -p content` and step 9 (review session in light of Rule Number One) from top level agents file is required.  Full verification
resumes after Phase E.

**Update this README at end of every task.**  The remaining-phases table,
next-session order, ec_to_ctx counts, and phase status markers must reflect
current state.  This file is the canonical plan — it must never be stale.

**Prune the README.**  After every few sessions, remove or compress outdated
sections (completed phase details, stale examples, duplicated design notes,
dependency-order diagrams).  The README is a living plan, not a log.
