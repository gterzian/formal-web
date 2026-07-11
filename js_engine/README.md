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

## JSC backend current state (2026-07-11)

### Build — Boa backend (default)

```bash
# Full build (root binary + content/net/media helper processes + WPT runner)
rustup run 1.94.0 cargo build --release

# Check only (fast)
rustup run 1.94.0 cargo check

# Build just the js_engine crate
rustup run 1.94.0 cargo build --release -p js_engine
```

### Build — JSC backend (macOS only)

The `js_engine` crate compiles and all 18 unit tests pass on JSC.
The `content` crate previously had pre-existing compilation errors on both
backends (`iframe_object` mutability).  These were fixed in the 2026-07-11
session.

The root `formal-web` binary and WPT runner cannot be built with JSC until
the `content` crate errors are resolved.

```bash
# js_engine crate only (compiles, all tests pass)
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine
rustup run 1.94.0 cargo test --no-default-features --features jsc -p js_engine

# content crate (does NOT compile — 4 pre-existing errors)
rustup run 1.94.0 cargo check --no-default-features --features jsc -p content
```

### WPT test inventory (JSC, 2026-07-11)

All WPT tests from `tests/wpt/include.ini` and `tests/formal/include.ini`:

**Passing:**
- CSS.supports (3/3)
- DOM nodes: Element-hasAttribute, Element-insertAdjacentText, Element-remove
- HTML: document.title (3), document-dir, iframe (2), anchor (2)
- Formal: gc-protection
- Streams: ReadableStream constructor, construct-byob-request, tee-locked-stream

**`Promise.resolve` `this` binding (FIXED 2026-07-11):**
`promise_resolve()` was passing `undefined` as `thisObject` to the cached
`Promise.resolve` function.  `Promise.resolve` requires `this` to be the
Promise constructor.  Passing undefined/null caused JSC to substitute the
global object, which is not a constructor, throwing
`|this| is not an object`.

Fixed by passing `constructor.raw` as the `thisObject` parameter to
`JSObjectCallAsFunction`.

**WPT test status after fix (2026-07-11, 98 tests):**

**PASS (30):**
- CSS.supports (3), DOM Element tests (3)
- HTML: document.title (3), document-dir, iframe (2), anchor (2)
- Streams readable: bad-strategies, cancel, constructor, count-queuing-strategy,
  crashtests/garbage-collection, default-reader, floating-point-total-queue-size,
  garbage-collection, read-task-handling, reentrant-strategies
- Streams piping: general-addition, throwing-options
- Streams readable-byte: construct-byob-request, tee-locked-stream
- Streams transform: formal-debug-order, formal-debug-terminate, patched-global,
  properties
- Streams writable: bad-strategies, bad-underlying-sinks, byte-length-queuing-strategy,
  constructor, count-queuing-strategy, error, floating-point-total-queue-size,
  properties, start
- WASM: validate

**ERROR (SIGSEGV/SIGBUS crash — content process dies) (28):**
- Most piping tests, readable-streams with complex async, transform-streams
- Root cause: likely GC protection of JS objects held by Rust across async
  boundaries (callbacks, promise reactions).  The `Callback` struct in
  `content/src/webidl/callback.rs` lacks `JSValueProtect`/`JSValueUnprotect`
  on JSC, so callbacks can be GC'd while Rust still holds references.
  See "Remaining work" in Phase 3 below.

**FAIL (no crash, spec-compliance issues) (6):**
- readable-byte-streams: enqueue-with-detached-buffer, patched-global
- readable-streams: from, patched-global
- structured-clone.any.js (Blob not implemented)
- dom/nodes/Node-constants.html

**Pre-existing expected failures (metadata):**
- Various streams tests with TODO metadata (BYOB, cross-realm, transferable)

### Unit tests


```bash
# Boa: content-level integration tests
rustup run 1.94.0 cargo test -p content generic_js_test

# JSC: js_engine-level unit tests only (18 pass)
rustup run 1.94.0 cargo test --no-default-features --features jsc -p js_engine

# JSC: content-level tests require content to compile (currently broken)
# rustup run 1.94.0 cargo test --no-default-features --features jsc -p content generic_js_test
```

