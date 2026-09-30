# JavaScriptCore backend — removed

The JavaScriptCore backend (`feature = "jsc"`) was removed.  This directory
holds only this note: why it was removed, what was tried, and what was learned,
so the investigation is not repeated.

## Verdict

The system JavaScriptCore, driven through its public C API (plus a small
Objective-C shim exposing `JSManagedValue`), is not viable as the engine of a
web engine.  Two properties of the public API cannot be reconciled with the
HTML/ECMA-262 execution model:

1. **Microtasks.**  JSC drains its microtask queue only when control returns
   from the outermost C API call.  Web-platform algorithms routinely run inside
   a JS call (streams, promise reactions), where `.then()` handlers therefore
   do not run.  No public C API forces a drain.
2. **GC.**  Values held only by Rust are invisible to JSC's GC.  The public
   managed-reference mechanism (`JSManagedValue` + `addManagedReference:`) can
   root them, but the registrations corrupt the heap during `JSGarbageCollect`
   (SIGSEGV in a later `JSEvaluateScript`), and a correct Rust↔JS cycle
   collector is out of reach.

The backend reached a useful WPT subset (DOM, basic streams) and, on good runs,
the whole streams/piping suite, but could never make the piping suite and
`gc-protection` reliable.  The two research branches that carried the JSC work
were never merged; V8 (default) and Boa are the supported engines.

## What was built

- `JsTypes`/`JsEngine`/`ExecutionContext` implementations over the JSC C API
  (`jsc_sys.rs` raw bindings, `types.rs` value wrappers, `engine.rs`).
- A small Objective-C shim exposing `JSManagedValue` to Rust.
- A one-context-per-realm engine with per-value GC protection integrated into
  the generic `GcCell`/`GcRootHandle`.

## Public-API walls

- **Immutable global `[[Prototype]]`.**  Assigning it through `set_prototype`
  fails.  The workaround copied `Window`/`EventTarget` members onto the global
  object, so `addEventListener` and the rest of the window methods worked, but
  `instanceof Window` stayed false and the prototype chain could not be
  established.
- **No forced microtask drain.**  `CFRunLoopRunInMode` does not drain the queue
  either.  Drainage is tied to returning from the outermost C API call.
- **No `queueMicrotask`** in the system JSC (`typeof === "undefined"`).
- **One `JSGlobalContextRef` per realm; shared contexts break evaluation.**
  `JSEvaluateScript` resolves global variables against the context's global
  object, not a child realm's global, so a shared context makes child-realm
  scripts lose `window`.  Consequently `window.open` windows live in a separate
  JSC heap: `WindowProxy` could not read the backing window (`document`,
  `open`, `close` came back `undefined`, and a cross-origin `open` threw
  `TypeError` instead of `SecurityError`).
- **Symbol property keys** silently no-op through the string-based property API;
  `JSObjectGetPropertyForKey`/`SetPropertyForKey`/`HasPropertyForKey`/
  `DeletePropertyForKey` (macOS 10.15+) are required for `@@asyncIterator` and
  the like.
- **Setting a property from inside a C callback crashes JSC**
  (`JSObjectSetProperty` / `setValue:forProperty:` during interface
  registration or binding callbacks).  This is a durable crash class, not a bug
  in the caller.
- **`JSObjectSetPrototype` crashes on objects that have a `callAsConstructor`
  callback** (macOS 26), so builtin-constructor prototypes had to copy
  `Function.prototype` members instead of setting the prototype.
- **`to_object` must go through `JSValueToObject`**; handing a raw primitive
  cell to a property API crashes.
- **Miscellaneous stubs:** `detach_array_buffer` was a no-op,
  `species_constructor` always returned the default constructor, and
  cross-realm `new.target` (`get_function_realm`) always returned the current
  realm.
- **Wasm background compilation** needs the creating thread's run loop pumped,
  so compile/instantiate timed out.  (V8 implements WebAssembly natively, so
  this is moot now.)

## Microtasks: what was tried

- Rust-side "queue a microtask" jobs were moved into **JSC's own microtask
  queue**: the job closure was stored as private data on a `JOB_CLASS` function
  object and queued with `Promise.resolve(undefined).then(jobFn)` using the
  captured `%Promise.prototype.then%`.  JSC then ran them FIFO with all other
  microtasks, which fixed the tee "only pull enough to fill the emptiest queue"
  ordering failure.
- Depth-0 enqueues (no JS call active) were deferred into `pending_jobs`,
  because calling into JS to queue a microtask at depth 0 is itself an outermost
  C API call and JSC drains on its return — which would run the job
  synchronously.  `run_jobs`/`perform_a_microtask_checkpoint` flushed the queue
  and forced a drain with `eval_script_raw("void 0")`.
