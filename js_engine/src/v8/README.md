# V8 backend (`js_engine/src/v8`)

The default engine: `rusty_v8` 150.1.0, macOS arm64 only. The backend
integrates with cppgc (the unified heap) so platform objects, their `GcCell`
fields, and JS edges are collected in one pass.

## GC tracing through cppgc

- `#[gc_struct]` types implement the generic `Trace` (field-walking `trace`
  visiting every cppgc edge, plus `store` converting rooted handles into
  edges and `make_strong` rooting them back) and `GarbageCollected`, generated
  by the `gc_struct_v8` proc macro (`js_engine_macros`), which skips only
  `#[ignore_trace]` fields and copies each remaining field's `#[cfg]` onto its
  generated statement, so a conditionally-present field is traced in the build
  where it exists.
- `GcCell<T>` is a two-mode reference to a `HeapCell<T>`: a cppgc
  `Member<HeapCell<T>>` edge while the cell is stored in traced storage, and a
  cppgc `Persistent<HeapCell<T>>` root while it has no traced owner — a fresh
  cell and a clone held in Rust-owned memory. `gc_cell_new` constructs the
  rooted mode so a collection running between creation and storage cannot
  sweep the cell, and `Clone` likewise always produces the rooted mode, so a
  cell captured in a queued job or plain struct keeps the `HeapCell` and its JS
  contents alive until the clone drops; `Trace::store` converts it back to an
  edge when it re-enters traced storage. The `HeapCell` is a heap object whose
  `trace` delegates to `T`'s edges. Cells are traced from their owning platform
  object while an edge holds them and from their own root while a clone does.
- JS references are two-mode `V8Handle`s: `Root(Global)` for ephemeral Rust
  values, `Edge(Rc<TracedReference>)` once stored. Conversion happens at
  `gc_cell_new`/`GcCell::set` (via `Trace::store`), when a `GcCell::borrow_mut`
  guard is dropped (so `borrow_mut().push(...)` cannot leave a strong root in
  a cell), and at reflector writes (`ExecutionContext::store_js_object`
  converts the handle to an edge, then `with_object_any_mut` assigns the
  slot). `Trace::make_strong` is the inverse direction: `GcCell::get`/`take`
  and `with_cloned_platform_mut`'s write-back use it to root a value before it
  leaves traced storage, so a value removed from a cell (or a platform-object
  plain field cleared after being read) stays valid across engine calls.
  `Trace::store` converts only handles that are
  still `Root`, so storing an already-converted container does not open a V8
  value scope per element. A `V8Object` holds two independently created edges
  (its value handle and its typed object handle) and a typed wrapper holds its
  object's edges plus its own; `Trace` visits every one, because an edge
  skipped by the marker has its `TracedReference` storage cell reclaimed and a
  later read is a use-after-free. The `Trace` impls abort in every build on a
  store-invariant violation (a `Root` handle or a rooted `GcCell` reached by
  marking), and every edge carries the epoch it was created or last traced in:
  `V8Handle::to_local`/`make_strong` assert that no collection has run since
  then, so reading an edge whose referent may already be reclaimed fails
  deterministically instead of dereferencing a freed storage cell. The isolate
  bumps the epoch from a V8 GC prologue callback (`gc_prologue_bump_epoch`).
- Platform objects are allocated on the cppgc heap (`V8PlatformData`, a
  type-erased cppgc object tracing through the concrete type) and linked to
  their JS wrapper with `v8::Object::wrap`, so the unified heap traces
  wrapper → platform → cells → JS edges and can collect cycles.
- `gc::create_platform_object` is the single entry point that wraps concrete
  platform data (`Trace` + `Finalize`) in the backend's GC wrapper
  (`V8PlatformData` on V8, `TraceableBox` on Boa) before `create_object_with_any`; the Web IDL bindings
  (`create_interface_instance`, constructor closures, async-iterator
  objects) call it instead of reaching into `js_engine::v8::` or
  `js_engine::boa::` directly. Passing an unwrapped `Box<dyn Any>` to
  `create_object_with_any` falls back to no-op tracing — safe only for
  prototypes and namespace objects with no `GcCell` fields.
- The isolate's `CppHeap` is created with atomic marking and sweeping
  (`SharedIsolate::new`), so traces run stop-the-world on the isolate thread
  and traced-value destructors (which drop V8 handles) run on the isolate
  thread.

Forced-collection coverage lives in the js_engine test module: `engine.gc()`
(a V8 full collection plus a cppgc sweep with `NoHeapPointers`) reclaims a
wrapper↔platform reflector cycle, a two-platform mutual cycle, a JS object
referenced only through a cell edge, and a child realm whose wrapper or
function is captured by one of its own native callbacks (once the Rust realm
handle is dropped) in a single pass; platform data is finalized at isolate
destruction.

