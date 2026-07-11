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

## JSC backend current state (2026-07-12)

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

To run WPT on JSC, build the content binary with JSC and leave the
embedder with default (Boa) features:

```bash
# Build js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine
rustup run 1.94.0 cargo test --no-default-features --features jsc -p js_engine

# Build content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run WPT tests (embedder uses default Boa features, spawns JSC content)
target/release/formal-web wpt dom/nodes/Element-hasAttribute.html
```

### WPT test inventory (JSC, 2026-07-12)

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

**TIMEOUT (promise_state microtask drain issue — see below):**
- streams/piping: abort, close-propagation-backward, close-propagation-forward,
  error-propagation-backward, error-propagation-forward, flow-control (HTTP error),
  general, multiple-propagation
- streams/readable-streams: cancel, read-task-handling
- streams/writable-streams: bad-strategies, bad-underlying-sinks,
  byte-length-queuing-strategy

**FAIL (no crash, spec-compliance issues):**
- Wasm compile (timeout)
- structured-clone (Blob not implemented)
- streams/idlharness: `Can't find variable: fetch`

**Pre-existing expected failures (metadata):**
- Various streams tests with TODO metadata (BYOB, cross-realm, transferable)

### Unit tests

```bash
# JSC: js_engine-level unit tests only (18 pass)
rustup run 1.94.0 cargo test --no-default-features --features jsc -p js_engine
```

### WPT

```bash
# Boa (default — full WPT suite)
rustup run 1.94.0 cargo run --release -- wpt

# JSC — build content with JSC, then run:
RUST_LOG=error target/release/formal-web wpt <test-path>
```

Latest Boa WPT result (2026-07-11): `executed=84 unexpected=0`

## Remaining work

### 1. Piping test TIMEOUTs — promise_state microtask drain

**Symptom:** Most piping tests (abort, close-propagation, error-propagation,
general, multiple-propagation) time out instead of crashing.

**Why it's not the old SIGSEGV:** The private-property-map crash
(`JSCallbackObjectData::JSPrivatePropertyMap::visitChildren` during parallel
GC marking) is fixed. These tests now PROGRESS past the crash point and reach
a different bottleneck.

**Root cause:** `promise_state()` in `js_engine/src/jsc/engine.rs` uses
`JSEvaluateScript` with `.then()` handlers and `eval_script_raw("void 0")` to
drain microtasks. JSC only drains its microtask queue when control returns
from the outermost C API call. Inside a nested C API call (which is the common
case — `promise_state` is called from within stream algorithm code that's
itself running inside a JS call), the `void 0` eval does NOT trigger microtask
drainage. The `.then()` handlers never fire, so the state always reads as
`Pending`.

**Dead-end investigations:**
- There is no public C API to force JSC microtask drainage.
- The `eval_script_raw("void 0")` approach works at the outermost C API level
  but not from nested calls.
