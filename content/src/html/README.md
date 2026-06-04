# content/src/html

`content/src/html` owns HTML parser integration, document lifecycle work, navigation helpers, and HTML global-object carriers such as `Window` and `GlobalScope`.

- Keep DOM-tree entry points under `content/src/html/html_dom_tree.rs`, and route per-element hooks from there into element modules.
- Keep iframe bindings and iframe processing algorithms together in `content/src/html/html_iframe_element.rs` as free functions over content-process state (`ContentProcess`).
- Keep helper names aligned with the corresponding HTML algorithm anchors, and prefer explicit error returns or `debug_assert!` plus safe early returns over sentinel ids.
- Trigger parser-discovered iframe work from document-load parsing completion.
- Use the `web_standards` extension (`spec_lookup`) with `https://html.spec.whatwg.org/` to read the HTML spec.

## Structured clone (`safe_passing_of_structured_data.rs`)

### String round-tripping — use `Vec<u16>`, never `to_std_string_escaped()`

Boa's `JsString::to_std_string_escaped()` is a **display-only** method that
replaces unpaired surrogates with literal `\uXXXX` escape sequences. Using it
for serialization corrupts strings like lone surrogates (`\uD800`, `\uDC00`).

**Correct serialization:**
```rust
let utf16_units: Vec<u16> = js_string.as_str().to_vec();  // serializable
```

**Correct deserialization:**
```rust
let js_string = JsString::from(&utf16_units[..]);
```

### RegExp source — `[[OriginalSource]]` vs the escaped getter

The `source` accessor on RegExp applies `EscapeRegExpPattern` (spec 22.2.3.2.5),
which escapes `/`, `\n`, `\r`, `\u2028`, and `\u2029`. Passing the escaped form
back to the RegExp constructor produces a different pattern. Always store the
raw `[[OriginalSource]]`. Since Boa's accessor is `pub(crate)`, reverse the
escaping with `unescape_regexp_source()`.

### Error "message" — `[[GetOwnProperty]]`, not `[[Get]]`

The spec step for Error serialization (step 17.4) uses `[[GetOwnProperty]]` for
the "message" property — this checks only own data descriptors, ignores the
prototype chain, and does not invoke accessors. Using `object.get("message")`
(which is `[[Get]]`) is wrong. Use the property map directly:
```rust
let desc = object.borrow().properties().get(&PropertyKey::from(js_string!("message")));
let message = match desc {
    Some(d) if d.is_data_descriptor() => {
        d.value().map(|v| v.to_string(context).map(|s| s.to_std_string_escaped())).transpose()?
    }
    _ => None,
};
```

### EnumerableOwnProperties — filter by enumerability

The spec uses `EnumerableOwnProperties(value, "key")`, which returns only
enumerable own property keys. `own_property_keys()` returns ALL own keys
(including non-enumerable ones like `length` on arrays). Always check
enumerability:
```rust
let desc = object.borrow().properties().get(&key);
let enumerable = desc.as_ref().and_then(|d| d.enumerable()).unwrap_or(false);
```

### Wrapper objects — Boolean/Number/String/BigInt

When serializing, check for `[[BooleanData]]` / `[[NumberData]]` / etc.
internal slots (steps 7–10). When deserializing, create wrapper *objects*
with the correct prototype (steps 6–9), not primitive values:
```rust
let prototype = context.intrinsics().constructors().boolean().prototype();
let bool_obj = JsObject::from_proto_and_data(prototype, *b).upcast();
```

### Error cause — serialize custom data

The spec says "User agents should attach a serialized representation of any
interesting accompanying data." The `cause` property (ES2022) was added as
an optional `Box<SerializedRecord>` to the `Error` variant.

## Content / User-Agent split for navigation algorithms

Many navigation-related algorithms in the HTML spec (e.g. "window open steps",
"follow the hyperlink", "rules for choosing a navigable") run on the event loop
in spec terms. In our architecture the content process is the event loop, but
creating navigables, managing the navigable registry, and spawning content
processes requires the user_agent, which runs on a separate thread.

When an algorithm step requires user-agent state:

1. Run as many spec steps as possible in content (URL parsing, feature
   tokenization, noopener computation, etc.) — these are the steps that
   only touch document-local state.
2. Send a typed IPC message (`WindowOpenRequested`, `NavigationRequested`,
   `CreateChildNavigable`, etc.) to the user agent with the accumulated
   context.
3. The user agent continues the remaining algorithm steps, including
   navigable creation, target name lookup, opener tracking, and actual
   navigation.

Because IPC message delivery is ordered per content process, step re-shuffling
across the boundary is safe: a content process cannot issue a later message
that overtakes an earlier one.

## Window.open (`Window::open`, `window_open_steps` in `window.rs`)

The `open()` method on `Window` runs the content-side prefix of the window
open steps (steps 1–12: URL parsing, target normalization, feature tokenization,
noopener/referrerPolicy computation). It then sends a `WindowOpenRequested`
IPC to the user agent, which continues with step 13 (rules for choosing a
navigable), step 15 (popup/is-auxiliary/opener setup), and steps 15.4/16.1
(navigation).

## WindowProxy (follow-up)

`window.open()` currently returns `null` as a placeholder. The spec requires
returning a `WindowProxy` exotic object (step 18) that:

- Has the same [[Prototype]] as the underlying Window.
- Delegates all internal methods ([[Get]], [[Set]], [[HasProperty]],
  [[Delete]], etc.) to the Window it currently targets.
- Can switch which Window it targets between navigations (the Window
  object gets replaced by a new one for the new document, but the
  WindowProxy reference stays the same).

### Approach

The WindowProxy can be implemented as a Boa `JsObject` that wraps a
reference to the current Window `JsObject`. The key infrastructure needed:

1. **WindowProxy struct** — A Rust type implementing `boa_engine::JsData`
   (like `Window` does) that stores a `JsObject` handle to the current
   Window. It is registered as a Boa `Class` with the same prototype chain
   as Window.

2. **Property delegation** — The WindowProxy class overrides `__get__`,
   `__set__`, `__has__`, etc. to forward all operations to the current
   Window. Boa's `Class` trait doesn't expose these hooks directly, so
   this requires either:

   a) **JavaScript Proxy** — Create a JS `Proxy` wrapping the Window with
      a handler that delegates all traps. The WindowProxy is then the
      Proxy object. Simple but adds a JS Proxy indirection.

   b) **Boa NativeObject hooks** — Add a mechanism to Boa's `Class` or
      `NativeObject` traits that allows custom [[Get]]/[[Set]] behavior.
      More work but no indirection.

3. **Window replacement on navigation** — When a cross-document navigation
   completes (`finalize a cross-document navigation`), the new Document
   creates a new Window. The user agent must update the WindowProxy's
   inner reference to point to the new Window. This requires a way for
   the content process to communicate "this WindowProxy now targets this
   new Window" — either by returning the WindowProxy handle alongside
   the new Window creation, or by having the WindowProxy resolve the
   current Window from the navigable each time.

4. **Global object** — The JavaScript global (`this` at the top level,
   `globalThis`) should be the WindowProxy, not the Window directly.
   The WindowProxy delegates to the underlying Window.

Since this depends on Boa's object model and the navigation finalization
pipeline, it is deferred to a dedicated follow-up.