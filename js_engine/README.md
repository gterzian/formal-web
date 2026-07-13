# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.

## Feature flags

| Flag | Effect |
|---|---|
| `boa` (default) | Boa + Wasmtime JS engine backend |
| `jsc` | JavaScriptCore backend (macOS only) |

## Build commands

**Boa (default, runs WPT):**

```bash
rustup run 1.94.0 cargo build --release
rustup run 1.94.0 cargo run --release -- wpt
```

**Boa + WebAssembly:**

```bash
rustup run 1.94.0 cargo build --release --features wasm
```

**JSC (macOS only):**

```bash
# Build js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine

# Build content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run a single WPT test via JSC
target/release/formal-web wpt dom/nodes/Element-hasAttribute.html
```

## WPT test results

### Boa backend (primary — run full suite)

Latest result (2026-07-12): `executed=79 unexpected=0`

Wasm tests are excluded from the default WPT run (separate `wasm` feature).
Run with `--features wasm` to enable wasm tests.

### JSC backend (experimental)

**PASS:** CSS.supports, DOM Element tests, Node-constants, document.title,
document-dir, iframe, anchor, basic streams (constructor, default-reader,
strategies, transform, writable), formal gc-protection.

**TIMEOUT:** Most piping tests, cancel, read-task-handling (promise_state
microtask drain issue — see below).

**FAIL:** structured-clone (Blob not implemented), wasm compile (timeout).

## Safe builtin function creation

Use `create_builtin_fn_static(behaviour, length, name)` for stateless `fn`
pointers.  Use `create_builtin_fn_with_captures(ec, captures, ...)` for
stateful functions where `captures: C` is `boa_gc::Trace + 'static`.

## Remaining work

### 1. Piping test TIMEOUTs — promise_state microtask drain

`promise_state()` in `js_engine/src/jsc/engine.rs` uses `eval_script_raw("void 0")`
to drain microtasks. JSC only drains its microtask queue when control returns
from the outermost C API call. Inside a nested C API call (common — stream
algorithm code runs inside a JS call), the eval does NOT trigger microtask
drainage. The `.then()` handlers never fire, so the state always reads as
`Pending`.

**Dead ends:** There is no public C API to force JSC microtask drainage.
The `eval_script_raw("void 0")` works at the outermost C API level only.
Tracked promise states failed because the stream algorithm polls CHAINED
promises (via `.then()`), not the original tracked promise.

**Instrumentation:** `ENGINE_NESTING_DEPTH` thread-local in `EngineGuard`
exports `nesting_depth()` — depth == 0 means outermost C API boundary where
drainage might work.

### 2. setTimeout not pumped during piping tests

Piping tests that use `delay()` time out because the timer/task queue is not
serviced while the C API path is blocking.

### 3. `instanceof Window` returns false

The global object's `[[Prototype]]` is immutable through the public C API —
`JSContextGetGlobalObject()` returns a `JSGlobalObject` whose prototype is set
at context creation time. `JSObjectSetPrototype` crashes on macOS 26 for
`JSObjectMake`-created callback objects.

### 4. Other unfixed issues

- **`WindowTimer.arguments`** — `Vec<JsValue>` elements in HTML timer code
  unprotected from GC. Needs `GcRootHandle` wrapping.
- **`detach_array_buffer`** — No-op (`Ok(())`). `is_detached_buffer`
  approximates as `byteLength == 0`.
- **`species_constructor`** — Always returns `default_constructor` (skips
  `Symbol.species` lookup).
- **Cross-realm `new.target`** — `get_function_realm` always returns the
  current realm.
- **WASM compile/instantiate timeout on JSC** — Background compilation
  requires the creating thread's run loop to be pumped.


