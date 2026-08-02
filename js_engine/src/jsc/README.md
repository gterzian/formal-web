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
callback-gc-protection, streams/writable-streams/constructor, all
`streams/piping/*` except `abort` (disabled in meta), and
`streams/readable-streams/cancel` + `read-task-handling`.

**FAIL:** structured-clone (Blob not implemented), wasm compile (timeout),
formal/byob-debug (WIP internal test).

**Unexpected in default suite (unfixed, pre-existing):**
`streams/readable-byte-streams/patched-global.any.js` and
`streams/readable-byte-streams/respond-after-enqueue.any.js` (BYOB
buffer-fill bug: read-into buffers come back zero-filled).  Both fail on
HEAD too — not caused by the piping fixes.

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

## Piping test status

All `streams/piping/*` tests pass except `abort.any.js` (disabled in meta
because readable byte stream tee abort semantics are unimplemented).  The
pipe state machine and the JSC promise-state plumbing were reworked to
make them pass:

- **Write registration vs shutdown race** — `write_chunk` runs the sink's
  write algorithm through `writer.write`, which can synchronously trigger
  nested promise reactions (JSC drains microtasks when a JS call returns).
  A nested reaction could run the error/close propagation and finalize
  before the write promise was pushed to `pending_writes`.  Fixed with a
  `write_in_progress` flag: shutdown waits for the write to be registered
  when the destination is still writable.  (`content/src/streams/readablestream.rs`)
- **Action promise registration vs shutdown race** — `perform_action` sets
  the pipe state to `ShuttingDownPendingAction` before the action promise
  is created, so a nested reaction could finalize with no action promise
  recorded (pipeTo fulfilled instead of rejecting with the close error).
  Fixed by waiting in that branch until the action promise's own reaction
  drives the shutdown.
- **Wrong error on rejected action** — `pipeTo` rejected with the original
  shutdown error instead of the cancel/abort action's rejection reason.
  The `ShuttingDownPendingAction` branch polls the action promise via
  `promise_state`; see the resolver-recording and stale-record fixes below.

## Promise settlement recording

`perform_promise_then` wraps every reaction in a builtin that records the
promise's settlement (`promise object pointer → (fulfilled, value)`) in
`JscEngine::settled_promises` before delegating to the original handler
(the original handler is rooted through a `JscManagedValue` owned by the
wrapper — it is invisible to JSC's GC otherwise).  `promise_state` consults
the record first, falling back to the eval-based drain.  This lets the
streams pipe state machine observe settled promises from inside reaction
callbacks, where JSC's microtask drain is a re-entrancy no-op.

Two additions make the records reliable:

- **Resolver recording** — `new_promise_capability` registers the
  resolve/reject functions in `JscEngine::promise_resolvers` (resolver
  pointer → (promise pointer, is_reject)).  `call` consults it, so invoking
  a resolver records the promise's settlement synchronously.  This makes
  `promise_state` correct for promises settled through the engine's own
  resolvers (writer `ready`, closed promises, write/close/abort request
  promises) even inside nested calls, where the eval fallback returns
  `Pending`.
- **Stale-record cleanup** — records are keyed by promise pointer; when JSC
  collects a promise and recycles its address, a newer promise at that
  address would hit the old record.  `perform_promise_then` removes any
  record at the promise's address before attaching a reaction (the reaction
  re-records the true settlement when it fires), and `new_promise_capability`
  does the same for freshly created promises.  Without this, the
  "rejected cancel promise" piping test intermittently rejected with the
  wrong error.  Records are also cleared in `gc()`.

## Open issues

- **Piping test infra flakiness** — `close-propagation-backward` and
  others intermittently report `ERROR`/`CRASH` ("WebDriver child did not
  become ready", "Resource temporarily unavailable", content-process
  `SIGSEGV` in `run_window_timer`/interface registration) on repeat runs;
  the same test passes in other runs.  The `SIGSEGV`s are dangling-pointer
  crashes from the missing timer/interface GC rooting below, exposed by
  timing shifts.
- **`uncaught callback error: undefined` during gc-protection** — the
  first `setTimeout(0)` callback reports this error.  The timer's
  `Callback`/`arguments` live in `window_timers` with no GC rooting on
  JSC, so the now-working `gc()` collects the callback function before
  the timer fires (dangling pointer).  Related to the `WindowTimer`
  issue below; the test still passes (the error is logged, not thrown).
- **Microtask drain during nested C API calls** — `promise_state()` uses
  `eval_script_raw("void 0")` to drain microtasks, but JSC only drains its
  queue when control returns from the outermost C API call; inside nested
  calls `.then()` handlers never fire.
  *Failed attempt:* no public C API forces JSC microtask drainage; a
  `CFRunLoopRunInMode` pump inside a reaction does not drain the remaining
  queue either.  (Mitigated by the settlement recording above, but the
  eval-based fallback still returns `Pending` for untracked promises
  inside reactions.)
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