### WPT

```bash
# Boa (default — full WPT suite)
rustup run 1.94.0 cargo run --release -- wpt

# Boa — single test
rustup run 1.94.0 cargo run --release -- wpt dom/nodes/Element-hasAttribute.html

# JSC — not available until content crate compiles on JSC
```

Latest Boa WPT result (2026-07-11): `executed=84 unexpected=0`

### What works on each backend

**Boa (default):** All 83 WPT tests pass. Full stream/DOM/promise/WASM support.

**JSC (js_engine crate tests only):**
- Value construction, type conversion, error construction
- Property access, prototype manipulation
- Promise capability + resolve
- ArrayBuffer allocation
- Script evaluation
- GC root survival under pressure

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

### Session investigation log

#### 2026-07-11 — Callback GC protection for JSC backend

**Files changed:**
- `content/src/webidl/callback.rs` — Added `root: Option<Rc<GcRootHandle<Types>>>` field
  (JSC-only), changed `from_object()` to take `ec` and protect the value.
- `content/src/streams/strategy.rs` — Pass `ec` to `Callback::from_object()`.
- `content/src/streams/writablestreamdefaultcontroller.rs` — Pass `ec` to
  `Callback::from_object()`.
- `content/src/streams/readablestreamdefaultcontroller.rs` — Pass `ec`.
- `content/src/streams/transformstream.rs` — Pass `ec` (7 call sites).
- `content/src/html/window_or_worker_global_scope.rs` — Pass `ec`.
- `content/src/js/bindings/html/html_iframe_element.rs` — Fixed `iframe_object`
  mutability compilation errors (`mut iframe` bindings).
- `js_engine/README.md` — Updated status.
- `tests/formal/include.ini` — Added `callback-gc-protection.html`.
- `tests/formal/tests/callback-gc-protection.html` — New test (10 sub-tests).

**Instrumentation added:** `Rc<GcRootHandle<Types>>` field in `Callback` struct.
On JSC, `create_root()` calls `JSValueProtect` on construction; the `Rc`
refcount keeps the protection alive across all `Clone`/`Drop` cycles. Only
the final `Drop` (when all references are released) calls `JSValueUnprotect`.
On Boa, `create_root()` is a no-op and the `root` field doesn't exist.

**What was confirmed:**
- `Callback` is used in `SourceMethod` (stream pull/write/close/abort/transform/flush
  algorithms), `TimerHandler` (setTimeout/setInterval), and event listeners.
- All call sites have `ec` available for the new `from_object` signature.
- Full WPT suite passes: `executed=84 unexpected=0` (previously 83 — new
  `callback-gc-protection.html` test added).
- New callback GC protection test has 10 sub-tests that all pass:
  - ReadableStream pull callback survives GC
  - ReadableStream cancel callback survives GC
  - WritableStream write/close/abort callbacks survive GC
  - TransformStream transform/flush callbacks survive GC
  - Multiple GC cycles with repeated callback invocation
  - Strategy size callback survives GC

**What was ruled out:**
- Using `GcRootHandle` directly (without `Rc`) would not handle clones
  correctly — on JSC, `GcRootHandle::clone()` creates a new handle without
  an unroot action, so dropping the clone would lose the protection.
- Using `from_object` without `ec` would not allow protection on JSC.

**Not investigated:**
- `WindowTimer.arguments` (`Vec<JsValue>`) elements are not individually
  protected on JSC. These are JS values stored alongside timers and could
  be GC'd while a timer is pending. Same pattern affects any `Vec<JsValue>`
  or `Vec<JsObject>` stored in Rust-owned structs.

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

### Phase 3: IntrinsicsCache — replace `eval_script_raw` with cached native function references

Added `Intrinsics` struct and `resolve_global_path` helper on `JscEngine`, replacing per-call
`JSEvaluateScript` with one-time native property walks + `JSObjectCallAsFunction`.