- Because `promise_state` could not observe chained promises inside nested
  calls, `perform_promise_then` and `new_promise_capability` recorded
  settlements and resolvers (see "Promise settlement recording").
- The eval-based `promise_state` fallback ran arbitrary JS at nested depth and
  could re-enter itself through a reaction; this caused unbounded recursion
  (stack overflow) until an `in_promise_state_eval` re-entrancy guard was added.
- **Never solved.**  The pipe state machine itself completed correctly, but the
  JS-level promise/timer machinery stalled: the test harness's chained
  `setTimeout(0)` + `.then` `flushAsyncEvents` stopped being processed while the
  testharness timeout timer still fired.  Engine-created function objects
  (the `JOB_CLASS` job functions and the settlement-reaction wrappers) were also
  collected while still referenced, producing `TypeError: f is not a function`.
- The upstream `main` branch (plain Rust job queue, before the microtask and
  managed-reference redesigns) did not reproduce the piping flakiness, so those
  regressions were local to the JSC changes.  Verdict: the principled fix —
  hold the JSLock across the whole content-command handling so microtasks drain
  only at the explicit checkpoint — is not implementable through the public C
  API.

## GC integration: what was tried

`GcCell<T>` on JSC was `Rc<RefCell<T>>`, invisible to JSC's GC; a JS value in a
cell was therefore not kept alive.

1. **Per-value protect wrappers.**  `JsObjectCell`/`JsValueCell` wrapped a value
   in a `JscManagedValue` and held the managed reference **inside the cell**,
   against a per-context anchor.  This was stable but rooted every cell value
   unconditionally for the cell's lifetime, and forced content to name a
   backend-aware cell type.
2. **Unified `GcCell` with managed edges.**  A unified cell kept `Rc<RefCell<T>>`
   storage plus `JSManagedValue` edges for the JS values inside `T`, with
   `#[gc_struct]` generating a `GcTraceable` walk and `create_interface_instance`
   adopting the instance's cells onto a per-object owner.
3. **Per-object owners.**  Each platform object got an `NSObject` owner exported
   on its reflector (`setValue:forProperty:`), so its edges were scanned exactly
   while the wrapper was reachable — the intended Boa-like trace semantics.

Established facts (empirical, not assumed):

- The owner of a managed reference must be an Objective-C object **exported to
  JS** (a `JSAPIWrapperObject`); the `JSContext` wrapper as owner does not
  protect.  WebKit's own `testObjectiveCAPI.mm` is the reference pattern.
- `removeManagedReference` only takes effect under the **synchronous**
  collector (`JSSynchronousGarbageCollectForDebugging` plus a brief run-loop
  pump); plain `JSGarbageCollect` reclaims nothing on its own.
- **`JSGarbageCollect` corrupts the heap while VM managed references exist.**
  `formal/gc-protection.html` SIGSEGVs inside
  `JSC::ProgramExecutable::initializeGlobalProperties` on the first
  `JSEvaluateScript` after `TestUtils.gc()`.  Verified trigger: with the
  `JSVirtualMachine` managed-reference registrations disabled, the test passed
  10/10 — but without them `.then()` callbacks and stream algorithms were
  collected.  `JSValueProtect` as a substitute also crashed (its unprotect runs
  during GC finalization).  Disabling generational/concurrent GC, the JIT, or
  `JSC_verifyGC` changed nothing.
- **The unified managed edges and the per-object owner export each crashed the
  GC on their own** during the heavy streams suite (content-process SIGSEGV in
  `llint_op_call_varargs`/PAC failure while draining microtasks after a
  `setTimeout(resolve, 0)` in `tee.any.js`).  The old `JsObjectCell` managed
  values were stable (8/8 clean on `tee.any.js`), so the crashes came from the
  new machinery.  Exporting the owner from within binding callbacks matches the
  documented "property set inside a C callback crashes" class.
- **Cross-language cycles were never collectable.**  A realm-lifetime
  `protected_objects` root pinned every platform wrapper, so
  `wrapper → platform data → … → wrapper` was never freed: the edges keep the
  JS side alive, but the wrapper itself stayed rooted from Rust for the engine's
  lifetime.  Removing that root needs a Rust→wrapper reachability story (a
  Rust-held platform object must keep its wrapper alive, or `upgrade_reflector`
  must recreate it) that the engine did not have.
- JSC conservatively scans the stack: any raw `JSValueRef` in a Rust stack slot
  keeps the value alive even after `removeManagedReference`, so
  collection-after-release is not observable from Rust tests.
- The `formalWebGcAnchor` global property was enumerable (`Object.keys` listed
  it); redefining it as non-enumerable through the C API crashed from within a
  callback.

