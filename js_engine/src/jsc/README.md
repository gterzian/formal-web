# JSC backend (`js_engine/src/jsc`)

Experimental, macOS only.

## Build

```bash
# Build js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine

# Build content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run a single WPT test via JSC
target/release/formal-web wpt dom/nodes/Element-hasAttribute.html
```

## WPT results

**PASS:** CSS.supports, DOM Element tests, Node-constants, document.title,
document-dir, iframe, anchor, basic streams (constructor, default-reader,
strategies, transform, writable), formal gc-protection.

**TIMEOUT:**  Most piping tests, cancel, read-task-handling.

**FAIL:** structured-clone (Blob not implemented), wasm compile (timeout).

## Remaining work

- **JSC microtask drain during nested C API calls.** `promise_state()` uses
  `eval_script_raw("void 0")` to drain microtasks, but JSC only drains its
  microtask queue when control returns from the outermost C API call.
  Inside nested calls (common — stream algorithm code runs inside a JS
  call), the eval does not trigger drainage and `.then()` handlers never
  fire.
  **Dead end:** No public C API forces JSC microtask drainage. Tracked
  promise states fail because stream algorithms poll CHAINED promises (via
  `.then()`), not the original tracked promise.
- **`instanceof Window` returns false** — Global object's `[[Prototype]]`
  is immutable through the public C API.
- **`WindowTimer.arguments`** — `Vec<JsValue>` elements unprotected from GC.
  Needs `GcRootHandle` wrapping.
- **`detach_array_buffer`** — No-op (`Ok(())`).
- **`create_platform_object_proxy`** — The (proxy, target) pairs are kept in
  `host_data` as unprotected `JscObject`s and compared by raw pointer, so a
  collected proxy's address can alias a later object.
- **`species_constructor`** — Always returns `default_constructor`.
- **Cross-realm `new.target`** — `get_function_realm` always returns the
  current realm.
- **WASM compile/instantiate timeout** — JSC's native WebAssembly:
  background compilation requires the creating thread's run loop to be
  pumped.
