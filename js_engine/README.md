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



### Remaining work (2026-07-12)

**Unfixed — GC protection:**
- `WindowTimer.arguments` (`Vec<JsValue>` elements) in HTML timer code.

**Pre-existing pipeTo SIGSEGV (not caused by GC protection changes):**
- Confirmed pre-existing: reverting all changes and testing shows same SIGSEGV.
- Crash in `JSC::JSCallbackObjectData::JSPrivatePropertyMap::visitChildren`
  during parallel GC marking of BUILTIN_CLASS objects (address 0x33).
- Root cause is in the JSC backend's function object creation path
  (`make_builtin_function`, `set_builtin_to_string`, `copy_function_prototype_methods`),
  not in content-crate GC protection.
- Attempted fix: protect BUILTIN_TO_STRING cached function with JSValueProtect
  (insufficient — crash persists). Needs deeper investigation of how private
  property maps interact with parallel GC marking on macOS 26.
- Debugger attach was attempted but content process dies before breakpoint can hit.

**Other unfixed JSC issues:**
- `promise_state()` eval always returns `Pending` inside nested JS calls (JSC
  doesn't drain microtasks until outermost C API call returns).
- WASM compile/instantiate timeout (requires event loop for background compilation).
- Piping tests that use `delay()` time out (setTimeout not pumped by C API path).
- `instanceof Window` returns `false` (global [[Prototype]] immutable on JSC).


