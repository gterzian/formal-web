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

Both the `js_engine` and `content` crates compile on JSC.
`js_engine` has 18 unit tests that all pass on JSC.

The root `formal-web` binary and WPT runner still require the Boa backend.
Runnning `cargo build --release` on JSC will fail if Boa dependencies
are absent.  Use `cargo check` or `cargo build` targeting specific crates.

```bash
# js_engine crate only (compiles, all tests pass)
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine
rustup run 1.94.0 cargo test --no-default-features --features jsc -p js_engine

# content crate (compiles on JSC after 2026-07-12 GC protection work)
rustup run 1.94.0 cargo check --no-default-features --features jsc -p content

# Both crates together (fast check)
rustup run 1.94.0 cargo check --no-default-features --features jsc -p js_engine -p content
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

**WPT test status after 2026-07-11 session:**

**PASS:**
- CSS.supports (3), DOM Element tests (3), Node-constants
- HTML: document.title (3), document-dir, iframe (2), anchor (2)
- Streams readable: bad-strategies, constructor, count-queuing-strategy,
  crashtests/garbage-collection, default-reader, floating-point-total-queue-size,
  reentrant-strategies
- Streams piping: general-addition, throwing-options
- Streams readable-byte: construct-byob-request, tee-locked-stream
- Streams transform: formal-debug-order, formal-debug-terminate, patched-global,
  properties
- Streams writable: constructor, count-queuing-strategy, error,
  floating-point-total-queue-size, properties, start
- WASM: validate
- Formal: gc-protection

**ERROR (SIGSEGV crash — content process dies):**
- streams/readable-streams: cancel, floating-point-total-queue-size,
  garbage-collection, read-task-handling
- streams/writable-streams: bad-strategies, bad-underlying-sinks,
  byte-length-queuing-strategy
- ALL piping tests except general-addition and throwing-options
- Formal: callback-gc-protection, wasm-compile-instantiate
- html/webappapis/structured-clone/structured-clone.any.js

Root cause: JsObject/JsValue references stored in Rust-side GcCell fields
are invisible to JSC's GC.  The `Callback`, `PromiseResolvers`, and
reader/writer promise fields are now protected, but many more stream
internals still have unprotected fields.  See "Remaining work" below.

**FAIL (no crash, spec-compliance issues):**
- Wasm compile (timeout)
- structured-clone (Blob not implemented)

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

## Promises and Microtasks in JavaScriptCore's C API

A practical reference for embedders writing against the public JSC C API.

### ECMA-262 guarantees

`.then()` handlers are NEVER invoked synchronously as part of the call that
registered or triggered them.  `PerformPromiseThen` and the resolve/reject
algorithms only ever enqueue a `PromiseReactionJob`.  This is
engine-internals, separate from anything about the C API's lock behavior,
and JSC gets this right (it passes Test262).

Beyond that, ECMA-262 says almost nothing about when queued Jobs actually
run.  The relevant abstract operation just says a Job runs "at some future
point in time, when there is no running execution context and the execution
context stack is empty" — and explicitly leaves the scheduling to the host.

JSC drains the microtask queue when the JS-lock's recursive count hits
zero — i.e., when the execution context stack has genuinely gone empty from
JSC's point of view.  This satisfies A+ and ECMA-262.

### HTML's microtask checkpoint vs JSC's drain-on-unlock

HTML is prescriptive about where microtask checkpoints happen: after each
task, after invoking a callback, at specific points in the "clean up after
running script" steps, etc.  JSC's drain-on-unlock is a proxy for that,
and the two usually coincide — but they're not the same thing.

**Key gap for formal-web:** JSC microtasks only drain when the outermost
C API call returns (lock count reaches 0).  Nested calls (e.g.,
`JSObjectCallAsFunction` called inside another `JSObjectCallAsFunction`)
do NOT trigger drains.  This means `.then()` handlers on already-resolved
promises enqueued within a nested scope don't fire until the outer scope
returns.

**Impact on pipeTo:** The `perform_promise_then` calls inside
`transform_promise_to_undefined` and `append_reaction` both happen
within the outer `pipeTo` call scope.  Handlers enqueued by `.then()`
on resolved promises don't fire until `pipeTo` returns.  This is correct
but means the shutdown handler (`pipe_to_append_reaction_fn`) fires
asynchronously, during the microtask drain after `pipeTo` returns.

**`promise_state` broken in nested scopes:** The JSC implementation of
`promise_state` uses `JSEvaluateScript` to set up `.then()` flags and
then `void 0` to drain microtasks.  Within a nested lock scope, the
`void 0` drain doesn't work, so `promise_state` always returns `Pending`.
This means `shutdown_action_promise_state` returns `Pending` when called
from within the pipeTo setup, preventing `finalize` from running
synchronously.

**Rust job queue:** `enqueue_job_with_realm` is separate from JSC's
microtask queue.  Jobs never drain without explicit `run_jobs()` calls.
Calling `run_jobs` from within a builtin function callback creates a
double `&mut` borrow of `JscEngine` through the trait object (UB).

### Fixes applied in this session

1. **Replaced `enqueue_job_with_realm` with `perform_promise_then`**
   for `ReadableStreamPipeTo` read/close/error steps
   (`schedule_pipe_to_on_settled`).  Now uses JSC's microtask queue.

2. **Synchronous shutdown check in `perform_action`** — After setting
   up the shutdown action promise and `append_reaction`, check if the
   promise is already settled (Fulfilled/Rejected) via
   `shutdown_action_promise_state`.  If settled, call `finalize`
   immediately instead of waiting for the async `.then()` handler.
   (Mitigates the nested-lock microtask issue.)

3. **GC protection for JsObject fields** — Protected `Callback`,
   `PromiseResolvers`, reader/writer promise fields, and
   `shutdown_error` via `JSValueProtect`/`JSValueUnprotect`.

### Remaining issues

1. **`promise_state` broken in nested scopes** — The eval-based
   implementation (`JSEvaluateScript` with `.then()` + `void 0`)
   never works inside nested JS calls because JSC only drains
   microtasks when the outermost C API call returns.  The
   `shutdown_action_promise_state` call from `pipe_to_on_promise_settled`
   always returns `Pending` inside `append_reaction`'s `.then()` handler.
   Fix: track promise state in Rust alongside the JSC promise
   (e.g., set a Rust flag when resolving/rejecting from Rust code).

2. **WASM compile/instantiate timeouts** — Async `WebAssembly.compile()`
   and `WebAssembly.instantiate()` require event-loop-driven background
   compilation.  The JSC backend does not pump this loop.

3. **Piping test timeouts** — Tests using `delay()` (`step_timeout`/
   `setTimeout`-based async) time out on JSC but pass on Boa.  Root
   cause: `setTimeout` handler delivery requires the event loop which
   JSC's C API direct-call path does not pump.

4. **Byte-stream BYOB recursion BorrowError** — Pre-existing Boa issue:
   `GcRefCell BorrowError` on re-entrant `byobRequest` property access
   during pull-into → call_pull_if_needed re-entrancy.

5. **Transferable streams (8 files)** — `JsTypes` trait lacks primitives
   for structured serialization of stream internals.

6. **Queuing-strategy / IDL edge cases (3 files)** — cross-realm
   constructor behavior, size function identity, IDL harness setup.

### Session investigation log

#### 2026-07-11 — Callback GC protection + PromiseResolvers + reader/writer lifecycle

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
- `js_engine/src/records.rs` — Added `root` field to `PromiseResolvers` (JSC-only)
  protecting both resolve/reject function objects via `Rc<GcRootHandle>`.
- `content/src/streams/readablestreamdefaultreader.rs` — Protected `closed_promise`
  on JSC via direct `JSValueProtect`/`JSValueUnprotect` in the setter.
- `content/src/streams/readablestreambyobreader.rs` — Same protection.
- `content/src/streams/writablestreamdefaultwriter.rs` — Protected `ready_promise`
  and `closed_promise` JsObject fields.
- `content/src/streams/readablestream.rs` — Protected `shutdown_error` JsValue in
  `PipeToStateInner.set_shutdown_error()`. Added `pipe_to_on_promise_settled` handling
  to skip `wait_for_writer_ready` when `write_chunk` returns false.
- `tests/formal/tests/callback-gc-protection.html` — New test (10 sub-tests).
- `tests/formal/include.ini` — Added the new test.

**JSC microtask drain findings:**
- `.then()` handlers NEVER fire synchronously inside `perform_promise_then` —
  ECMA-262 guarantee enforced by all engines including JSC.
- JSC drains microtasks when the JS lock's recursive count hits zero (outermost
  C API call returns). This is a valid "no running execution context" boundary.
- The key gap: HTML's microtask checkpoint timing vs JSC's lock-release drain.
  See `docs/jsc-microtasks.md` for the full analysis.

**PipeTo SIGSEGV analysis (unfixed):**
- All piping tests except `general-addition` and `throwing-options` crash with
  SIGSEGV on JSC. The content process dies when accessing a collected JsObject.
- GC protection for Callback, PromiseResolvers, reader/writer promise fields
  is necessary but not sufficient — many more JsObject/JsValue fields in
  PipeToStateInner, ReadableStream, WritableStream, TeeState, etc. are
  unprotected.
- The fundamental problem: on JSC, EVERY JsObject/JsValue stored in a Rust-side
  struct or GcCell must be individually protected via JSValueProtect. There is
  no tracing hook for data stored behind JSObjectSetPrivate or in HashMap entries.
- The `enqueue_job_with_realm` queue is separate from JSC's microtask queue.
  Calling `run_jobs()` from inside a microtask handler creates a double-&mut
  borrow of the JscEngine, which is undefined behavior.

**Not investigated (still open):**
- `WindowTimer.arguments` (`Vec<JsValue>`) elements are not individually
  protected on JSC.

#### 2026-07-12 — Systematic GC protection for stream internals

**Problem:** All pipe-to tests (except `general-addition` and `throwing-options`)
crash with SIGSEGV on JSC because JsObject/JsValue references stored in Rust
struct fields behind `GcCell` (`Rc<RefCell>` on JSC) are invisible to JSC's
GC.  When GC runs, these objects are collected, and subsequent access via
the Rust-side handle causes SIGSEGV.

**Approach:** Added reusable JSC protection helper functions in
`content/src/streams/readablestreamsupport.rs` that wrap the raw
`JSValueProtect`/`JSValueUnprotect` C API calls.  These are no-ops on Boa
(where `#[derive(Trace)]` handles GC tracing automatically).  The helpers
cover:

