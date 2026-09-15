# content/src/html/structured_data

Implements the HTML spec's "safe passing of structured data"
(<https://html.spec.whatwg.org/#safe-passing-of-structured-data>): the
structured serialization algorithms that turn JavaScript values into
IPC-serializable records and back.

The folder splits the spec section between the generic algorithms and the
per-platform-object parts of those algorithms, so a platform object's
transfer/`[Serializable]` handling does not grow the algorithm file:

- `safe_passing_of_structured_data.rs` — the generic algorithms
  (`StructuredSerializeInternal`, `StructuredDeserialize`, the
  `*WithTransfer` variants, the `structuredClone` API, the `Serializable` /
  `Transferable` traits, the `MemoryMap`).  No platform-object-specific code
  lives here: transferable platform objects plug in through the
  `Transferable` trait, and the per-interface transfer steps live in their
  own module.
- `messageport.rs` — the MessagePort-specific parts: recognizing a
  transferable MessagePort (the [[Detached]]-slot check of
  `StructuredSerializeWithTransfer` step 2.1), running its transfer steps
  and building its data holder (step 5.2), and rebuilding the port on the
  receiving side (`StructuredDeserializeWithTransfer` step 3.2).
- `offscreen_canvas.rs` — the OffscreenCanvas-specific parts: recognizing a
  transferable OffscreenCanvas, running its transfer steps (carrying the
  canvas id and bitmap dimensions in the data holder), and rebuilding the
  canvas on the receiving side.

The wire-format data (`SerializedRecord`, `TransferDataHolder`,
`PortTransferData`, `PortMessagePayload`, `PostMessageRequest`) lives in
`ipc_messages::safe_passing_of_structured_data` — it is the wire format and
must be defined in the crate both processes link.

The MessagePort platform object itself stays in
`content/src/html/messageport.rs` (it implements the `Transferable` trait
for `PortTransferData`); this folder only holds the parts of the
safe-passing algorithms that are specific to it.  Future transferable or
`[Serializable]` platform objects get their own module here the same way.

## Serialization pitfalls (the generic algorithms)

The generic algorithms round-trip values between JS and the wire format, so
a conversion that is fine for display corrupts data.  The algorithm bodies
show the correct calls; the pitfalls below say why the tempting wrong call
is wrong.

### String round-tripping — use UTF-16 units, never a display-escaped string

Strings are serialized as raw UTF-16 code units. Any display/escaping
conversion (one that replaces unpaired surrogates with literal `\uXXXX`
escape sequences) corrupts strings like lone surrogates (`\uD800`, `\uDC00`).

**Correct serialization:**
```rust
let utf16_units: Vec<u16> = ec.js_string_to_rust_string(&s).encode_utf16().collect();
```

**Correct deserialization:**
```rust
let js_string = ec.js_string_from_str(&String::from_utf16_lossy(&utf16_units[..]));
```

### RegExp source — `[[OriginalSource]]` vs the escaped getter

The `source` accessor on RegExp applies `EscapeRegExpPattern` (spec 22.2.3.2.5),
which escapes `/`, `\n`, `\r`, `\u2028`, and `\u2029`. Passing the escaped form
back to the RegExp constructor produces a different pattern. Always store the
raw `[[OriginalSource]]`: read `ec.get_regexp_source` and reverse the escaping
with `unescape_regexp_source()`.

### Error "message" — `[[GetOwnProperty]]`, not `[[Get]]`

The spec step for Error serialization uses `[[GetOwnProperty]]` for the
"message" property — this checks only own data descriptors, ignores the
prototype chain, and does not invoke accessors. Using the generic get is
wrong; read `ec.get_own_property` and take the value from the data
descriptor.

### EnumerableOwnProperties — filter by enumerability

The spec uses `EnumerableOwnProperties(value, "key")`, which returns only
enumerable own property keys. `ec.own_property_keys` returns ALL own keys
(including non-enumerable ones like `length` on arrays). Always check
enumerability through `ec.get_own_property`.

### Wrapper objects — Boolean/Number/String/BigInt

When serializing, check for `[[BooleanData]]` / `[[NumberData]]` / etc.
internal slots (steps 7–10). When deserializing, create wrapper *objects*
with the correct prototype (steps 6–9), not primitive values — construct
through the realm's intrinsic constructor.

### Error cause — serialize custom data

The spec says "User agents should attach a serialized representation of any
interesting accompanying data." The `cause` property (ES2022) was added as
an optional `Box<SerializedRecord>` to the `Error` variant.
