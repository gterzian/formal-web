# JSC backend (`js_engine/src/jsc`)

Experimental, macOS only.  JavaScriptCore is used through its C API plus a
small Objective-C shim (`src/jsc_gc_wrapper.m`) that exposes `JSManagedValue`.

## Build

```bash
# Build js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine

# Build content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run a single WPT test via JSC
target/release/formal-web wpt dom/nodes/Element-hasAttribute.html
```

The `jsc` feature compiles the Objective-C shim with ARC and links
JavaScriptCore and Foundation, so it only builds for Apple targets.

## GC integration

JS values held by Rust (realm roots, platform-object data, reflectors) are
wrapped in a `JSManagedValue` (`src/jsc_gc_wrapper.{h,m}`, wrapped for Rust by
`src/jsc/gc.rs`).  This is the public API for holding a JS value outside the
JS heap and replaces the hand-balanced C API protect set:

- `JscGcOwner` is a per-realm owner object bridged into the JS runtime as a
  property of the global object.  `create_root`/`protect_value` register a
  managed value against it, so a root stays alive while its `GcRootHandle`
  lives and is released when the handle drops or the engine drops.
- `JscManagedValue::new_weak` is a weak reference whose `get` returns `None`
  once the JS value is collected.  `JsTypesGcExt::Reflector` uses it so a
  reflector reports a collected wrapper instead of a dangling pointer.
- `create_object_with_any` roots platform wrappers through a managed value
  held for the realm's lifetime.

`JSValueProtect`/`JSValueUnprotect` remain only for the cached builtin-function
properties stored as JSClass private data (balanced protect/unprotect).

### Cross-language cycles

A platform object's JS-value fields are protected by per-value managed
references.  On JSC every `GcCell<T>` (see `js_engine/src/gc.rs`) holds an
`Rc<RefCell<T>>` slot plus a `Vec<JscManagedValue>` of edges for the JS values
directly inside `T`; `#[gc_struct]` generates a `GcTraceable` walk of each
platform object's fields, and `create_interface_instance` adopts the instance's
cells onto the platform object's holder (`JscGcOwnerRef::platform`).  The
holder is the exported ObjC object behind the platform object's JS wrapper, so
the edges are scanned exactly while the wrapper is reachable.  `set`
re-registers the edges; in-place mutation through `borrow_mut` needs an
explicit `sync`.  Cells created outside the interface-creation path (for
example `DefaultAsyncIterator`) keep the realm anchor as owner.

The realm-lifetime managed root in `create_object_with_any`
(`protected_objects`) still pins every platform wrapper, so the `wrapper →
platform data → … → wrapper` cycle is never collected: the edges keep the JS
side of the cycle alive, but the wrapper itself is rooted from Rust for the
engine's lifetime.  Removing that root needs a Rust→wrapper reachability story
(a Rust-held platform object must keep its wrapper alive, or `upgrade_reflector`
must recreate it), which the engine does not have.  V8 collects these cycles
because platform objects and their edges live in the cppgc unified heap, and
Boa because `boa_gc` traces `#[gc_struct]` fields.

The per-object owner is the bridge holder that `valueWithObject:` already
creates; no `JSExport` conformance is required for an owner to be scanned, and
no property is set from within a binding callback.  Do not make the owner a
separate `NSObject` exported with `JSValue setValue:forProperty:` on the
reflector: `origin/jsc_objc_2` did so and saw SIGSEGVs in the heavy streams
tests, a crash class that matching a plain exported holder avoids.

## WPT results

**PASS:** CSS.supports, DOM Element tests (including
`dom/nodes/Element-hasAttribute.html`), Node-constants, document.title,
document-dir, iframe, anchor, basic streams (constructor, default-reader,
strategies, transform, writable), `formal/gc-protection.html`,
`formal/callback-gc-protection.html`.

**TIMEOUT:**  Most piping tests, cancel, read-task-handling.

**FAIL:** structured-clone (Blob not implemented), wasm compile (timeout),
`formal/window-open-basic.html` and `formal/window-proxy-lifecycle.html`
(the opened WindowProxy's `document` is not reachable in the popup realm,
and a cross-origin `open` throws a `TypeError` instead of a `DOMException`).

## Binary size

