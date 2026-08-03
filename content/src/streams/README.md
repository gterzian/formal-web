# content/src/streams

`content/src/streams` owns the native Streams [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) and Streams Standard algorithms used by the content process.

- All stream code operates exclusively on the generic `js_engine` trait API.
  Zero `boa_engine::*` or `boa_gc::*` imports in the entire `streams/` directory.
- Use the local type alias pattern:
  ```rust
  use crate::js::Types;
  type JsValue = <Types as JsTypes>::JsValue;
  type JsObject = <Types as JsTypes>::JsObject;
  type ArrayBuffer = <Types as JsTypes>::ArrayBuffer;
  ```
- Keep Web IDL-visible stream methods on the [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) types here, and keep `content/src/js/bindings/streams` limited to argument conversion, [inherited interfaces](https://webidl.spec.whatwg.org/#dfn-inherited-interfaces) checks, and delegation.
- Match each [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) method's return channel to the Web IDL contract: throwing operations use `JsResult`, while promise-returning operations create and settle their promise on the platform object side.
- Prefer typed Rust state for internal slots and related DOM integration, converting back to `JsObject` only at Web IDL boundaries.
- Keep long-lived pipe state, abort handling, and finalization on typed Rust state instead of routing them through JavaScript callbacks.
- Model shared mixins and abstract operations with Rust traits or receiver-owned methods when the spec describes reusable behavior.
- Use the `web_standards` extension (`spec_lookup`) with `https://streams.spec.whatwg.org/` to read the Streams spec, and `vendor/wpt/streams` as the test reference.