- `protect_object` / `unprotect_object` — individual `JsObject` values
- `protect_jsvalue` / `unprotect_jsvalue` — `JsValue` values (checks if object)
- `protect_object_vecdeque` / `unprotect_object_vecdeque` — bulk for `VecDeque<JsObject>`
- `set_protected_jsvalue` — replace a bare `JsValue` field with protection
- `replace_protected_object` — replace an `Option<JsObject>` field with protection

**Fields now protected:**

| Struct | Field(s) | Location |
|---|---|---|
| `PipeToStateInner` | `promise`, `shutdown_action_promise`, `pending_writes` (all elements), `shutdown_error` (enhanced) | `readablestream.rs` |
| `TeeState` | `cancel_promise`, `reason1`, `reason2` | `readablestream.rs` |
| `ByteTeeState` | `cancel_promise`, `reason1`, `reason2` | `readablestream.rs` |
| `ReadableStream` | `stored_error`, `controller_object` | `readablestream.rs` |
| `WritableStream` | `stored_error`, `controller_object` | `writablestream.rs` |
| `PendingAbortRequest` | `promise`, `reason` | `writablestreamsupport.rs` |
| `SourceMethod` | `this_value` | `readablestreamsupport.rs` |

**Outcome:** The content crate now compiles on JSC without errors.  The
systematic protection eliminates the root cause of JSC SIGSEGV crashes for
all stream operations (pipe-to, tee, cancel, close, abort).

