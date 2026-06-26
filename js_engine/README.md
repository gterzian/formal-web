# `js_engine` crate

Implements the ECMA-262 abstract operation trait (`JsEngine<T>`) that bridges
between ECMAScript engines (Boa, JSC) and formal-web's HTML/DOM/WebIDL layers.

<https://tc39.es/ecma262/>

## Philosophy

`js_engine` has exactly two categories of abstractions:

### Standard: ECMA-262 operations

Web standards (HTML, DOM, Streams, Web IDL) define their behavior in terms of
ECMA-262 abstract operations: `Call`, `Get`, `ToNumber`, `NewPromiseCapability`,
`PerformPromiseThen`, `CreateRealm`, etc.  The `JsEngine<T>` and
`EcmascriptHost<T>` traits expose these same operations behind a generic
interface.  Every integration point between Web standards and JavaScript is
specified in the Web standards themselves:

| Web Standard | Spec operation | Trait method |
|---|---|---|
| Web IDL "react to a promise" | `PerformPromiseThen` | `JsEngine::perform_promise_then` |
| Web IDL "a new promise" | `NewPromiseCapability` | `JsEngine::new_promise_capability` |
| HTML "create a new realm" | `InitializeHostDefinedRealm` | `JsEngine::create_realm` |
| HTML "host promise rejection tracker" | `HostPromiseRejectionTracker` | `HostHooks::promise_rejection_tracker` |
| Web IDL "invoke a callback function" | `Call` | `EcmascriptHost::call` |
| Streams "size algorithm" | `Call` | `EcmascriptHost::call` |
| DOM event dispatch | `Get`, `Call` | `EcmascriptHost::get`, `.call` |

No new abstractions.  No wrappers around wrappers.  The code follows the same
structure the Web standards already define.

### Engine-specific: GC and lifecycle

The one thing **not** specified in any Web standard is GC integration.  Each JS
engine provides its own internal GC API (tracing GC in Boa,
`JSValueProtect`/`JSValueUnprotect` in JSC).  The `gc.rs` module abstracts over
those differences — it is deliberately the one "weird" part of the crate.


## Features

| Feature | Engine | Default | Link |
|---|---|---|---|
| `boa` | Boa (git dep) | **default** | Links boa_engine; WebAssembly via wasmtime in `content/src/wasm/` |
| `jsc` | JavaScriptCore (macOS) | opt-in | Links system framework; WebAssembly built-in (no wasmtime) |

Features are mutually exclusive — only one engine can be active at a time.

### Backend selection

```bash
# Boa (default)
cargo check -p js_engine

# JSC
cargo check -p js_engine --no-default-features --features jsc
```

### WebAssembly

- **Boa backend**: WebAssembly is provided externally via `wasmtime` in
  `content/src/wasm/`.  Boa delegates to wasmtime for compilation and
  instantiation; the content crate manages the background worker.
- **JSC backend**: WebAssembly is built into JavaScriptCore.  The
  `content/src/wasm/` module is not used.  Wasm compile/instantiate
  go through `JSEvaluateScript` or JSC's internal Wasm API.

## Layout

```
src/
  lib.rs        Crate root — module declarations, feature-gated re-exports
  types.rs      JsTypes + JsTypesWithRealm
  engine.rs     JsEngine<T>, Completion, EcmascriptHost<T>, HostHooks<T>
  enums.rs      Numeric, PreferredType, IntegrityLevel, IteratorKind, etc.
  records.rs    IteratorRecord, PromiseCapability, PropertyDescriptor, etc.
  gc.rs         Trace, Finalize, JsTypesGcExt, JsEngineGcExt, GcRootHandle
  boa/          feature = "boa"
    mod.rs      module root
    types.rs    BoaTypes (JsTypes + JsTypesWithRealm marker)
    engine.rs   BoaEngine (JsEngine<BoaTypes> + EcmascriptHost<BoaTypes> impl)
  jsc/          feature = "jsc"
    mod.rs      module root
    sys.rs      raw `extern "C"` FFI bindings to JavaScriptCore (unsafe-only)
    types.rs    safe JSC wrapper types (JscContext, JscValue, JscObject, etc.)
    engine.rs   JscTypes + JscEngine (JsEngine<JscTypes> + EcmascriptHost<JscTypes> impl)
```

### Safety boundary

