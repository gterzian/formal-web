**Streams conventions**

- Keep Web IDL-visible stream methods on the native carrier types in this directory, and keep `content/src/boa/bindings/streams` limited to argument conversion, downcasting, and delegation.
- Store stream internal slots as native carriers backed by shared Rust state where possible instead of raw `JsObject` handles, so spec algorithms can operate on typed Rust state directly.
- Model shared stream mixins and algorithm slots with Rust traits and enums when the spec describes them as reusable behavior or polymorphic internal algorithms.
- Keep `ReadableStream`, `ReadableStreamDefaultController`, and `ReadableStreamDefaultReader` in separate files, keep `reflector` as the first field on each exposed carrier, and invoke underlying-source callbacks with the original `underlyingSource` object as the Web IDL callback this value.