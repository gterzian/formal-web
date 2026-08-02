# `jsc` — JavaScriptCore backend

JavaScriptCore backend for `js_engine` (macOS only, feature `jsc`).  Modules:
`types.rs` (value wrappers), `engine.rs` (the `JsEngine`/`ExecutionContext`/
`EcmascriptHost` implementations), `objc_gc.rs` (ObjC managed-reference GC
integration).

## Build & test

```bash
# Build the js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine

# Build the content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run a single WPT test via JSC
rustup run 1.94.0 cargo run --release --no-default-features --features jsc -- wpt dom/nodes/Element-hasAttribute.html

# Generic engine tests
rustup run 1.94.0 cargo test --no-default-features --features jsc -p content generic_js_test
```

## Status / WPT results

**PASS:** CSS.supports, DOM Element tests, Node-constants, document.title,
document-dir, iframe, anchor, basic streams (constructor, default-reader,
strategies, transform, writable), formal gc-protection,
callback-gc-protection, streams/writable-streams/constructor.

**TIMEOUT:** Most piping tests, cancel, read-task-handling.

**FAIL:** structured-clone (Blob not implemented), wasm compile (timeout),
formal/byob-debug (WIP internal test).

Generic engine tests: 95/96 pass; only
`generic_js_test::tests::constructor_has_function_prototype_methods_on_jsc`
fails (`TestWidget.toString()` returns `[object FormalWebBuiltin]`).

## GC integration (managed references)

Values are protected from collection through the ObjC API's
managed-reference mechanism (`JSManagedValue` +
`addManagedReference:withOwner:` / `removeManagedReference:withOwner:`),
following WebKit's `testObjectiveCAPI.mm` patterns.  `JSValueProtect`/
`JSValueUnprotect` are gone.  See `objc_gc.rs` for the bindings and the
`JscManagedValue` RAII type.

Design facts (established empirically, not assumed):

- The owner of a managed reference must be an Objective-C object
  *exported to JS* (`setObject:forKeyedSubscript:` — a
  `JSAPIWrapperObject`) for the GC to scan the edge.  Each context exports
  one `NSObject` anchor as the `formalWebGcAnchor` global; every managed
  value reports it as owner.  The anchor is attached to the cached
  `JSContext` wrapper via an associated object, so it is found from a bare
  `JSContextRef`.
  *Dead end:* the `JSContext` wrapper itself as owner does not protect
  under the synchronous collector — only exported objects count as
  "reachable from the JavaScript runtime".
- `gc()` uses `JSSynchronousGarbageCollectForDebugging` **plus** a brief
  `CFRunLoopRunInMode` pump.  Plain `JSGarbageCollect` defers
  sweeping/finalization to the run loop and reclaims nothing on its own,
  and `removeManagedReference` only takes effect under the synchronous
  collector.  (The synchronous collector is what WebKit's own ObjC test
  suite uses.)

Generic GC tests live in `content/src/generic_js_test.rs`
(`gc_reclaims_unreferenced_objects`, `root_keeps_value_alive_until_dropped`,
`js_value_cell_keeps_value_alive_then_releases`,
`js_object_cell_keeps_value_alive_then_releases`) and run on every backend.

## Open issues

- **Microtask drain during nested C API calls** — `promise_state()` uses
  `eval_script_raw("void 0")` to drain microtasks, but JSC only drains its
  queue when control returns from the outermost C API call; inside nested
  calls `.then()` handlers never fire.
  *Failed attempt:* no public C API forces JSC microtask drainage.
- **`setTimeout` not pumped during piping tests** — `delay()` timeouts.
- **`instanceof Window` returns false** — the global object's
  `[[Prototype]]` is immutable through the public C API.
- **`WindowTimer.arguments`** — `Vec<JsValue>` elements unprotected from
  GC; needs `GcRootHandle` wrapping.
- **`detach_array_buffer`** — no-op (`Ok(())`).
- **`species_constructor`** — always returns `default_constructor`.
- **Cross-realm `new.target`** — `get_function_realm` always returns the
  current realm.
- **WASM compile/instantiate timeout** — background compilation requires
  the creating thread's run loop to be pumped.
- **Collection-after-release not observable from Rust tests** — JSC
  conservatively scans the stack: any raw `JSValueRef` in a Rust stack
  slot keeps the value alive even after `removeManagedReference`.  The
  release contract is verified by ObjC experiments, not by the generic
  tests (their release assertions were removed for this reason).
  *Failed attempt:* clobbering the test locals (reassigning the `JscValue`
  slots to null before `gc()`) does not reliably release — the pointer
  survives in registers/spill slots.
- **`formalWebGcAnchor` global property is visible to JS** — it is an
  enumerable global; `Object.keys(globalThis)` lists it.
  *Failed attempt:* redefining it as non-enumerable via the C API
  (`JSObjectSetProperty` with `kJSPropertyAttributeDontEnum`) crashes JSC
  when called from within a C callback (during interface registration).
- **Values linger after unrooting under `JSGarbageCollect`** —
  `removeManagedReference` only takes effect with the synchronous
  collector (this is why `gc()` uses it).  With the sync collector +
  pump, released values are reclaimed when no stale stack pointers remain.
  *Failed attempt:* the old `JSValueProtect`/`JSValueUnprotect` also left
  values uncollected after unprotect in this JSC version — no release
  difference between the two mechanisms.