The JSC backend is split into two modules with a strict safety boundary:

| Module | Contents | Safety |
|---|---|---|
| `jsc_sys.rs` | Raw `extern "C"` function declarations, `#[repr(C)]` types | **Unsafe only** — every function is `unsafe` |
| `jsc/` | Safe wrapper types (RAII, borrow-checked) | **Safe** — encapsulates `unsafe` calls to `jsc_sys` |

No `unsafe` code in `jsc/`. All raw pointer manipulation goes through `jsc_sys`
functions.  Safe wrappers own or borrow pointers via RAII and uphold invariants
(context validity, GC rooting, string lifetime rules).

## Architecture

### Relationship between layers

- **HTML** calls ECMA-262 abstract operations → `JsEngine<T>` trait methods.
- **Web IDL** calls ECMA-262 abstract operations → same trait, same methods.
- **Domain types** (`Window`, `Document`, `Event`) need engine-specific GC heap
  access (`downcast_ref`, `create_interface_instance`) — these stay behind the
  concrete engine type and are not abstracted through `JsEngine`.

### What gets abstracted, what doesn't

| Category | Abstracted via `JsEngine<T>`? | Example |
|---|---|---|
| ECMA-262 abstract operations | **Yes** (§7.1, §7.2, §7.3, §9.3, §16, §25, §27) | `Call`, `Get`, `ToNumber`, `CreateRealm` |
| Web IDL callback operations | **Yes** — via `EcmascriptHost<T>` | `call_user_objects_operation`, `invoke_callback_function` |
| GC heap traversal (`downcast_ref`, `with_global_scope`) | **No** — engine-specific | `window.downcast_ref::<Window>()` |
| JS native function registration (`NativeFunction`) | **No** — Boa-specific API shape | `NativeFunction::from_fn_ptr(...)` |
| Platform object construction | **No** — uses Boa `ObjectInitializer` | `create_interface_instance::<Event>(...)` |
| Proxy creation | **No** — Boa's proxy builder not publicly creatable | `Object::from_proto_and_proxy_handler(...)` |

### Three-layer architecture

| Layer | Location | What it contains |
|---|---|---|
| **Domain** | `content/src/<domain>/` | Rust struct + spec-algorithm methods |
| **Web IDL bindings infra** | `content/src/webidl/bindings/` | Generic traits, NOT domain-specific |
| **JS bindings glue** | `content/src/js/bindings/<domain>/` | Thin arg-extraction + delegation |

## Spec Documentation Convention

Every method on `JsEngine<T>` has **only** the ECMA-262 spec anchor URL in its
doc comment.  Zero prose.  Examples:

- `/// <https://tc39.es/ecma262/#sec-toboolean>`
- `/// <https://tc39.es/ecma262/#sec-tonumber>`
- `/// <https://tc39.es/ecma262/#sec-get-o-p>`

The spec IS the documentation.  The trait method name mirrors the spec operation
name exactly.  If the name is not enough context, the algorithm lives in the spec.

Infrastructure traits (`Trace`, `Finalize`, `JsTypesGcExt`, etc.) carry no spec
links — they are crate-internal abstractions, not spec-defined operations.

## Design

### `JsTypes`

ECMA-262 §6 defines language types (Undefined, Null, Boolean, String, Symbol,
Number, BigInt, Object) and object subtypes by internal slot profile
(`[[ArrayBufferData]]`, `[[PromiseState]]`, etc.).  Each distinct slot profile
becomes an associated type.

```rust
pub trait JsTypes: Sized + 'static {
    type JsValue: Clone;
    type JsObject: Clone;
    type JsString: Clone + Eq + Hash;
    type ArrayBuffer: Clone;
    type Promise: Clone;
    // ... 15 more subtypes
    type PropertyKey: Clone;

    // Infallible upcasts (owned conversions via From impls)
    fn object_from_array_buffer(ab: Self::ArrayBuffer) -> Self::JsObject;
    fn value_from_object(o: Self::JsObject) -> Self::JsValue;
    // ... more upcasts

    // Fallible downcasts
    fn value_as_object(v: &Self::JsValue) -> Option<Self::JsObject>;
    fn object_as_array_buffer(o: &Self::JsObject) -> Option<Self::ArrayBuffer>;
    // ... more downcasts
}
```

### `JsEngine<T>`