- Creating a separate "drain" loop with `JSEvaluateScript` in a while-loop
  would either hang or not help (the issue is that reactions aren't queued,
  not that they're queued but unprocessed).

**What's needed:** A way to observe promise state without relying on `.then()`
microtasks. The `new_promise_capability()` has already been refactored to use
a native `StoredBehaviour` executor (stores resolve/reject in a Rust-side
`Rc<RefCell<>>`). A similar approach for `promise_state()` — replacing the
`.then()`-and-poll pattern with direct state observation — would fix this.

One approach that was discussed but not implemented: wrap the resolve/reject
functions returned by `new_promise_capability` with native callbacks that
record the promise state in a Rust-side cell, so `promise_state()` can read it
without needing microtasks at all.

### 2. WASM compile/instantiate timeout

JSC's WebAssembly compilation dispatches background compilation work that
requires the creating thread's run loop to be pumped. The content process's
event loop doesn't pump `CFRunLoop`/GCD between `run_jobs()` calls, so
background compilation completes but its promise never resolves.

### 3. setTimeout not pumped during piping tests

Piping tests that use `delay()` time out because the timer/task queue is not
serviced while the C API path is blocking. This is a content-crate event-loop
ordering question, not a `js_engine::jsc` issue.

### 4. `instanceof Window` returns false

The global object's `[[Prototype]]` is immutable through the public C API —
`JSContextGetGlobalObject()` returns a `JSGlobalObject` whose prototype is set
at context creation time. `JSObjectSetPrototype` crashes on macOS 26 for
`JSObjectMake`-created callback objects, so the workaround of creating a
separate global-proxy object doesn't help.

A `hasInstance` callback on the `Window` constructor (using the existing
`builtin_has_instance` mechanism) was suggested as a lower-risk fix but not
implemented.

### 5. Other unfixed issues

- **`WindowTimer.arguments`** — `Vec<JsValue>` elements in HTML timer code are
  unprotected from GC. Needs `GcRootHandle` wrapping.
- **`detach_array_buffer`** — No-op (`Ok(())`). `is_detached_buffer`
  approximates detachment as `byteLength == 0`, which misclassifies legitimately
  empty buffers.
- **`species_constructor`** — Always returns `default_constructor`
  unconditionally (skips `Symbol.species` lookup). Fine as placeholder but any
  subclassed Array/Promise/TypedArray methods relying on species will silently
  misbehave.
- **Cross-realm `new.target`** — `get_function_realm` always returns the
  current realm. Add a `debug_assert!` at call sites that rely on this.
- **`Object.getOwnPropertyDescriptor` on builtin functions** — The
  `getProperty`/`setProperty` callback approach for `.name`/`.length` may
  cause `Object.getOwnPropertyDescriptor` to return a non-standard descriptor.
  Not currently tested by the WPT subset, but would fail strict WPT validation
  if checked.

### Failed fix attempts

1. **`BUILTIN_STATIC_FUNCTIONS` + `staticValues` for name/length** — Using
   `JSClassDefinition.staticValues` for `.name`/`.length` produces accessor
   property descriptors instead of data property descriptors (spec violation).
   Using `staticFunctions` for `toString`/`bind`/`call`/`apply` on the class
   prototype doesn't resolve correctly when a `getProperty` callback is also
   present — accessed values return `undefined` even though `in` reports them
   as present.

2. **Removing function prototype method copies entirely** — Omitting
   `toString`/`bind`/`call`/`apply` from BUILTIN_CLASS objects broke stream
   controller code that uses `.bind()` on builtin methods like `c.enqueue`.
   These properties must be present as own properties.

## JSC Architecture & Debugging Notes (learned 2026-07-12)

### Private property map and parallel GC

`JSCallbackObjectData::JSPrivatePropertyMap` is JSC's per-object lazily-allocated
HashMap that stores properties set via `JSObjectSetProperty` when the class has
no `setProperty` callback. The map is traced during GC marking via
`visitChildren`. During parallel GC marking, helper threads trace this map
concurrently. The crash at address `0x33` (offset from null) indicates a
dangling or corrupted `JSPrivatePropertyMap*` being accessed during concurrent
tracing.

**Key insight:** The content process is single-threaded (mutator runs on one
thread), but JSC's GC helper threads run in PARALLEL with marking work. This
means single-threaded-mutator guarantees do NOT protect against concurrent-GC
races on JSC-managed data structures.

**Fix mechanism:** Providing a non-null `setProperty` callback on the
`JSClassDefinition` prevents JSC from ever creating the private property map.
When `setProperty` is non-null, JSC calls it for every `JSObjectSetProperty`
call instead of falling through to the map. The callback must accept and store
the values somewhere (we use the Rust-side `BuiltinFunctionData` struct with
`JSValueProtect`/`JSValueUnprotect` for GC safety).

### `getProperty`/`setProperty` vs `staticValues`

| Mechanism | JSC descriptor type | Use case |
|---|---|---|
| `staticValues` | Accessor property (`get`/`set`) | NOT for `.name`/`.length` — fails `Object.getOwnPropertyDescriptor` checks |
| `staticFunctions` | Function value on class prototype | `toString`/`bind`/`call`/`apply` — but does NOT resolve correctly when `getProperty` callback is also present |
| `getProperty`/`setProperty` callbacks | Data property (when returning values) | `.name`, `.length` — bypasses private property map; `Object.getOwnPropertyDescriptor` returns `{value, writable, enumerable, configurable}` |

**Critical finding:** `staticFunctions` entries on the automatic prototype do
not resolve correctly when a `getProperty` callback is present on the same
class. `'bind' in fn` returns `true` but `fn.bind` returns `undefined`. This
means staticFunctions and getProperty callbacks are incompatible — use one or
the other, not both.

### Testing with CDP (browser extension)

To test JSC changes without running the full WPT suite:

```bash
# Build content with JSC, embedder with default (Boa) features
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content
target/release/formal-web cdp --port 9222 --startup-url "about:blank" &
```

Then use the browser extension tools (`browser_evaluate`, `browser_navigate`,
etc.) to run JS in the JSC content process. Notable capabilities:
- `TestUtils.gc()` triggers synchronous GC (useful for testing GC-related crashes)
- Stream creation, piping, .bind() on builtin methods all work
- CDP `Runtime.evaluate` executes JS synchronously — promises can't resolve
  in this context (they need the event loop)

### Useful C API behavior notes

- **`JSObjectSetPrototype` crashes** on macOS 26 for `JSObjectMake`-created
  objects with callbacks (`callAsFunction`/`callAsConstructor`). This prevents
  setting prototype inheritance for BUILTIN_CLASS objects.
- **`JSValueProtect`/`JSValueUnprotect`** work on any GC-managed heap value:
  objects (`kJSTypeObject`), symbols (`kJSTypeSymbol`), and bigints
  (`kJSTypeBigInt`). Primitives (undefined, null, boolean, number, string) are
  stack-allocated and need no protection.
- **`JSGlobalContextCreate(JSClassRef)`** accepts a class for the global object.
  If a non-null class is provided, the global object's class is that class;
  otherwise JSC uses a default class. The global object's prototype is set
  during context creation and cannot be changed afterwards through the C API.
- **`JSEvaluateScript` does not drain microtasks** from nested C API calls.
  JSC only drains the microtask queue when control returns from the outermost
  JS->C boundary. Inside a nested JS call, `eval("void 0")` executes but does
  NOT trigger microtask processing.
- **`JSObjectMakeFunctionWithCallback`** has no user-data parameter (the
  callback receives no context pointer), which is why we use the
  `JSObjectSetPrivate` + `JSClassDefinition.callAsFunction` pattern instead.

### `jsc_sys.rs` FFI notes

1. **`JSPropertyNameAccumulatorRef`** — The `getPropertyNames` callback
   parameter was typed as `*mut JSObjectRef`; it should be
   `*mut JSPropertyNameAccumulatorRef` (a separate opaque type, not an object).
2. **`Default` for `JSClassDefinition`** — Added via `std::mem::zeroed()` to
   simplify construction.
3. **`Send`/`Sync` for `JSStaticValue`/`JSStaticFunction`** — Required for use
   in `LazyLock` statics.
