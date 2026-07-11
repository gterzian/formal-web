# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Safe builtin function creation

Unsafe trait methods that stored closure captures in a no-op GC trace wrapper
have been removed.  Use these safe alternatives:

- **`create_builtin_fn_static(behaviour, length, name)`** — stateless `fn` pointers.
- **`create_builtin_fn_with_captures(ec, captures, behaviour_fn, ...)`** — stateful
  functions where `captures: C` is a concrete `boa_gc::Trace + 'static` type.

The deprecated `create_builtin_fn`/`create_builtin_function` remain on the trait
with no-op trace via `UnsafeFnBox` for migration.  Use the safe APIs in new code.

## JSC backend current state (2026-07-10)

### Build
```bash
# JSC backend (macOS)
rustup run 1.94.0 cargo build --release --no-default-features --features jsc

# Boa backend (default)
rustup run 1.94.0 cargo build --release
```

### Unit tests
```bash
cargo test --no-default-features --features jsc -p content generic_js_test   # 105 pass (JSC)
cargo test -p content generic_js_test                                          # 103 pass (Boa)
```

### WPT
```bash
# Boa
PYTHON=python3.12 cargo run --release -- wpt

# JSC (prebuilt binaries — use direct wpt runner)
PYTHON=python3.12 target/release/formal-web-wpt dom/nodes/Element-hasAttribute.html
```

### Working
- Global methods: `addEventListener`/`removeEventListener`/`dispatchEvent`,
  `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval`,
  `requestAnimationFrame`/`cancelAnimationFrame`.
- DOM events dispatch with correct GC rooting.
- `ReadableStream`, `TransformStream`, `WritableStream` constructors and
  basic operations (enqueue, read, cancel, transform).
- `Promise.resolve().then(...)` chains, `perform_promise_then` result_capability.
- `new WebAssembly.Module()`, `new WebAssembly.Instance()` sync path.
- `window.open()` with multi-realm via `new_shared_realm()`.
- TypedArray, DataView, ArrayBuffer operations via JSC C API.
- All DOM/CSS/HTML `include.ini` WPT tests pass (17/17).
- All readable/writable/transform stream WPT tests pass.
- Edge piping test `general-addition` passes.

### Remaining JSC limitations

**Function.prototype inheritance (macOS 26):**
`JSObjectSetPrototype` crashes on `JSObjectMake`-created objects with
`callAsFunction`/`callAsConstructor` callbacks.  Both constructors and
non-constructors copy `bind`/`call`/`apply`/`toString` from
`Function.prototype` as own properties.  `JSObjectMakeFunctionWithCallback`
is avoided because on macOS 26 JSC passes a different function pointer
to the callback than the stored key in `FUNCTION_REGISTRY`.

**Known issues:**
- `define_property_or_throw` uses `Object.defineProperty` via script eval.
- Global object prototype immutable; properties copied to global at setup.
- `instanceof Window` returns `false` (global [[Prototype]] immutable).
- `structuredClone` returns `null` (Blob not implemented).
- Async `WebAssembly.compile()`/`WebAssembly.instantiate()` times out
  (requires event loop for background compilation).
- Piping tests that use `delay()` (`step_timeout`/`setTimeout`-based async)
  time out on JSC but pass on Boa (root cause not yet identified).
- Byte-stream tests disabled per meta annotations (pre-existing Boa issue:
  GcRefCell BorrowError on re-entrant `byobRequest` property access).
- `Object.getOwnPropertyDescriptor`/`Object.getOwnPropertyNames` fail on
  `create_object_with_any`-created prototype objects (Boa exotic-object
  limitation).

## Dead-end investigations

### WPT stream null-prototype bug (2026-07-09, Boa)

`create_read_result` and `create_iterator_result_object` used
`ec.create_plain_object(None)`, creating null-prototype objects.  WPT's
`assert_object_equals` calls `actual.hasOwnProperty(p)` (testharness.js),
which produced `TypeError: not a callable function` because
`hasOwnProperty` was `undefined`.  This affected all readable-stream tests
involving reading, canceling, teeing, or async iterating.

**Fix:** Pass `&intrinsics.object_prototype` to `create_plain_object` in all
three iter-result creation sites.

**Ruled out before finding root cause:** GC collection / trace chain issues,
`ec.call()` producing the error, `create_builtin_fn` capture auditing,
Boa `force_collect()`, `run_jobs()` errors.

### call_pull_if_needed error propagation (2026-07-09, Boa)

