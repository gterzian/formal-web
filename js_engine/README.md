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

### 1. 🔴 Missing anchor activation behavior

`HTMLAnchorElement::activation_behavior` exists but is never wired to the
event dispatch layer.  `EventDispatchHost::has_activation_behavior` and
`run_activation_behavior` have default no-op implementations in
`dispatch.rs`, and `UiEventDispatchHost` in `ui_event_dispatch.rs` does
not override them.  Clicking an `<a href="...">` dispatches the event
but never navigates.

**Risk:** Other trait-method overrides or callback wire-ups may have been
silently lost.  The Rust compiler does not warn about default trait
methods returning `false` or `Ok(())`.

### 2. 🟡 15 unexpected Boa WPT failures

81 executed, 66 PASS.  The 15 unexpected are all pre-existing:
readable-stream tests fail with "TypeError: not a callable function"
(Boa promise microtask issue), plus wasm branding failures.  Not
introduced by this refactor.

### 3. 🟡 JSC backend not functional

JSC compiles and launches but `addEventListener` is missing, the content
process loops at 100% CPU, and WPT tests time out.  Pre-existing
condition; full JSC integration deferred.

## Tasks for migration completion

1. **Diff against `main`** — Compare every file touched by the refactor
   against `main` to catch silently-lost hook overrides, trait method
   implementations, and callback wire-ups.  Focus on `EventDispatchHost`,
   `UiEventDispatchHost`, and similar hook traits with default no-ops.

2. **Fix anchor activation** — Override `has_activation_behavior` and
   `run_activation_behavior` in `UiEventDispatchHost` to detect
   `HTMLAnchorElement` targets.

3. **Fix readable-stream WPT failures** — Resolve the pre-existing Boa
   promise microtask issue causing "TypeError: not a callable function"
   in 47 readable-stream tests.

4. **Restore JSC backend** — Wire `addEventListener`/DOM event
   infrastructure on JSC; fix the content-process infinite loop.

5. **Prune historical notes** — Remove the extensive Category 1-8 fix
   attempts, GC tracing investigations, and per-test WPT inventories
   from this README once the above tasks are complete.
