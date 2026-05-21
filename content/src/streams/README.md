# content/src/streams

`content/src/streams` owns the native stream carriers and Streams Standard algorithms used by the content runtime.

- Keep Web IDL-visible stream methods on the carrier types here, and keep `content/src/boa/bindings/streams` limited to argument conversion, downcasting, and delegation.
- Match each carrier method's return channel to the Web IDL contract: throwing operations use `JsResult`, while promise-returning operations create and settle their promise on the carrier side.
- Prefer typed Rust state for internal slots and related DOM integration, converting back to `JsObject` only at Web IDL boundaries.
- Keep long-lived pipe state, abort handling, and finalization on typed Rust state instead of routing them through JavaScript callbacks.
- Model shared mixins and abstract operations with Rust traits or receiver-owned methods when the spec describes reusable behavior.
- Use `web_standards/Streams.html` and `vendor/wpt/streams` as the spec and test references.