`formal-web-content` built with `--no-default-features --features v8`
(no media) is 72.80 MiB; the same build with `--features jsc` is 18.27 MiB —
a 54.5 MiB difference (V8 binary ≈ 3.98× JSC).  Media is not a factor: the
default build (`media` + `v8`) is 72.82 MiB, so media adds ~19 KiB to the
content binary (the media backend lives in the `formal-web-graphics` helper).

The difference is dominated by linking, not by engine code: the V8 build
statically links V8 (`librusty_v8.a` is ~140 MiB of objects; the binary's
`__TEXT` segment is 53.4 MiB), while the JSC build dynamically links the
system `JavaScriptCore.framework`, whose code lives in the macOS dyld shared
cache and is not counted in the app binary (JSC's `__TEXT` is 14.1 MiB).  The
18.27 MiB JSC number is therefore "everything except the engine" plus a
dynamically-linked system framework, not a like-for-like engine-size
comparison.

## Remaining work

- **`window.open` windows live in a separate JSC context.**  JSC's
  `build_realm_inner` builds a fresh `JscContext` for each child realm, so
  the `WindowProxy`'s backing Window object is in a different JS heap than
  the proxy traps.  `trap_get` reads that backing object with the parent
  realm's execution context — a cross-context access — so `open`, `close`,
  and `document` come back `undefined` and `w.open(...)` throws a `TypeError`
  instead of the expected `SecurityError` DOMException.  Fixing it needs
  either JSC multi-realm support (one shared `JSGlobalContextRef` with
  per-realm globals) or forwarding the WindowProxy's members through the
  content process instead of reading the backing JS object.
  **Dead end:** `JscEngine::new_shared_realm()` + `setup_realm` shares the
  context but breaks script evaluation — a child realm's `realm_global` is a
  plain object, while `JSEvaluateScript` still resolves global variables
  (`window`, …) against the context's global object, so child-realm scripts
  lose `window`.  It also breaks the main run, since test pages are loaded in
  child realms.
- **`JSEvaluateScript("void 0")` drains SIGSEGV.** `JscEngine::call` and
  `construct` end with `eval_script_raw("void 0")` to drain microtasks; a
  `formal/gc-protection.html` run intermittently SIGSEGVs inside
  `JSC::ProgramExecutable::initializeGlobalProperties` under that eval,
  reached from a queued job (`run_jobs → perform_a_microtask_checkpoint`).  The
  crash reproduces with the managed edges disabled, so it is the eval-drain
  path, not edge registration.  Reproduce with
  `FORMAL_WEB_CRASH_TRACE=1 target/release/formal-web wpt tests/formal/tests/gc-protection.html`
  after re-adding a signal handler that calls `backtrace_symbols_fd` (the
  `std::backtrace` capture cannot unwind the signal frame).  Not yet
  investigated: whether the eval is redundant at the outermost call boundary
  (where JSC already drains on C-API return) and can simply be dropped, and
  whether the nested-call drains can be replaced by a drain keyed to the
  outermost return.
- **JSC microtask drain during nested C API calls.**  JSC only drains its
  microtask queue when control returns from the outermost C API call, so
  inside nested calls (common — stream algorithm code runs inside a JS call)
  `.then()` handlers do not run.  `promise_state()` works around this with an
  eval-based fallback plus settlement records.
  **Dead end:** No public C API forces JSC microtask drainage; a
  `CFRunLoopRunInMode` pump does not drain the queue either.  Tracked promise
  states fail because stream algorithms poll CHAINED promises (via `.then()`),
  not the original tracked promise.
- **`instanceof Window` returns false** — the global object's [[Prototype]]
  is immutable through the public C API.  `set_prototype` reports the failed
  assignment so the realm bootstrap copies the `Window`/`EventTarget`
  prototype members onto the global object (`addEventListener` and the rest
  of the window methods work), but the prototype chain itself cannot be
  established, so `instanceof` remains false.
- **`WindowTimer.arguments`** — `Vec<JsValue>` elements are not rooted.
  Each element needs a `GcRootHandle` (backed by `JscManagedValue`).
- **`detach_array_buffer`** — No-op (`Ok(())`).
- **`species_constructor`** — Always returns `default_constructor`.
- **Cross-realm `new.target`** — `get_function_realm` always returns the
  current realm.
- **WASM compile/instantiate timeout** — JSC's native WebAssembly:
  background compilation requires the creating thread's run loop to be
  pumped.