The two branches explored both mechanisms.  `jsc_objc_2` tried the unified
`GcCell`/per-object-owner design and hit the crashes above; `managed_val_js`
kept the per-value managed edges against the anchor and identified the
`JSGarbageCollect` corruption as the blocker.  Neither reached a correct,
stable GC.

## Promise settlement recording

Because `promise_state()` could not observe chained promises inside nested
calls, the engine recorded settlements on the Rust side:

- `perform_promise_then` wrapped every reaction in a builtin that recorded
  `promise pointer → (fulfilled, value)` before delegating to the original
  handler (the handler itself was rooted through a `JscManagedValue`, otherwise
  it was collected).
- `new_promise_capability` registered the resolve/reject functions
  (resolver pointer → promise pointer), so `call` could record a settlement
  synchronously even inside nested calls.
- Records were keyed by promise address.  When JSC collected a promise and
  recycled its address, a stale record misreported a genuinely-pending write as
  settled, causing a pipe to finalize early.  The fix was to **root the promise
  inside each record** (`_promise_root`), so its address could not be recycled
  while a record existed.

## Binary size

`formal-web-content` built with `--no-default-features --features v8` (no media)
is 72.80 MiB; the same build with `--features jsc` was 18.27 MiB.  This is not a
like-for-like engine-size comparison: V8 is statically linked (`librusty_v8.a`
is ~140 MiB of objects), while JSC dynamically links the system
`JavaScriptCore.framework`, whose code lives in the macOS dyld shared cache and
is not counted in the app binary.

## WPT baseline (best reached)

**Pass:** `CSS.supports`, DOM `Element` tests, Node constants, `document.title`,
`document-dir`, iframe, anchor, `formal/callback-gc-protection.html`, the full
readable/transform/writable stream suites, and all `streams/piping/*` on good
runs.

**Fail (BYOB, shared with Boa):** `enqueue-with-detached-buffer.any.js`
(structured-clone/transfer), `patched-global.any.js` (`DataView` instead of
`Uint8Array`), `respond-after-enqueue.any.js` (zero-filled read-into buffers).

**Flaky:** `streams/piping/pipe-through.any.js` and
`error-propagation-backward.any.js` intermittently TIMEOUT or SIGSEGV; the
whole `streams/piping` suite flaked roughly every other run.

**Other:** `formal/window-open-basic.html` and
`formal/window-proxy-lifecycle.html` failed on the separate-realm limitation.

## Dead ends

- **`new_shared_realm()` + shared context.**  Sharing the context to give all
  realms one heap breaks script evaluation: a child realm's `realm_global` is a
  plain object, so child-realm scripts lose `window`, and test pages load in
  child realms.
- **`JSValueProtect`/`JSValueUnprotect` as a managed-reference substitute.**
  Crashes (unprotect during GC finalization), and leaves values uncollected
  after unprotect in this JSC version.
- **Lock-holder hack.**  Wrapping each queued Rust job in a no-op builtin call to
  hold the JSLock across it fixed ordering but was superseded by the microtask
  redesign; the cached lock-holder object itself was not GC-rooted and crashed,
  and the `CURRENT_ENGINE` `RefCell` could not be borrowed while the job ran.
- **Spec-deferred pull erroring.**  Erroring the stream only on pullPromise
  rejection (per spec) fixed JSC but broke Boa's `tee.any.js`, so synchronous
  erroring plus chunk-delivery reordering was kept and the difference is not a
  JSC option.
- **Disabling generational/concurrent GC, the JIT, `JSC_verifyGC`;**
  clearing `CURRENT_ENGINE` around `JSGarbageCollect`; no effect on the
  corruption/crashes.
- **Clobbering Rust locals before `gc()`** to observe release: the pointer
  survives in registers/spill slots.
- **Non-enumerable `formalWebGcAnchor`** via the C API: crashes from within a
  callback.
- **`std::backtrace` in a SIGSEGV handler:** cannot unwind the signal frame; a
  `backtrace_symbols_fd` handler is the working repro tool.
- **Diagnostics.**  The content process did not generate crash reports on the
  development machine for these crashes, and lldb was unavailable, so no
  backtraces were captured for the post-change crashes; the exact JSC-internal
  crash mechanism was never identified.

## Research trail

- Branches `managed_val_js` (per-value managed edges against the realm anchor,
  `JSGarbageCollect` corruption identified) and `jsc_objc_2` (unified `GcCell`
  managed edges, per-object owners, microtask-job and settlement-record
  redesigns, full session logs).
- `scratchpad/jsc-objc-gc-test/*.m` — standalone Objective-C probes for which
  owner types protect under the synchronous collector, edge reachability, and
  the property-set-from-callback crash.
