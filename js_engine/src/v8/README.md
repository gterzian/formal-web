# `v8` — V8 backend

V8 backend for `js_engine` via `rusty_v8` (macOS arm64 only, feature `v8`).

## Build & test

```bash
# Build every process with V8 and media support
rustup run 1.94.0 cargo build --release \
  --no-default-features --features v8,media

# Run the browser after the complete build
rustup run 1.94.0 cargo run --release \
  --no-default-features --features v8,media

# Run the generic engine tests
rustup run 1.94.0 cargo test --no-default-features \
  --features v8 -p content generic_js_test
```

The first build downloads the pinned V8 150.1.0 archive.  Set
`RUSTY_V8_ARCHIVE=/absolute/path/to/librusty_v8_release_aarch64-apple-darwin.a.gz`
to use a local archive, or set `RUSTY_V8_MIRROR` to an alternate releases base
URL.  Cargo also caches downloaded archives under `.cargo/.rusty_v8` in the
Cargo home directory.

WebAssembly support is deferred for V8; use Boa with the `wasm` feature.

## Open issues

- **Platform-object tracing through cppgc** — V8 stores generic `GcCell<T>`
  in `Rc<RefCell<T>>` and keeps reflectors through V8 weak handles.
  Migrate platform-object ownership to a `cppgc::Heap` attached to each
  shared isolate.  Objects allocated on that heap must trace every
  `Member`, `WeakMember`, and `TracedReference` edge, while off-heap
  owners use `Persistent` handles only when they are genuine roots.

  The generic cell API must change as part of this work: cppgc allocation
  needs the isolate heap, and cppgc cell access requires isolate-scoped
  proof instead of the current context-free `gc_cell_new`, `borrow`, and
  `borrow_mut` calls.  Add forced-collection tests covering reflector
  cycles, platform-object cycles, weak edges, finalization, and isolate
  destruction.
