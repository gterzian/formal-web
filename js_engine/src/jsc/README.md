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
document-dir, iframe, anchor, formal gc-protection, callback-gc-protection,
all `streams/piping/*` (including `abort.any.js` when run), and the full
readable/transform/writable stream suites: `async-iterator`, `from`,
`patched-global` (default streams), `tee`, `general`, `cancel`,
`default-reader`, `read-task-handling`, strategies, transform, writable.

**FAIL (BYOB, pre-existing):**

- `streams/readable-byte-streams/enqueue-with-detached-buffer.any.js` —
  "The object could not be cloned." (structured-clone/transfer behavior).
- `streams/readable-byte-streams/patched-global.any.js` —
  `result.value` is a `DataView` instead of the requested `Uint8Array`
  (BYOB read-into view construction).
- `streams/readable-byte-streams/respond-after-enqueue.any.js` —
  read-into buffers come back zero-filled (BYOB buffer-fill bug).  The
  last two also fail on Boa (zero-fill family); the first passes on Boa.

**Flaky (infra):** `streams/piping/pipe-through.any.js` intermittently
`ERROR`/`TIMEOUT` (first subtest: "Piping through a transform errored on
the writable end does not cause an unhandled promise rejection") in full
suite runs; passes standalone repeatedly.  Also affects Boa, so it is not
engine-specific.

The 2026-08-03 session (log below) fixed the `error-propagation-forward`
early-finalize FAIL (stale settlement records), the `abort.any.js` and
`close-propagation-*` SIGSEGV crashes (collected `Callback` objects), and
one stack-overflow SIGSEGV (re-entrant `promise_state` eval fallback).
`error-propagation-backward.any.js` and `pipe-through.any.js` still fail
intermittently (TIMEOUT, or SIGSEGV after the pipe settles); the system
JavaScriptCore was judged not viable for a web engine and no further
fixes were pursued.

Generic engine tests: 95/96 pass; only
`generic_js_test::tests::constructor_has_function_prototype_methods_on_jsc`
fails (`TestWidget.toString()` returns `[object FormalWebBuiltin]`).

## Microtask jobs

Rust-side "queue a microtask" jobs (`enqueue_job` / `enqueue_job_with_realm`)
are enqueued into **JSC's own microtask queue** rather than a separate Rust
list: the job closure is stored as private data on a `JOB_CLASS` function
object and queued via `Promise.resolve(undefined).then(jobFn)` using the
captured `%Promise.prototype.then%`.  JSC runs these interleaved FIFO with
all other microtasks, so a job's promise resolutions queue reactions that
run after it (spec microtask semantics).  During a drain the JSLock is
held (drain runs in `JSLockHolder::willReleaseLock`), so a job's nested
C API calls never re-drain mid-job.

Depth-0 enqueues (no JS call active — e.g. the content process's microtask
checkpoint) are deferred into `JscEngine::pending_jobs`, because calling
into JS to queue a microtask at depth 0 is itself the outermost C API
call and JSC drains on its return, which would run the job synchronously.
`run_jobs` / `perform_a_microtask_checkpoint` flush `pending_jobs` (under
an `EngineGuard`) and force a drain with `eval_script_raw("void 0")`.
`queueMicrotask` does **not** exist in this JSC (`typeof === "undefined"`),
so the promise+then hop is the substitute.

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

All `streams/piping/*` tests pass (including `abort.any.js`, which runs
when metadata is ignored for explicit paths).  The pipe state machine and
the JSC promise-state plumbing were reworked earlier; this session added:

- **Microtask job interleaving** — the tee "only pull enough to fill the
  emptiest queue" test failed because the separate Rust job queue ran
  outside JSC's microtask ordering: at the depth-0 checkpoint, a job's
  nested `call()` (resolving a read promise) is the outermost C API call
  and JSC drained microtasks mid-job, firing `Promise.all` reactions
  before the job's synchronous readAgain-repull.  Fixed by moving jobs
  into JSC's microtask queue (see "Microtask jobs" above).
- **Source error ordering** — "errors in the source should propagate to
  both branches" failed because `pull_steps` called `CallPullIfNeeded`
  (which errors the stream synchronously when the pull algorithm throws)
  *before* the read-request chunk steps queued the tee's chunk-delivery
  job, so the branch error reaction ran before `'b'` reached branch1.
  Fixed by delivering the chunk (`chunk steps`) before calling pull;
  see `content/src/streams/README.md`.

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
- **Record-rooted promises** — each `PromiseSettlement` additionally roots
  its promise via a managed reference (`_promise_root`).  While a record
  exists the promise cannot be collected, so its address cannot be recycled
  and a record found at an address always describes the promise currently
  there.  This closes the stale-record window for *native* promises (e.g. a
  test sink's `new Promise(...)`) that never pass through
  `perform_promise_then`/`new_promise_capability`; without it,
  `promise_state` misreported a genuinely-pending write as settled and the
  pipe finalized early (`error-propagation-forward` "shutdown must not
  occur until the final write completes").

## Open issues

- **Piping test infra flakiness** — `pipe-through.any.js` and
  `error-propagation-backward.any.js` intermittently report `TIMEOUT` or
  crash with a content-process `SIGSEGV`; the `error-propagation-forward`
  early-finalize FAIL and the `abort.any.js`/`close-propagation-*` crashes
  were fixed by the 2026-08-03 session (stale-record rooting, `Callback`
  rooting, `promise_state` re-entrancy guard — see the session log).  The
  `main` branch (with the plain Rust job queue, before the microtask-job
  redesign and the ObjC managed-reference rooting) does **not** reproduce
  this, so the regression is from the recent JSC commits (`d88c9f36e`,
  `0f171bf24`, `57e774a74`).  The earlier `SIGSEGV` in `run_window_timer`
  (dangling timer callback) is fixed by the timer GC rooting, but the
  piping flakiness remains and the system JavaScriptCore was judged not
  viable for a web engine (verdict in the session log).
- **`uncaught callback error: undefined` during gc-protection** — the
  first `setTimeout(0)` callback reports this error (logged, not thrown).
- **Microtask drain during nested C API calls** — JSC only drains its
  queue when control returns from the outermost C API call; inside nested
  calls `.then()` handlers never fire.  Mitigated by the settlement
  recording below plus the microtask job redesign, but the eval-based
  fallback in `promise_state()` still returns `Pending` for untracked
  promises inside reactions.
  *Failed attempt:* no public C API forces JSC microtask drainage; a
  `CFRunLoopRunInMode` pump inside a reaction does not drain the remaining
  queue either.
- **`setTimeout` not pumped during piping tests** — `delay()` timeouts.
- **`instanceof Window` returns false** — the global object's
  `[[Prototype]]` is immutable through the public C API.
- **`detach_array_buffer`** — no-op (`Ok(())`); also the original chunk
  buffer is never detached in byte-stream `enqueue_steps` (only cloned),
  which contributes to the detached-buffer BYOB failure.
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

## Session investigation log

### 2026-08-03 — JSC stream suite parity with Boa

**Files changed:** `js_engine/src/jsc/engine.rs`, `js_engine/src/jsc_sys.rs`,
`content/src/html/global_scope.rs`, `content/src/html/environment_settings_object.rs`,
`content/src/html/window_or_worker_global_scope.rs`,
`content/src/streams/readablestreamdefaultcontroller.rs`.
**Instrumentation added:** temporary `eprintln!` markers in the tee pull
algorithm, chunk-delivery job, `call_pull_if_needed`, and pipeTo state
machine; all removed.  Browser repro pages under `scratchpad/` (removed at
end of session).
**What was confirmed:**
- Symbol property keys were silently no-ops (`define_property_or_throw`
  returned `Ok` for `JscPropertyKey::Symbol`), so
  `ReadableStream.prototype[@@asyncIterator]` was never installed.
  Fixed with `JSObjectGetPropertyForKey`/`JSObjectSetPropertyForKey`/
  `JSObjectHasPropertyForKey`/`JSObjectDeletePropertyForKey` (macOS
  10.15+).
- `realm_intrinsics().async_iterator_prototype` was a placeholder
  (`Object.prototype`); the real `%AsyncIteratorPrototype%` is reachable
  via `Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}
  ).prototype)` and is what `for await` needs (`%AsyncIteratorPrototype%
  [@@asyncIterator]` returns `this`).
- `to_object` returned raw primitive cells as objects; a string arg to
  `ReadableStream.from('ab')` crashed `JSObjectGetPropertyForKey`.  Fixed
  with `JSValueToObject`.
- `perform_promise_then` read the user-visible `then` property; the
  patched-global test required the intrinsic `%Promise.prototype.then%`,
  captured at engine construction.
- Tee pull-count ordering failure root cause: the separate Rust job
  queue + JSC draining microtasks at the outermost C API call return
  (see "Microtask jobs" above).
- Tee error-propagation failure root cause: `pull_steps` ordered
  `CallPullIfNeeded` (synchronous stream error on pull throw) before the
  chunk delivery, so the branch error reaction ran before `'b'` was
  delivered.  Fixed by delivering the chunk first.
- Timer `run_window_timer` `SIGSEGV` root cause: `WindowTimer`'s
  callback/arguments were unrooted on JSC; the working `gc()` collected
  them.  Fixed with `GcRootHandle` (`protect_value`) roots stored on
  `WindowTimer`.
- Freshly rebuilt helper binaries intermittently get `SIGKILL (Code
  Signature Invalid)` on macOS; re-sign with
  `codesign --force --deep -s - target/release/formal-web*`.  Caused
  false "regressions" (e.g. `enqueue-with-detached-buffer` "failing" on
  Boa) and "WebDriver child did not become ready" flakiness.
**What was ruled out:**
- *Lock-holder hack:* wrapping each queued Rust job in a no-op
  `JOB_CLASS`-style builtin call to hold JSC's JSLock across the job
  fixed the ordering but was superseded by the microtask redesign.  Two
  pitfalls found: a cached lock-holder object is not GC-rooted and gets
  collected (dangling pointer crash), and the `CURRENT_ENGINE` RefCell
  must not be borrowed while running the job (re-borrow panic).
- *`queueMicrotask`:* not available in this JSC (`typeof === "undefined"`).
- *Spec-deferred pull erroring:* reverting `call_pull_if_needed` to error
  the stream only upon pullPromise rejection (spec) fixed JSC but broke
  Boa's `tee.any.js` (Boa runs the pullPromise rejection reaction *after*
  a subsequently queued `enqueue_job` microtask, so `reader2.read()`
  resolved `'a'` before the tee branches errored).  Kept synchronous
  erroring and reordered chunk delivery instead.
**Not investigated:** BYOB failures (`enqueue-with-detached-buffer`
structured-clone, `patched-global` DataView-vs-Uint8Array,
`respond-after-enqueue` zero-fill); the pipe-through full-suite TIMEOUT;
`queueMicrotask` availability beyond the page global.

### 2026-08-03 — piping test flakiness (pipe-through TIMEOUT) on JSC

**Files changed:** `content/src/streams/readablestream.rs`
(env-gated `[stream-debug][pipe]` logging: `log_pipe_debug` calls in
`shutdown`, `finalize`, `write_chunk`, `perform_action`,
`prune_settled_pending_writes`, `pipe_to_on_promise_settled`),
`js_engine/src/jsc/engine.rs` (`[stream-debug][jsc]` logs in
`enqueue_js_microtask`/`job_call_as_function`; backtrace in
`report_exception`), `tests/formal/tests/pipe-repro.html` (repro page;
passes 20/20 standalone, removed after session).  All logging is gated
behind `FORMAL_WEB_DEBUG_STREAMS=1`.
**Reproduction:** `formal-web-wpt streams/piping` (13 tests) flakes on
JSC roughly every other run.  Failure modes seen across ~50 runs:
`pipe-through.any.js` (whole-file TIMEOUT, or subtest TIMEOUT
"Piping through a transform errored on the writable end..." with the
rest NOTRUN), `error-propagation-backward.any.js` (whole-file TIMEOUT),
`error-propagation-forward.any.js` (FAIL "shutdown must not occur until
the final write completes; preventAbort = true" — the pipe finalized
early), and one content-process `SIGSEGV` in `abort.any.js` (during job
execution).  Full-suite runs flake the same way (`wpt_full_2/3`).  The
failing subtest passes standalone (20/20).
**What was confirmed (instrumentation):**
- The pipe state machine **completes correctly** in the TIMEOUT cases:
  `finalize` is reached and the pipe promise settles with the right
error.  The hang is in the JS-level promise/timer machinery, not the
pipe: the test's `flushAsyncEvents` (4 chained `setTimeout(0)` + `.then`)
stalls while the testharness 10s timeout timer still fires — so plain
timer callbacks work but promise reactions stop being processed.
- Early-finalize evidence: `shutdown: should_wait=false
pending_writes=1` occurs in `error-propagation-backward` (spec-valid
when the destination is erroring/errored — the spec only waits for
pending writes while dest is "writable").  For the failing
`error-propagation-forward` test the destination stays writable, yet no
`should_wait=false pending_writes>=1` appears in that page's log — the
pending write must have been pruned from `pending_writes` (i.e. JSC
`promise_state` reported a genuinely-pending write as settled) before
shutdown ran.
- `TypeError: f is not a function. (In 'f()', 'f' is an instance of
FormalWebPlain)` — a JSC function object was GC-collected and its memory
recycled while still referenced (queued microtask / reaction record),
consistent with engine-created function objects (JOB_CLASS job fns from
the `Promise.resolve().then(jobFn)` microtask redesign; settlement-
reaction wrappers from `wrap_settlement_reaction`) not surviving GC.
- "uncaught callback error: error1: error1!" — backtrace (user-run +
instrumentation): `report_exception` from `content::dom::dispatch::invoke
← dispatch_event ← fire_event ← continue_document_load`, i.e. during the
window "load" event.  No test/harness/inject code registers a "load"
listener, and the listener type filter is correct — which listener
throws error1 is unresolved.  This error is a symptom, not the cause
(failing runs occur without it).
**What was ruled out:**
- Boa reproduction: not achieved anywhere.  Current-repo Boa build: 8
piping + 3 full-suite runs clean.  `main` branch (`../formal-web`,
pace_frame_rate, Boa): 10 piping + 6 full-suite runs + user's full-suite
run clean.  The "also affects Boa" note in the earlier flakiness entry
could not be confirmed.
- Pre-commit regression point: the full pre-commit workspace build (at
`8f7b04ee4`, before `d88c9f36e`/`0f171bf24`/`57e774a74`) was never
completed, so the regression point is inferred from `main` being clean,
not directly tested.
- `__fw_ps_*` global clobbering in the eval-based `promise_state`: the
pipe paths run at nesting depth >= 1 where the eval-drain is a no-op, so
the re-entrancy-clobbering scenario was not confirmed.
- The `write_in_progress`/pending-writes wait logic (`0f171bf24`): the
`should_wait=false` + pending-write case matches the spec when dest is
erroring; not itself the bug.
**Not investigated:** which listener throws error1 during the "load"
event; the exact microtask-stall mechanism (GC of JOB_CLASS/reaction
function objects is the leading hypothesis, unverified); whether the
flake predates `d88c9f36e` on JSC (needs a full pre-commit workspace
build).

### 2026-08-03 — piping flakiness: stale records, collected callbacks, promise_state re-entrancy; JSC deemed unviable

**Files changed (kept):** `js_engine/src/jsc/engine.rs` —
`PromiseSettlement` now roots its promise (`_promise_root` managed
value); `promise_state` has an `in_promise_state_eval` re-entrancy guard
with the eval fallback extracted to the free function
`promise_state_eval_fallback`.  `content/src/webidl/callback.rs` —
`Callback::from_object` roots the callback object via `create_root`
(managed reference; no-op on Boa).
**Instrumentation added (all removed):** env-gated `[stream-debug][jsc]`
logs (EngineGuard drain points, `run_jobs`, eval `void 0`,
`perform_promise_then` attaches, settlement-reaction fires, resolver
hits, `promise_state` record hits, `write_chunk` value, `finalize`
reject), a `Promise.prototype.then` reaction-fire tracer injected into
the test pages via the WPT runner, a temporary lldb wrapper around the
content-process spawn (no lldb installed on the machine — removed), and
unit tests probing the capability-reject flow, the `void 0` drain, and
managed-reference survival under the automatic collector.
**Reproduction:** full `streams/piping` suite flakes on JSC on most runs;
failure modes seen: `pipe-through.any.js` TIMEOUT,
`error-propagation-backward.any.js` TIMEOUT or ERROR (content-process
SIGSEGV), `error-propagation-forward.any.js` FAIL ("shutdown must not
occur until the final write completes; preventAbort = true"),
`abort.any.js` SIGSEGV (during job execution).
**What was confirmed:**
- **Early-finalize FAIL = stale settlement records.**  `settled_promises`
  is keyed by promise address; after JSC collects a promise and recycles
  its address, a stale record misreports a genuinely-pending write as
  settled, so `prune_settled_pending_writes` removes it and `shutdown`
  computes `should_wait=false` → the pipe finalizes before the final
  write completes.  The pre-existing cleanup only covered promises
  passing through `perform_promise_then`/`new_promise_capability`, not
  native promises (a test sink's `new Promise(...)`).  Fixed by rooting
  the promise inside each record: while a record exists its address
  cannot be recycled, so records can never go stale.  This eliminated the
  `error-propagation-forward` FAIL in suite runs.
- **SIGSEGV in `writer.write` = collected callback objects.**  The crash
  report `formal-web-content-2026-08-03-052931.ips` shows a JOB_CLASS job
  running inside a timer callback's return drain: `run_window_timer →
  JscEngine::call → JSObjectCallAsFunction → [return] JSLockHolder →
  willReleaseLock → drainMicrotasks → job_call_as_function →
  pipe_to_on_promise_settled → write_chunk → writer.write →
  JSObjectIsFunction` on a near-null `JSObjectRef`.  `Callback` held a
  raw, unrooted `JsObject`; a sink method (e.g. `write`) referenced only
  from Rust was collected by JSC's automatic GC, and
  `invoke_callback_function`'s `IsCallable` check dereferenced the
  dangling pointer.  Fixed by rooting the callback object in
  `Callback::from_object`.  `abort.any.js` and `close-propagation-*`
  stopped crashing in suite runs.
- **Stack-overflow SIGSEGV = re-entrant `promise_state` eval fallback.**
  With the debug logs on, failing runs showed a repeating cycle:
  `promise_state: no settled record` → eval `void 0` → `settlement
  reaction fired` → `on_promise_settled` (ShuttingDownPendingAction) →
  `promise_state` again — consistent with the eval fallback firing queued
  reactions at each JSEvaluateScript boundary, a drained reaction
  re-entering `promise_state`, which evals and drains again (unbounded
  recursion until stack overflow).  Fixed with the `in_promise_state_eval`
  guard: re-entrant calls return `Pending` without evaluating.
  *Caveat:* the README's earlier claim ("JSC only drains its queue at the
  outermost C API call") was not conclusively verified either way — a
  unit-test attempt to observe nested-drain fire timing was flawed (the
  reaction fired at a top-level eval return, not during the nested call).
  The guard is correct regardless of drain semantics: it bounds
  re-entrancy.
- Crash reports are **not** generated for the content process on this
  machine (DiagnosticReports empty for the crashing runs), and lldb is
  not installed, so no backtrace was captured for the post-fix crashes.
**What was ruled out:**
- Managed references (JSManagedValue + addManagedReference) being
  ineffective under the automatic collector: a targeted unit test could
  not isolate the mechanism — JSC's conservative stack scan keeps any
  pointer-looking stack slot alive, so the object stayed alive regardless
  of the managed edge.
- `promise_state` eval fallback `__fw_ps_*` global clobbering as the sole
  failure cause: the re-entrancy guard subsumes the failure mode.
**Remaining failures after the fixes:** `error-propagation-backward.any.js`
still intermittently TIMEOUTs or SIGSEGVs (the post-fix crash occurs
right after the pipe promise settles and the user-level `.then` reactions
fire; the dangling value there was not identified before the decision to
stop), and `pipe-through.any.js` TIMEOUT persists.
**Verdict:** the system JavaScriptCore's microtask/GC model — drains at
every C API boundary; the automatic collector reclaiming values held only
in Rust structures; the eval-based `promise_state` fallback running
arbitrary JS at nested depth — is not viable for a web engine.  The
principled fix (hold the JSLock across the entire content-command
handling so microtasks drain only at the explicit checkpoint) is not
implementable through the public C API.  The JSC backend remains
experimental; Boa is the supported engine.
