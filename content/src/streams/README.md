**Streams conventions**

- Keep Web IDL-visible stream methods on the native carrier types in this directory, and keep `content/src/boa/bindings/streams` limited to argument conversion, downcasting, and delegation.
- Store stream internal slots as native carriers backed by shared Rust state where possible instead of raw `JsObject` handles, so spec algorithms can operate on typed Rust state directly.
- Prefer typed DOM carriers such as `AbortSignal` over raw wrapper-object handles in stream internals and related DOM integration; convert back to `JsObject` only at Web IDL boundaries.
- Model shared stream mixins and algorithm slots with Rust traits and enums when the spec describes them as reusable behavior or polymorphic internal algorithms.
- Keep abstract operations that primarily mutate `WritableStream`, `WritableStreamDefaultController`, or `WritableStreamDefaultWriter` as methods on those carrier types, so the Rust implementation follows the spec receiver directly.
- Keep `ReadableStream`, `ReadableStreamDefaultController`, and `ReadableStreamDefaultReader` in separate files, keep `reflector` as the first field on each exposed carrier, and invoke underlying-source callbacks with the original `underlyingSource` object as the Web IDL callback this value.
- Treat harness-only writable stream failures as an integration signal first: confirm the same behavior with direct probes before changing stream algorithms for a `testharness.js`-specific failure.