Unsafe blocks follow the repo-wide discipline in `AGENTS.md` ("Unsafe-code
discipline"). The backend-specific checks are the `GcCell` borrow counters on
the guard deref/drop paths, the edge epoch check on every handle
materialization, the platform-data writer flag (a trace or shared read during
a `with_object_any_mut` borrow aborts), strict LIFO drop order for the
thread-local engine and callback-scope guards, isolate identity on engine
entry and `with_object_any`, the null check on cached platform addresses, and
the non-shared backing store before `array_buffer_data` iterates it as
`Cell<u8>`.  A re-entrant V8 callback runs against an engine handle copied
from `CURRENT_ENGINE` (`V8Engine::handle`), never against a `&mut` to the
suspended engine, so no write goes through a pointer hidden behind that
frame's `&mut self`. Checks that run inside V8's C++ callbacks (the
cppgc `trace` functions) use `fatal_invariant!` instead of a panic.

## Native callback records and realm teardown

`make_builtin_function` records live in the shared-isolate callback registry
until the function is collected. A record built from a `Box<dyn Fn>` closure
that captures a strong JS handle roots that object; if the object
transitively references its own realm's wrappers, the context is pinned,
because a `v8::Global` is an isolate root. `create_builtin_fn_with_captures`
and `perform_promise_then` avoid this: the capture payload is a cppgc object,
its rooted handles are converted to edges by `Trace::store`, the payload is
wrapped in an API object (the holder), and the holder is attached to the
function as a private property. The record keeps only a raw pointer into the
payload, so ownership is
`function → private property → holder → payload → captured edges` — no Rust
root, and the captures are released with the function. The holder is required
because a `v8::Function` is not an API wrapper: `v8::Object::is_api_wrapper()`
is false for functions (including those built from a `FunctionTemplate` with
an instance template), so `v8::Object::wrap` on the function stores the
pointer but V8 never marks it and cppgc would sweep the payload while the
function is alive
(`function_object_private_capture_holder_is_traced`).

**Cost.** Every captured callback allocates one cppgc payload plus one
holder API object (`perform_promise_then` builds two handlers, so two of each
per `.then()` reaction), where the opaque path allocated only the record and
its closure. The behaviour stays a `fn` pointer, so call dispatch is
unchanged; the allocation and tracing volume on the promise-reaction path is
the thing to watch if `.then()` throughput regresses.

The opaque `Box<dyn Fn>` path (`create_builtin_fn` / `create_builtin_function`)
is no longer on the `JsEngine` trait — generic domain code cannot reach it,
and the Web IDL constructor in `register_interface_spec` goes through
`create_builtin_fn_static`. The concrete engines keep it as an inherent
method for their own tests; its closures must capture
**no strong JS handles** (resolve the realm's objects per call instead).

`destroy_document` still clears the document's event-listener and
event-handler callbacks, and `prune_dead_realm_callbacks` still clears the
behaviour of records whose creation realm has been dropped. These are
housekeeping and defence in depth now, not what makes a dead realm
collectable (`realm_wrapper_captured_by_a_native_callback_is_collected`).

## Build

```bash
# Build everything
rustup run 1.94.0 cargo build --release

# Run WPT suite
rustup run 1.94.0 cargo run --release -- wpt

# Run the generic engine tests
rustup run 1.94.0 cargo test -p content generic_js_test
```

The first build downloads the pinned V8 150.1.0 archive. Set
`RUSTY_V8_ARCHIVE=/absolute/path/to/librusty_v8_release_aarch64-apple-darwin.a.gz`
to use a local archive, or set `RUSTY_V8_MIRROR` to an alternate releases base
URL. Cargo also caches downloaded archives under `.cargo/.rusty_v8` in the
Cargo home directory.

## WPT results

Last recorded: `executed=280 unexpected=0`.

## Remaining work

1. **Edge equality approximation in `PartialEq`.** `V8Object`/`V8Symbol`
   `PartialEq` uses `V8Handle::same_identity`, which is exact for two roots or
   clones of one edge but under-approximates mixed and independently-created
   edges.  Live identity paths no longer use it: the associated-object lookup
   and the structured-clone memory map use `ExecutionContext::same_object`,
   which materializes both sides through `same_value`.  The impls remain only
   to satisfy the generic `JsTypes` trait bounds, and are documented as
   approximate.
2. **Scope-macro `&mut PinScope` reborrow, and the stored scope's
   `'static` lifetimes.** The `v8_engine_scope_with_*`
   macros reborrow the callback scope as `&mut` on each nested engine call
   during a native callback. The references never overlap in use (each is
   created, used for a bounded sequence of C calls, and dropped) and the
   underlying memory is C++-owned, but the pattern is not Stacked-Borrows
   clean; a Miri run would flag it.  The stored scope type
   (`StoredCallbackScope`) is `v8::PinScope<'static, 'static>`, so a `Local`
   materialized through it carries a `'static` lifetime and code that stored
   one would keep it past its `HandleScope`; no code stores a `Local` (only
   `Global`/`TracedReference` handles are kept), so the lifetime is a missing
   compile-time check rather than a live dangling handle.  Keeping the real
   lifetimes needs the scopes passed as parameters or a closure-based
   accessor; neither is implemented.
3. **The opaque closure path remains on the concrete engines.**
   `create_builtin_fn` / `create_builtin_function` are no longer `JsEngine`
   trait methods, so generic domain code cannot use them; they survive as
   inherent methods for the engine tests. A future
   production caller holding a concrete engine could still pass a closure
   with rooted captures, so the doc-comment rule (capture no strong JS
   handles) still applies.
4. **Strong roots remain in host-data holders.** `store_host_any` values and
   `AssociatedPlatform.object` hold strong `V8Object` roots inside the
   realm's host-data holder, so a dead realm needs one extra collection
   cycle. This is safe today only because nothing JS-reachable points at the
   host-data holder; a JS path to it would form a strong cycle none of the
   current tests catch.
5. **`is_constructor` has no exact check.** See the IsConstructor gap under
   "ArrayBuffer / IsConstructor gaps".
6. **`Trace for Rc<T>` roots nothing on its own.**
   `store`/`make_strong` on a shared value are no-ops, so a Rust-held `Rc`
   clone shares whatever edges the traced copy owns and is only valid while
   that copy keeps tracing them; the epoch check aborts a read once a
   collection has run without visiting the shared edge. Give a `Rc`-shared
   value its own root (or store it in a `GcCell`) when it must outlive its
   traced owner.
7. **`create_realm` retains every realm state.**
   `V8Engine::created_realm_states` holds a strong `Rc<V8RealmState>` per
   `create_realm` call so the realm state outlives the returned handle. Only
   the generic engine tests call `create_realm`, but nothing removes the
   entries, so each call leaks the realm's intrinsics and host-data holder
   for the engine's lifetime.
8. **Traced-capture payload is reached through a raw pointer.**
   `create_builtin_fn_with_captures` caches a `*const C` into the capture
   holder and dereferences it from the callback closure. The holder is alive
   exactly while the function is callable (the closure cannot run after the
   function dies), so the pointer is sound, but nothing re-validates it at
   call time. Resolving the payload from the function's holder instead would
   remove the raw pointer; the `native_callback` record is read from its
   owning slot rather than a cached pointer.
9. **`gc()` assumes no cppgc pointers on the Rust stack.**
   `V8Engine::gc` sweeps with `EmbedderStackState::NoHeapPointers`.  A
   `GcCell` borrow guard can no longer be live across it (the guard borrows
   the execution context), but any other cppgc pointer in a caller frame — a
   `Member` read into a local, a cached platform address — would break the
   claim and let the sweep reclaim a reachable object.  The hook is
   test-only.

### ArrayBuffer / IsConstructor gaps

- `allocate_array_buffer`/`allocate_shared_array_buffer` honor the supplied
  constructor via a full `[[Construct]]` call, but `AllocateArrayBuffer`
  (§25.1.2.1) only reads the constructor's `.prototype`
  (`OrdinaryCreateFromConstructor`) and never runs a subclass constructor
  body. Untriggered today — every caller passes the realm's intrinsic
  `ArrayBuffer` — but a future `SpeciesConstructor`-aware
  `ArrayBuffer.prototype.slice` would run subclass bodies the spec forbids.
- `is_constructor` (the `ObjectProfile` bit cached at wrap time) is a
  heuristic — own `prototype` property with generator/async functions
  excluded — not ECMA-262 `IsConstructor` (§7.2.4). It is wrong for bound
  constructors ([[Construct]] mirroring a constructible target, no own
  `prototype`) and for arrow functions given a manual `prototype`
  assignment; a Proxy is classified as not-a-constructor without probing it.
  `delete Foo.prototype` cannot produce a false negative — `prototype`
  is non-configurable on functions. The exact check is
  `v8::Object::IsConstructor` (`v8-object.h`); `rusty_v8` 150.1.0 binds only
  `StackFrame::IsConstructor`, so a one-line upstream binding (or a patch to
  the `v8` crate) is required.

## WebAssembly

V8 implements WebAssembly natively — no `wasm` feature is needed and
there is nothing to implement on our end. The `wasm` feature is the
Wasmtime-based WebAssembly API for the Boa backend (see `../boa/README.md`).
