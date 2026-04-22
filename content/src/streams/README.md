**Streams conventions**

- Keep Web IDL-visible stream methods on the native carrier types in this directory, and keep `content/src/boa/bindings/streams` limited to argument conversion, downcasting, and delegation.
- Store stream internal slots as native carriers backed by shared Rust state where possible instead of raw `JsObject` handles, so spec algorithms can operate on typed Rust state directly.
- Prefer typed DOM carriers such as `AbortSignal` over raw wrapper-object handles in stream internals and related DOM integration; convert back to `JsObject` only at Web IDL boundaries.
- Keep `ReadableStreamPipeTo` abort handling in typed Rust state stored on `AbortSignal` abort algorithms, and unregister that state during pipe finalization instead of wiring signal callbacks through JavaScript.
- When `ReadableStreamPipeTo` is waiting for pending writes during shutdown, re-check destination error/close state before executing a queued close action so a last-write failure rejects the pipe instead of being masked by forward close propagation.
- Model shared stream mixins and algorithm slots with Rust traits and enums when the spec describes them as reusable behavior or polymorphic internal algorithms.
- Keep abstract operations that primarily mutate `WritableStream`, `WritableStreamDefaultController`, or `WritableStreamDefaultWriter` as methods on those carrier types, so the Rust implementation follows the spec receiver directly.
- Keep `ReadableStream`, `ReadableStreamDefaultController`, and `ReadableStreamDefaultReader` in separate files, keep binding-only wrapper creation and downcasts in `content/src/boa/bindings/streams`, and invoke underlying-source callbacks with the original `underlyingSource` object as the Web IDL callback this value rather than caching a controller wrapper alongside the algorithm.
- Treat harness-only writable stream failures as an integration signal first: confirm the same behavior with direct probes before changing stream algorithms for a `testharness.js`-specific failure.
- Use the spec in formal-web/web_standards/Streams.html and the wpt test suite in /formal-web/vendor/wpt/streams.