When the pull algorithm throws, `call_pull_if_needed` must error the stream
synchronously instead of propagating the error with `?`.  Before the fix,
`tee.any.js` and `bad-underlying-sources.any.js` left branch streams in a
readable state with pending promises that never settled (timeout).

### read-min.any.js — BorrowError during BYOB recursion (Boa)

```
pull_into → call_pull_if_needed → pull_algorithm.call(&controller_object, ec)
  → JS pull fn → c.byobRequest → getter → respond(2)
    → early return → call_pull_if_needed [RE-ENTRANT]
      → pull_algorithm.call(&controller_object, ec)
        → JS pull fn → c.byobRequest
          → __get__ → ordinary_get_own_property → obj.borrow() → PANIC
```

The conflicting mutable borrow is from Boa's internal VM machinery (IC cache
or vtable dispatch).  Disabled with metadata.  Band-aids rejected: microtask
deferral, `try_borrow` in `with_object_any`, caching controller data.

### JSC `FUNCTION_REGISTRY` pointer mismatch (2026-07-10)

`JSObjectMakeFunctionWithCallback` on macOS 26 passes a different function
pointer to `registry_call_as_function` than the one returned by object
creation, causing `FUNCTION_REGISTRY` lookups to fail for all method calls
(`reader.read()` returned `undefined`).  Fixed by switching non-constructor
functions to `BUILTIN_CLASS` with private-data storage
(`JSObjectGetPrivate`).

### callAsConstructor ABI mismatch (2026-07-10)

The Rust FFI binding for `JSClassDefinition.callAsConstructor` used
`JSObjectCallAsFunctionCallback` (6 params with `thisObject`), but the
actual C API's `JSObjectCallAsConstructorCallback` takes 5 parameters.
This shifted all argument registers, causing garbage reads that manifested
as SIGBUS in `invoke_stored_behaviour`.

### JSCEngine Drop order (2026-07-10)

`JscEngine`'s `context` field (releasing `JSGlobalContextRef`) was dropped
before `host_data` (containing `GcRootHandle` unroot closures).  The unroot
closure's `ctx_raw` became dangling, causing SIGSEGV.  Fixed by explicit
`Drop for JscEngine` that clears `host_data` and `queued_jobs` first.

## Completed refactoring (2026-07-11)

### TestUtils namespace (`gc()` method)

Implemented the `TestUtils` namespace per
<https://testutils.spec.whatwg.org/#the-testutils-namespace>:

- Added `fn gc(&mut self)` to the `EcmascriptHost` trait (`js_engine/src/engine.rs`)
- **JSC:** calls `JSGarbageCollect(self.ctx_ptr())`
- **Boa:** calls `boa_gc::force_collect()`
- **Delegating wrappers:** `EcDispatchHost`, `EnvironmentSettingsObject`,
  `BlitzJSEventHandler` all forward to their inner engine
- Proper folder structure: `content/src/testutils/` (domain) +
  `content/src/js/bindings/testutils/` (JS bindings), following the
  same pattern as `css/` and `streams/`
- Added `JSGarbageCollect` to `jsc_sys.rs` FFI bindings
- Test pages at `scratchpad/gc-protection-test.html` and
  `scratchpad/jsc-protection-test.html`

### Phase 1: `JSValueProtect`/`JSValueUnprotect` everywhere

`create_root` already used `JSValueProtect`/`JSValueUnprotect`.  Three additional
sites were converted from the old global-object-property rooting pattern:

1. **`drain_noop`** — `JSValueProtect` on creation in `drain_microtasks()`,
   `JSValueUnprotect` in `Drop for JscEngine`.
2. **`new_promise_capability`** — Replaced `__fw_pcap_root_{id}_{tag}` global
   properties with temporary `JSValueProtect`/`JSValueUnprotect` around the
   three promise-capability values.  The caller (`create_promise_capability`
   in `engine.rs`) adds permanent protection via `create_root`.
3. **`create_object_with_any`** — Replaced `__fw_any_root_{id}` global
   properties with `JSValueProtect`.  Protected object pointers tracked in
   `JscEngine.protected_objects` for cleanup in `Drop`.

A new `protected_objects: Vec<*mut JSValueRef>` field was added to `JscEngine`
to track JSValueProtect'd objects for cleanup on engine teardown.

### Phase 2: Replace `JSEvaluateScript` with native C API