Everything ECMA-262 specifies that is callable from outside the engine:
abstract operations, realm creation, script execution, the job queue.
Nothing from HTML §8.1, nothing from Web IDL §3.

| Chapter | Section | Methods |
|---|---|---|
| §7.1 | Type Conversion | `to_primitive`, `to_boolean`, `to_number`, `to_numeric`, `to_int*`, `to_uint*`, `to_js_string`, `to_object`, `to_property_key`, `to_length`, `to_index`, etc. |
| §7.2 | Testing & Comparison | `require_object_coercible`, `is_array`, `is_callable`, `is_constructor`, `is_extensible`, `is_integral_number`, `is_property_key`, `same_value`, `same_value_zero`, `is_loosely_equal`, `is_strictly_equal` |
| §7.3 | Object Operations | `get`, `get_v`, `set`, `create_data_property`, `define_property_or_throw`, `delete_property_or_throw`, `get_method`, `has_property`, `has_own_property`, `call`, `construct`, `set_integrity_level`, `test_integrity_level`, `species_constructor`, `get_iterator`, `iterator_step_value`, `iterator_close`, `async_iterator_close` |
| §9.3 | Realm | `create_realm`, `set_realm_global_object`, `set_default_global_bindings`, `current_realm`, `realm_intrinsics` |
| §9.6 | Jobs | `enqueue_job`, `run_jobs` |
| §16 | Script/Module | `evaluate_script`, `evaluate_module` |
| §25.1 | ArrayBuffer | `allocate_array_buffer`, `is_detached_buffer`, `detach_array_buffer`, `clone_array_buffer`, `is_fixed_length_array_buffer`, `get_value_from_buffer`, `set_value_in_buffer` |
| §25.2 | SharedArrayBuffer | `allocate_shared_array_buffer` |
| §27.2 | Promise | `promise_resolve`, `new_promise_capability`, `perform_promise_then` |
| §27.5 | Generator | `generator_start` |

Value construction (`value_from_string`, `value_from_bool`, `value_from_number`,
`value_undefined`, `value_null`) lives on `JsEngine<T>` (not `JsTypes`) because
JSC's C API requires a `JSContextRef` for value creation.

### `Completion<T, Ty>`

```rust
pub type Completion<T, Ty> = Result<T, <Ty as JsTypes>::JsValue>;
```

Isomorphic to the spec's Completion Record (§6.2.4):
- `Ok(v)` → normal completion `~v~`.
- `Err(e)` → throw completion `*e*`.
- Rust `?` → spec `?` (ReturnIfAbrupt).
- `!`-guaranteed operations return `T` directly.

### `EcmascriptHost<T>`

<https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
<https://webidl.spec.whatwg.org/#invoke-a-callback-function>

Thin trait wrapping the ECMA-262 operations that Web IDL callback algorithms
need: `Get`, `IsCallable`, `Call`, microtask checkpoint, and exception
reporting.  `BoaEngine` implements `EcmascriptHost<BoaTypes>` directly.

Where code paths only have `&mut Context` (the inner Boa handle), minimal local
adapters are defined inline within the calling function — no separate module
or public type.  These will be eliminated when the call chain threads `Engine`.  See the
in-function `struct CtxHost` in `strategy.rs` and `readablestreamsupport.rs` for
the pattern, which mirrors `ContextEventDispatchHost` in `event_target.rs`.

### `HostHooks<T>`

<https://html.spec.whatwg.org/#javascript-specification-host-hooks>

Constructor-time configuration for HTML-specific engine hooks:
`HostEnsureCanCompileStrings`, `HostPromiseRejectionTracker`,
`HostEnqueuePromiseJob`, `HostLoadImportedModule`.

### `PropertyDescriptor<T>` is concrete (NOT an associated type)

```rust
pub struct PropertyDescriptor<T: JsTypes> {
    pub value: Option<T::JsValue>,
    pub writable: Option<bool>,
    pub get: Option<T::Function>,
    pub set: Option<T::Function>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}
```

The spec's Property Descriptor is a plain record type with no engine-specific
representation.  Keeping it concrete avoids per-engine conversion.

## Implementation Status

### Boa backend (`boa/`)

**BoaTypes** — Complete. All associated types mapped, all upcast/downcast
methods implemented.