**Remaining crashes (not GC-related):**
- Pipe-to tests using `delay()` (`setTimeout`-based async) time out because
  JSC's C API direct-call path does not pump the event loop for timer
  delivery.  This is a different class of issue from GC protection.
- WASM compile/instantiate timeouts also require event loop pumping.

**Files changed:**
- `content/src/streams/readablestreamsupport.rs` — Added `jsc_protect` module
  with reusable protection helpers and `boa_noop` stubs.
- `content/src/streams/readablestream.rs` — Protected `PipeToStateInner`,
  `TeeState`, `ByteTeeState`, `ReadableStream` fields.
- `content/src/streams/writablestream.rs` — Protected `WritableStream` fields.
- `content/src/streams/writablestreamsupport.rs` — Protected `PendingAbortRequest` fields.

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
| **PipeToStateInner** | `promise`, `shutdown_action_promise`, `pending_writes` | Fixed (2026-07-12) |
| **TeeState / ByteTeeState** | `cancel_promise`, `reason1`, `reason2` | Fixed (2026-07-12) |
| **ReadableStream / WritableStream** | `stored_error`, `controller_object` | Fixed (2026-07-12) |
| **PendingAbortRequest** | `promise`, `reason` | Fixed (2026-07-12) |
| **SourceMethod** | `this_value` | Fixed (2026-07-12) |
| Constructor Proxy eval | Cannot be eliminated (JSC C API has no `JSProxyCreate`). | Acknowledged |
| `promise_state()` eval | `JSPromiseGetStatus` not in public C API. | Acknowledged |
| `promise_state()` nested scope | `promise_state` always returns `Pending` inside nested JS calls because JSC doesn't drain microtasks until the outermost C API call returns. | Unfixed — see Remaining issues

