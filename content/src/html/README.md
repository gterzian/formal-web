# content/src/html

`content/src/html` owns HTML parser integration, document lifecycle work, navigation helpers, and HTML global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) such as `Window` and `GlobalScope`.

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

## Algorithm split: content process vs user agent

Many HTML algorithms (navigation, window.open, iframe creation) span both the
content process (which runs JS and owns DOM state) and the user agent (which
owns the navigable tree, browsing contexts, and event-loop dispatch). The
split is:

| Side | Owns | Runs |
|------|------|------|
| **Content** | Document, Window, JS `Context`, `GlobalScope` | Document-owning algorithm steps: URL parsing, feature tokenization, noopener computation, rules-for-choosing-a-navigable (local subset), document creation |
| **User agent** | Navigable tree, browsing contexts, browsing context groups, agents, event loops, session history | Navigable-owning algorithm steps: find-by-target-name (cross-process), new-traversable creation (non-window.open), opener tracking, beforeunload, navigation fetching |

When an algorithm crosses this boundary, the side that hits its limit sends an
IPC message and the other side continues. The IPC ordering guarantee (per
content process, messages arrive in order) makes this safe.

### Document creation: two directions

Documents can be created either by the user agent (for startup, iframes, UA-originated
`_blank` navigations) or by content (for `window.open`). These are inverses:

**UA→Content** (`create_new_top_level_traversable` in `user_agent/src/user_agent.rs`):
1. UA allocates IDs (traversable, document, browsing context, agent)
2. UA sends `CreateEmptyDocument` IPC to content's event loop
3. Content creates the about:blank document, Window, and JS Context
4. UA registers the navigable in its state

**Content→UA** (`window_open_steps` in `window.rs`):
1. Content creates the about:blank document, Window, and JS Context locally
2. Content sends `NavigateRequest` with `new_traversable_info`
3. UA calls `create_new_top_level_traversable_from_content` (UA-side inverse of step 1)
4. UA registers the navigable, browsing context, agent, event loop WITHOUT
   sending `CreateEmptyDocument` back (content already did it)

Both paths converge to the same final state.

## The rules for choosing a navigable (`the_rules_for_choosing_a_navigable`)

Implements <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>.
Split between content and user agent:

### Content side (`html.rs:the_rules_for_choosing_a_navigable`)
| Step | What content does |
|------|-------------------|
| 1 | Let chosen = null |
| 3 | `_self` / empty → currentNavigable (Resolved) |
| 4 | `_parent` → parent (or current) (Resolved) |
| 5 | `_top` → traversable (Resolved) |
| 6 | Named target, not `_blank`, not noopener → cross-process lookup needed (NeedsUserAgentAction) |
| 7 | Otherwise → new top-level traversable (NeedsUserAgentAction) |

### User agent side (`user_agent.rs:the_rules_for_choosing_a_navigable`)
Continues when the content process returned `NeedsUserAgentAction`:
| Step | What UA does |
|------|-------------|
| 7 cont. | `find_navigable_by_target_name` across the global navigable registry |
| 8 | If still null: `create_new_top_level_traversable` (UA→Content path) |

## Window.open (`window_open_steps`)

Implements <https://html.spec.whatwg.org/#window-open-steps>.

### Steps 1–12 (content only)
URL parsing, target normalization, feature tokenization, noopener/referrerPolicy
computation. All local to the source document.

### Step 13 — apply the rules for choosing a navigable
Content runs `the_rules_for_choosing_a_navigable` (local subset) to resolve `_self`, `_parent`,
`_top`. For `_blank`, named targets, and noopener, it returns `NeedsUserAgentAction`.

### Step 14 — handle the chosen navigable
- **Resolved(id) where id == source:** Same-navigable. Return current window proxy.
- **Resolved(id) where id != source:** `_parent`/`_top`. Send `chosen_navigable_id`
  in the `NavigateRequest`. The UA navigates the correct navigable. The returned
  WindowProxy is the current global (wrong if parent/top is a different navigable —
  needs IPC resolution, tracked as a gap).
- **NeedsUserAgentAction:** Create an about:blank document locally via
  `GlobalScope::create_document`. This gives us a Window to back the WindowProxy
  immediately. Send `NavigateRequest` with `new_traversable_info`.

### Steps 15–17 (UA side)
- UA calls `create_new_top_level_traversable_from_content` to sync navigable state
- UA calls `setup_opener_for_window_open` for new-auxiliary tracking
- UA creates webview for the new top-level traversable
- UA starts navigation (fetch the destination URL)
- noopener → return null

### Step 18 — return WindowProxy
Return the target navigable's active Window's JsObject. For same-origin the
WindowProxy is transparent.

### Document creation for new traversables (the inverted split)

```
Content (window_open_steps):             UA (handle_navigate):
  |                                        |
  |-- create about:blank document          |
  |   (GlobalScope::create_document)        |
  |-- NavigateRequest {                    |
  |     new_traversable_info: Some(...),   |
  |     chosen_navigable_id: Some(id)      |
  |   }                                    |
  |                                        |
  |========================= IPC =========>|
  |                                        |
  |                                        |-- create_new_top_level_traversable_from_content
  |                                        |     (navigable, BCG, agent, 
  |                                        |      doc state, event-loop reg)
  |                                        |-- setup_opener_for_window_open
  |                                        |-- create_webview_for_new_top_level
  |                                        |-- handle navigation (fetch URL)
```

`GlobalScope::create_document` creates the about:blank document, JS Context, and
Window directly on the GlobalScope (no callback indirection). The method returns
the Window's global object which backs the WindowProxy.