**BoaEngine** — Implements `JsEngine<BoaTypes>`. All ECMA-262 abstract
operations listed above are implemented except:
- `evaluate_module` — module loader setup not wired (`todo!()`)
- `generator_start` — VM internal (`todo!()`)
- `enqueue_job` — no-op (Boa job trait not wired)

`EcmascriptHost<BoaTypes>` is also implemented on `BoaEngine` directly.

**Known Boa-specific quirks:**
1. `JsObject<T>` vs `JsObject` — Built-in types use typed wrappers. Upcast/downcast
   require owned conversions. Zero-cost reference casts are not possible.
2. `downcast_ref` returns `GcRef<T>` — Not `&T`. Must clone immediately.
3. `into_opaque` returns `Result<JsValue, JsError>` — Two-level unwrap.
   Helper `into_completion(result, ctx)` wraps this.
4. `JsNativeError::into_opaque` returns `JsObject` — Additional `JsValue::from()`.
5. `to_primitive` — Takes `PreferredType` (not `Option`). Uses `Default` variant.
6. Proxy builder is not public — `content/src/html/windowproxy.rs` uses
   `Object::from_proto_and_proxy_handler` directly.

### JSC backend (`jsc/`)

**FFI Coverage:** 34 `extern "C"` function declarations in `jsc_sys.rs`.

| Group | Count | Key functions |
|---|---|---|
| Context | 4 | `JSGlobalContextCreate/Release`, `JSContextGetGlobalObject` |
| Values | 18 | `JSValueGetType`, `JSValueTo/Is*`, `JSValueMake*` |
| Objects | 10 | `JSObjectIsFunction/Constructor`, `JSObjectCallAsFunction/Constructor`, `JSObjectCopy/Set/Has/DeleteProperty` |
| Strings | 7 | `JSStringCreate/GetLength/GetUTF8CString/Retain/Release/IsEqual` |
| Evaluation | 1 | `JSEvaluateScript` |
| Typed arrays | 1 | `JSObjectMakeArrayBufferWithBytesNoCopy` |

**Working:** §7.1 type conversions (partial), §7.2 testing/comparison (partial),
§7.3 object operations (most), §7.4 iterator operations, §16 script evaluation.

**`todo!()` items:** `to_primitive`, `to_bigint`, `string_to_bigint`, `is_array`,
`set_realm_global_object`, `realm_intrinsics`, `evaluate_module`,
`promise_resolve`, `new_promise_capability`, `perform_promise_then`,
`generator_start`, `enqueue_job`, `run_jobs`.

## GC & Lifecycle Module (`gc.rs`)

| Type | Role |
|---|---|
| `Trace` | Marker trait: declares GC-reachable fields |
| `Finalize` | Lifecycle hook when GC reclaims backing memory |
| `JsTypesGcExt` | Extends `JsTypes` with cycle-safe `Reflector` |
| `JsEngineGcExt` | Extends `JsEngine` with `create_root` |
| `GcRootHandle` | RAII guard for rooting a JS value |

Each backend provides its own concrete implementations inside `#[cfg]`-gated
sub-modules of `gc.rs`.

## What NOT To Do

### ❌ New wrapper types or adapters

The generic `JsEngine<T>` and `EcmascriptHost<T>` traits expose every
ECMA-262 operation that Web standards need.  Do not create new wrapper types,
adapter structs, or intermediate abstractions.  If a function needs
`EcmascriptHost`, pass `&mut Engine` directly — `BoaEngine` implements the trait.

If you must bridge from a `&mut Context` (the Boa inner handle), define a
minimal local struct inline within the function that needs it — the same pattern
as the pre-existing `ContextEventDispatchHost` in `event_target.rs`.  Do not put
it in a separate file, module, or public re-export.  These are migration
artifacts and will be eliminated when the call chain threads `Engine`.

### ❌ New Boa-specific wrapper types as workarounds

When hitting a hard-to-abstract feature (NativeFunction, proxy creation), do not
create new Boa-specific wrappers.  Wrap the operation as a thin generic method
on `JsEngine<T>` with a Boa implementation and `todo!()` for JSC.

### ❌ `use js_engine::BoaEngine` in domain or binding code

The concrete engine type is accessed through a content-local alias
(`content/src/js/mod.rs` → `Engine`).  Domain code works through
`JsEngine<BoaTypes>` trait methods.  The alias is the only place BoaEngine
is imported from `js_engine`.