| Eval site | Replacement |
|---|---|
| `is_array` | Cached `Array.isArray` function ref |
| `json_stringify` | Cached `JSON.stringify` function ref |
| `promise_resolve` | Cached `Promise.resolve` function ref |
| `array_push` | Cached `Array.prototype.push` function ref |
| `map_set_entry` | Cached `Map.prototype.set` function ref |
| `set_add_entry` | Cached `Set.prototype.add` function ref |
| `to_bigint`, `string_to_bigint`, `value_from_bigint` | Cached `BigInt` function ref (args passed as real `JSValueRef`, no source text) |
| `own_property_keys` | Cached `Reflect.ownKeys` function ref |
| `get_own_property` | Cached `Object.getOwnPropertyDescriptor` function ref |
| `allocate_shared_array_buffer` | Cached `SharedArrayBuffer` constructor |
| `create_proxy` | Cached `Proxy` constructor |
| `construct_data_view_from_buffer` | Cached `DataView` constructor |
| `map_get_entries` | Cached `Map.prototype.entries` + native iteration via `iterator_step_value` |
| `set_get_values` | Cached `Set.prototype.values` + native iteration via `iterator_step_value` |

### Remaining work

| Area | What needs doing | Status |
|---|---|---|
| `Callback` struct | `Rc<GcRootHandle>` protects `JsObject` across Clone/Drop. | Fixed (2026-07-11) |
| `TimerHandler::Function` | Uses `Callback` which is now protected. Covered. | Fixed (inherits Callback fix) |
| `WindowTimer.arguments` | `Vec<JsValue>` elements need protection. Same pattern applies to any `Vec<JsValue>` in Rust-owned structs. | Unfixed |
| Event listeners | Uses `Callback` which is now protected. Covered. | Fixed (inherits Callback fix) |
| Stream callbacks | `SourceMethod` wraps `Callback`. Covered. | Fixed (inherits Callback fix) |
| Constructor Proxy eval | Cannot be eliminated (JSC C API has no `JSProxyCreate`). | Acknowledged |
| `promise_state()` eval | `JSPromiseGetStatus` not in public C API. | Acknowledged |

### Phase 4: Verify pass-rate parity

After all remaining items, run the full WPT suite on both backends and confirm
`executed=N unexpected=0` on both.  Any remaining discrepancy needs either
a JSC bug fix (preferred) or shared metadata (last resort).

### Known JSC gaps (remaining after 2026-07-11 fixes)

| Gap | Reason | Status |
|---|---|---|
| `Object.freeze`/`seal`/`isFrozen`/`isSealed` via cached intrinsics | Implemented via cached `Object.freeze`/`Object.seal` etc. ✓ Works correctly. | Fixed |
| `to_property_descriptor` | Implemented using native property gets. ✓ | Fixed |
| `get_iterator` async kind + method parameter | Async path returns sync iterator (no `AsyncFromSyncIterator` wrapper). Method parameter supported. | Partial |
| `new_promise_capability` | Still uses eval for the executor arrow function (JSC C API has no native Promise construction). Constructor call goes through cached `Promise` constructor. | Partial (acknowledged hard problem) |
| `define_property_or_throw` | Now uses cached `Object.defineProperty` + native descriptor object. | Fixed |
| `FUNCTION_REGISTRY` dead code | Removed. | Cleaned |
| `registry_call_as_function` dead code | Removed. | Cleaned |
| `get_iterator` `CreateAsyncFromSyncIterator` | Requires creating an `AsyncFromSyncIterator` wrapper with async next/return/throw methods. Not implemented. | Unfixed |
| `set_current_engine`/`clear_current_engine` nesting | Public API does not nest (unlike `EngineGuard`). No callers yet outside the module. | Documented |
| Constructor Proxy eval | Cannot be eliminated (JSC C API has no `JSProxyCreate`). | Acknowledged |
| `promise_state()` eval | `JSPromiseGetStatus` not in public C API. | Acknowledged |

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
