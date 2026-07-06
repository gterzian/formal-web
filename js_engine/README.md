# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Migration status — builtin function creation unified

All builtin function creation now goes through a single method on
`ExecutionContext<T>`:

- **`create_builtin_function(behaviour, length, name, is_constructor)`** —
  the one canonical method.
- **`create_builtin_fn(behaviour, length, name)`** — convenience default
  method delegating with `is_constructor: false`.

Removed: `Behaviour` trait, `create_builtin_function_from_behaviour`,
`create_constructor`, and the unsafe `NativeFunction::from_closure` path.
The Boa backend now uses only `NativeFunction::from_copy_closure_with_captures`.

## Problems found

### 1. 🟡 Direct `JsObject::downcast_ref<T>()` broken for wrapped platform objects

`create_interface_instance` stores data as `NativeDataWrapper(TraceableBox(T))`
inside the JsObject.  Boa's native `downcast_ref::<T>()` can't see through
the wrapper.  Must use `ec.with_object_any(&obj).and_then(|d| d.downcast_ref::<T>())`.

**Fixed in this session:** `content/src/dom/ui_event_dispatch.rs` —
`has_activation_behavior`, `run_activation_behavior`, `apply_to_event_state`.

**Unfixed:** `content/src/html/location.rs:565` (`self.window.downcast_ref::<Window>()`)
requires threading `ec` through all Location methods.

### 2. 🔴 Attribute accessor descriptors not registering on prototypes

`define_regular_attributes` builds accessor descriptors via
`create_builtin_fn` + `define_property_or_throw`, but the properties
never appear on the prototype.  Operations (methods) register fine via
value descriptors.  Suspected in the `PropertyDescriptor<BoaTypes>` →
Boa native descriptor conversion for `get`+`set`-only descriptors.

### 3. ✅ `create_builtin_function` produces constructable functions (verified)

`create_builtin_function(behaviour, length, name, true)` correctly creates
constructable functions on the Boa backend.  All 86 unit tests pass,
including `register_interface_spec` (which creates interface constructors),
`construct_calls_constructor`, and `create_builtin_function_and_call`.

The `FunctionObjectBuilder::constructor(true)` + `from_copy_closure_with_captures`
path sets `NativeFunctionObject.constructor = Some(ConstructorKind::Base)`,
which causes `NativeFunctionObject::internal_methods()` to return the
`&CONSTRUCTOR` vtable (including `native_function_construct`).

### 4. 🔴 15 unexpected Boa WPT failures — introduced by migration

81 executed, 66 PASS.  The 15 unexpected regressions were introduced by
the generic JS layer migration.  The goal for Boa is
**zero unexpected failures** — every migration regression must be fixed.

Breakdown:
- 13 readable-stream tests: `TypeError: not a callable function` — Boa
  promise microtask issue (the stream's `pull` algorithm sees an
  uninitialized function because microtasks aren't flushed at the right
  points)
- 2 wasm branding tests: `Module.exports: argument is not a
  WebAssembly.Module` — wasm module internal slot not wired through
  `create_builtin_function`

### 3. 🟡 JSC backend not functional

JSC compiles and launches but `addEventListener` is missing, the content
process loops at 100% CPU, and WPT tests time out.  Pre-existing
condition; full JSC integration deferred.

## Tasks for migration completion

1. **🔴 `create_builtin_function` doesn't produce constructable functions** —
   `js_engine/src/boa/engine.rs`.  The `FunctionObjectBuilder::constructor(true)`
   + `build()` path works for `NativeFunction::from_fn_ptr` but not for
   `from_copy_closure_with_captures`.  The returned function lacks
   `[[Construct]]` despite the flag being set.  Likely fix: switch back to
   `from_fn_ptr` and thread captures through a different mechanism, or
   use `ConstructorBuilder` instead of `FunctionObjectBuilder`.

2. **🔴 Fix attribute accessor descriptor registration** —
   `define_regular_attributes` builds accessor descriptors but the
   properties never appear on the prototype.  Operations (value descriptors)
   work fine.  Needs comparison of data-descriptor vs accessor-descriptor
   paths in `define_property_or_throw` → Boa native conversion.

3. **🟡 Fix `location.rs` direct downcast** —
   `self.window.downcast_ref::<Window>()` always returns `None`.  Needs
   an `ec` parameter threaded through Location navigation methods.

4. **🔍 Audit remaining direct `downcast_ref` calls** — Find and convert
   all remaining `JsObject::downcast_ref::<T>()` calls that bypass
   `ec.with_object_any()`.

5. **Fix readable-stream WPT failures** — Boa promise microtask issue
   ("TypeError: not a callable function"). Introduced by migration.

6. **Restore JSC backend** — Wire `addEventListener`/DOM event
   infrastructure on JSC; fix the content-process infinite loop.

7. **Prune historical notes** — Remove Category 1-8 fix attempts, GC
   tracing investigations, and per-test WPT inventories.