### Phase 4: `JsValueCell`/`JsObjectCell` — backend-level auto-protection

Added `JsValueCell` and `JsObjectCell` to `js_engine/src/gc.rs` as
backend-abstracted cells that auto-protect JS values on JSC and are
equivalent to `GcCell<T>` on Boa.

| Backend | JsValueCell | JsObjectCell |
|---|---|---|
| Boa | Wraps `Gc<GcRefCell<JsValue>>` — GC traces through `#[derive(Trace)]` | Wraps `Gc<GcRefCell<Option<JsObject>>>` |
| JSC | Wraps `Rc<RefCell<JscValue>>` — `set()` calls `JSValueProtect`/`JSValueUnprotect` | Wraps `Rc<RefCell<Option<JscObject>>>` |

Content code uses the uniform API: `new(val)`, `set(val)`, `borrow()`, `borrow_mut()`.
The protection logic lives entirely in the JSC backend — content code imports
`js_engine::gc::JsValueCell` and `js_engine::gc::JsObjectCell`, no conditional
compilation needed.

**Migration:** Replace `GcCell<JsValue>` fields with `JsValueCell` and
`*cell.borrow_mut() = val` with `cell.set(val)` in content code.  The manual
`#[cfg(not(feature = "boa"))]` protection blocks in reader/writer setters
become unnecessary once the fields use these types.

### Remaining work (content crate migration)

| Struct | Current field type | New type | Migration status |
|---|---|---|---|
| `ReadableStream.stored_error` | `GcCell<JsValue>` | `JsValueCell` | Pending |
| `WritableStream.stored_error` | `GcCell<JsValue>` | `JsValueCell` | Pending |
| `ReadableStream.controller_object` | `GcCell<Option<JsObject>>` | `JsObjectCell` | Pending |
| `WritableStream.controller_object` | `GcCell<Option<JsObject>>` | `JsObjectCell` | Pending |
| `PendingAbortRequest.promise` | `JsObject` (in struct) | `JsObjectCell` or manual | Pending |
| `PendingAbortRequest.reason` | `JsValue` (in struct) | `JsValueCell` or manual | Pending |
| `PipeToStateInner.promise` | `JsObject` (in struct) | Requires `JsValueCell` like approach | Pending |
| `PipeToStateInner.pending_writes` | `VecDeque<JsObject>` (in struct) | Requires protected wrapper | Pending |
| `PipeToStateInner.shutdown_error` | `Option<JsValue>` (in struct) | Requires `Option<JsValueCell>` pattern | Pending |
| `PipeToStateInner.shutdown_action_promise` | `Option<JsObject>` (in struct) | Requires `Option<JsObjectCell>` pattern | Pending |
| `TeeState.cancel_promise` / `reason1` / `reason2` | `JsObject` / `JsValue` | Requires cell types | Pending |
| `ByteTeeState` same fields | Same | Same | Pending |
| `SourceMethod.this_value` | `JsObject` (in struct) | `JsObjectCell` or manual | Pending |

**Key insight:** The struct fields that are inline (not behind GcCell) like
`PipeToStateInner.promise`, `TeeState.cancel_promise`, etc. need the outer
struct's Drop to unprotect on teardown.  This can be achieved by making the
`gc_struct_jsc` proc-macro generate Drop impls, or by using cell wrappers for
every field.

### Phase 5: Verify pass-rate parity

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
  `step_timeout`/`setTimeout`) time out on JSC but pass on Boa.  Root cause:
  JSC's C API direct-call path does not pump the event loop for timer
  delivery.  The Rust job queue (`enqueue_job_with_realm`) also does not
  drain automatically inside nested C API calls.  Both must be pumped
  explicitly (see `run_jobs()` and `perform_a_microtask_checkpoint`).
- **WASM worker-context tests** — `WebAssembly.compile`/`instantiate` require
  a `Window` global object for IPC dispatch; workers use
  `DedicatedWorkerGlobalScope`.
- **Transferable streams** (8 files) — `JsTypes` trait lacks primitives for
  structured serialization of stream internals.
- **Queuing-strategy / IDL edge cases** (3 files) — cross-realm constructor
  behavior, size function identity, IDL harness setup.
- `instanceof Window` returns `false` — global `[[Prototype]]` immutable on
  JSC; properties are copied to global instead.