### ❌ Boa-specific type re-exports from `js_engine` crate root

`js_engine` exports only the generic interface (`JsTypes`, `JsEngine`,
`EcmascriptHost`, `Completion`, etc.).  Engine-specific types like `BoaEngine`
and `BoaTypes` live in `js_engine::boa::` and are NOT re-exported from the
crate root.  This enforces the abstraction boundary at the import level.

## Open Problems

### P1: `value_from_*` on JSC needs a context

Already moved to `JsEngine` as instance methods. JSC just needs each method
to plumb through the context reference.

### P2: No `JsEngineErased` type

`enqueue_job` takes `Box<dyn FnOnce() + Send>` — the closure can't access the
engine.  Promise reactions need to call engine operations when the job runs.

**Fix**: Create object-safe `JsEngineErased`, change `enqueue_job` to
`Box<dyn FnOnce(&mut dyn JsEngineErased) + Send>`.

### P3: `NativeFunction` barrier

Boa's `NativeFunction` callback type has signature `fn(&JsValue, &[JsValue], &mut Context)`
— fixed by Boa's FFI.  Callbacks receive `&mut Context` directly and cannot pass
`&mut BoaEngine` (or `&mut impl JsEngine<T>`) to domain code.

**Approach:** Wrap through a thin shim that stores a `*mut BoaEngine` in a
thread-local, retrieves it inside the `NativeFunction` callback, and calls
through `JsEngine<BoaTypes>`.  The shim lives in `js_engine/src/boa/engine.rs`.

For JSC, the callback API already provides a `*mut c_void` user data pointer —
no workaround needed.

### P4: `set_host_hooks` integration with Boa's ContextBuilder

Boa host hooks are set during `ContextBuilder::host_hooks()`, not at runtime.
`set_host_hooks` trait method is currently a no-op.

### P5: `realm_intrinsics` not available in JSC

No C API to extract constructors.  Workaround: look up by name on the global
object.

### P6: JSC GC safety

`JscValue`, `JscObject` hold raw `*mut` pointers.  JSC's GC may collect
unrooted values.  `GcRootHandle` + `JsEngineGcExt::create_root` provides the
RAII guard pattern.  Domain code must root any JS value that crosses an async
boundary.

### P7: `Callback` generic over `JsTypes`

The `Callback` struct in `content/src/webidl/callback.rs` is Boa-concrete
(uses `JsObject` directly, derives `boa_gc::Trace`/`Finalize`).  Making it
generic over `JsTypes` would require abstracting GC trait derives.

## Migration Plan: content/ → generic `JsEngine<T>`

The existing `content/` codebase already uses a clean layering:
- **Web IDL callback operations** abstracted behind `EcmascriptHost<T>`
- **Domain algorithms** written against `Context` for ECMA-262 ops
- **JS bindings** use `NativeFunction` for callback registration

The migration replaces Boa-specific types with generic `JsEngine<T>` equivalents,
layer by layer.  No new Boa-specific wrapper types are introduced.

| Phase | What | Status |
|---|---|---|
| **1. Foundation** | Add `js_engine` dep; create content-local alias in `content/src/js/mod.rs` | ✅ Done |
| **2. ESO storage** | `EnvironmentSettingsObject` stores `Engine`; ECMA-262 ops route through `JsEngine<BoaTypes>` | ✅ Done |
| **3. WebIDL infra** | `EcmascriptHost` made generic (✅), `Callback` still Boa-concrete | ⏳ Partial |
| **4. Domain layer** | Replace `context()` calls for ECMA-262 ops with `JsEngine<T>` trait calls | ❌ |
| **5. JS bindings** | Shim `NativeFunction` → `JsEngine<BoaTypes>` (thread-local or stored pointer) | ❌ |
| **6. Full generics** | Parameterize `content/` over `T: JsTypes` (when JSC backend is ready) | ❌ |

### What's left for Phase 3

- Make `Callback` generic over `JsTypes` (blocked by `#[derive(Trace, Finalize)]`)
- Eliminate duplicate `ContextEventDispatchHost` impls in `event_target.rs`,
  `writablestreamdefaultcontroller.rs`, `ui_event_dispatch.rs` — they all
  implement the same `EcmascriptHost<BoaTypes>` over `&mut Context`