The UA's `create_new_top_level_traversable_from_content` is the inverse of
`create_new_top_level_traversable`: it sets up only UA-side state (navigable,
browsing context group, agent, event-loop registration) and does NOT send
`CreateEmptyDocument` back to content.

### Opener tracking for auxiliary browsing contexts

<https://html.spec.whatwg.org/#creating-a-new-auxiliary-browsing-context>

When `window.open` creates a new navigable and noopener is false, the UA sets
up the opener relationship via `setup_opener_for_window_open`. This corresponds
to the spec's "create a new auxiliary browsing context" which:
1. Creates a new top-level traversable with the source navigable's browsing
   context as opener
2. Sets the opener browsing context on the new browsing context

The content process does not track opener relationships — those are purely
UA-side state. The opener is only used for:
- Navigation policy (e.g., `target=_blank` with `rel=opener`)
- `window.opener` JS property (not yet implemented)
- Popup blocking

## WindowProxy (`windowproxy.rs`)

<https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

`WindowProxy` is an ECMAScript Proxy exotic object (created via
`JsProxyBuilder`) wrapping the active Window.  The proxy uses native-function
traps for all 10 overridden internal methods specified by HTML §7.2.3.

### Current implementation (`JsProxyBuilder` + native-function traps)

The WindowProxy is implemented using `JsProxyBuilder` from
`boa_engine::object::builtins`, which is Boa's public API for creating Proxy
objects with native Rust trap functions.  Each of the 10 overridden internal
methods (`[[GetPrototypeOf]]`, `[[SetPrototypeOf]]`, `[[IsExtensible]]`,
`[[PreventExtensions]]`, `[[GetOwnProperty]]`, `[[DefineOwnProperty]]`,
`[[Get]]`, `[[Set]]`, `[[Delete]]`, `[[OwnPropertyKeys]]`) is a plain
`NativeFunctionPointer` — no captures, no custom handler struct, no access
to `pub(crate)` Boa internals.

For the same-origin fast path (always active in the current single-origin
content process):
- `[[GetOwnProperty]]` delegates to `OrdinaryGetOwnProperty(W, P)` on the
  inner Window object, so Window own properties are correctly visible.
- `[[DefineOwnProperty]]`, `[[Delete]]`, and `[[Set]]` delegate to the
  corresponding operations on the Window via public `JsObject` methods.
- `[[Get]]` delegates to `JsObject::get(key, context)` on the Window,
  covering both proxy own properties and the Window.prototype prototype chain.
- `[[OwnPropertyKeys]]` concatenates array-index keys (empty until child
  navigable tracking is added) with the Window's own property keys.
- `[[SetPrototypeOf]]` implements `SetImmutablePrototype`.

Each trap receives the proxy **target** (the Window) as `args[0]`, per the
ECMAScript Proxy internal method specification (10.5).  The target is obtained
from the trap arguments rather than from captures or custom handler fields.

Cross-origin paths (`CrossOriginGetOwnPropertyHelper`,
`CrossOriginPropertyFallback`, `CrossOriginGet`, `CrossOriginSet`,
`CrossOriginOwnPropertyKeys`) are structurally present as helper code but
unreachable because `is_platform_object_same_origin` is hardcoded to `true`.

### Remaining gaps

**1. Child navigable properties (array-index and named).**
The spec requires WindowProxy to expose child browsing contexts by numeric
index (`window[0]`, `window[1]`) and by name.  This requires tracking the
document-tree child navigables on the Document, which is not yet implemented.
The array-index branch in `[[GetOwnProperty]]` and `[[OwnPropertyKeys]]` is
stubbed (returns undefined / empty).

**2. `is_platform_object_same_origin` is hardcoded to `true`.**
The content process currently runs a single origin, so cross-origin access
does not arise during testing.  When multi-origin support is added, the
WindowProxy will silently leak all cross-origin properties instead of applying
the restricted CrossOriginProperties table (HTML §7.2.3).

**3. Navigation window swapping is untested and unused.**
The WindowProxy wraps a fixed Window; there is no mechanism to swap the
active Window behind the same proxy identity.  Cross-document navigation
does not update the proxy.

### Implementation notes

The WindowProxy uses `JsProxyBuilder` — Boa's public API for constructing
Proxy objects from native Rust function pointers.  This avoids any access to
`pub(crate)` internals (`Proxy::create`, `Proxy::try_data`, etc.) and works
with Boa as an external dependency from the `boa-dev/boa` repository.  See
`content/src/js/README.md` ("Working with Boa's public API: use spec links,
not `pub(crate)` internals") for the general methodology.

See also:
- `content/src/webidl/README.md` for the exotic-object pattern with JsData.

## Related documentation

- `content/src/webidl/README.md` — Boa platform object integration, exotic object pattern
- `content/src/js/README.md` — Boa integration specifics (Context ownership, bindings)
- `content/README.md` — Content-crate overview
- `user_agent/src/user_agent.rs` — `create_new_top_level_traversable_from_content`, `create_new_top_level_traversable`, `the_rules_for_choosing_a_navigable` (UA side), `setup_opener_for_window_open`
- `ipc_messages/src/content.rs` — `NewTraversableInfo`, `CreateEmptyDocument`, `NavigateRequest`
- `content/src/html.rs` — `the_rules_for_choosing_a_navigable` (content side), `navigate`, `ChosenNavigable`
- `content/src/html/window.rs` — `Window::open`, `window_open_steps`
- `content/src/html/global_scope.rs` — `create_document`, `set_navigable_hierarchy`