| Change | File | Description |
|---|---|---|
| `make_builtin_function` bind/call/apply copy | `jsc/engine.rs` | New `copy_function_prototype_methods()` helper uses `JSObjectGetProperty(Function.prototype, name) + JSObjectSetProperty(...)`. |
| `make_builtin_function` toString | `jsc/engine.rs` | New `set_builtin_to_string()` creates one shared BUILTIN_CLASS function (reads `this.name` at call time) cached in a thread-local, avoiding per-function JSEvaluateScript.  `.name` set as `ReadOnly|DontEnum` on every builtin. |
| `create_builtin_fn_with_captures` `.length` | `jsc/engine.rs` | Direct `JSObjectSetProperty(func, "length", value, ReadOnly \| DontEnum)` on the standalone function path.  The trait impl methods (`create_builtin_function`, `create_builtin_fn_static`) only gained `.length` support in the 2026-07-11 critical-fixes pass. |
| `get_fn_call()` | `jsc/engine.rs` | Traverses `Function → prototype → call` via C API instead of `eval("Function.prototype.call")`. |

### Critical fixes (2026-07-11)

Fixed spec-correctness bugs in the JSC backend (`js_engine/src/jsc/engine.rs`):

| Finding | Function(s) | Fix |
|---|---|---|
| ArrayBuffer memory leak | `allocate_array_buffer`, `clone_array_buffer` | Added real `free_array_buffer_data` deallocator callback with `Box<Vec<u8>>` context.  Added missing `deallocatorContext` parameter to `JSObjectMakeArrayBufferWithBytesNoCopy` sys binding. |
| `GetV` broken for primitives | `get_v` | Now calls `to_object(value)?` then `get(object, key)` per spec, matching Boa backend. |
| `ToIndex` never throws RangeError | `to_index` | Now throws `RangeError` for values outside `0..2^53-1` (including `+Infinity`). |
| `ToLength` wrong clamp | `to_length` | Now clamps to `2^53 - 1` instead of `u32::MAX`. |
| `SameValue`/`SameValueZero` NaN wrong | `same_value`, `same_value_zero` | Both now return `true` for `NaN` vs `NaN`. |
| Error constructor quote escaping | `new_type_error`, `new_range_error`, `new_syntax_error` | Escape single quotes (matching the JS string delimiter) instead of double quotes. |
| Raw values thrown instead of Errors | `require_object_coercible`, `to_object`, `get_iterator`, `evaluate_module` | Now throw proper `TypeError` objects via `new_type_error()`. |
| `iterator_close` missing Object check | `iterator_close` | Now validates that `return()` result is an Object per spec step. |
| `.length` not set on builtin functions | `create_builtin_function`, `create_builtin_fn_static` | Both now set `Function.length` via `JSObjectSetProperty` with `ReadOnly\|DontEnum`. |

### Remaining work

| Area | What needs doing |
|---|---|
| `Callback` struct | `content/src/webidl/callback.rs` — Protect on `from_object()` / Clone, unprotect on Drop.  Needs `ec` arg for context. |
| `TimerHandler::Function` | `content/src/html/global_scope.rs` — Store protected handle alongside callback. |
| `WindowTimer.arguments` | `content/src/html/global_scope.rs` — `Vec<JsValue>` elements need protection. |
| Event listeners | `content/src/dom/event.rs` — Protected `Callback` in listener records. |
| Stream callbacks | stream controllers — Protected `Callback` in controller state. |
| Constructor Proxy eval | Cannot be eliminated (JSC C API has no `JSProxyCreate`). |
| `promise_state()` eval | `JSPromiseGetStatus` not in public C API. |

### Phase 4: Verify pass-rate parity

After all remaining items, run the full WPT suite on both backends and confirm
`executed=N unexpected=0` on both.  Any remaining discrepancy needs either
a JSC bug fix (preferred) or shared metadata (last resort).

- **Piping test timeouts on JSC** — Tests using `delay()` (via
  `step_timeout`/`setTimeout`) time out on JSC but pass on Boa.  Root cause
  not identified.
- **WASM worker-context tests** — `WebAssembly.compile`/`instantiate` require
  a `Window` global object for IPC dispatch; workers use
  `DedicatedWorkerGlobalScope`.
- **Transferable streams** (8 files) — `JsTypes` trait lacks primitives for
  structured serialization of stream internals.
- **Queuing-strategy / IDL edge cases** (3 files) — cross-realm constructor
  behavior, size function identity, IDL harness setup.
- `instanceof Window` returns `false` — global `[[Prototype]]` immutable on
  JSC; properties are copied to global instead.
