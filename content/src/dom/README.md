# content/src/dom

`content/src/dom` stores the native [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) for the JavaScript-visible DOM interfaces and the DOM Standard algorithms that operate on them.

- `BaseDocument` remains the authoritative DOM tree and document state.
- `Document` and `Element` compose `Node`, so shared tree algorithms live on `Node` while type-specific Web IDL behavior stays on the owning [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object).
- HTML-owned global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) such as `GlobalScope` (implementing the [global object](https://html.spec.whatwg.org/#global-object) concept) and [Window](https://html.spec.whatwg.org/#window) live in `content/src/html`, and DOM dispatch code here depends on them when the DOM Standard talks about window-backed targets.
- `content/src/js/bindings` should delegate DOM algorithms here instead of embedding DOM logic in the binding layer.
- Native UI-event to DOM-dispatch bridging belongs here, with activation-target selection kept in `dispatch.rs`.
- Use the `web_standards` extension (`spec_lookup`) with `https://dom.spec.whatwg.org/` to read the DOM spec, and for single-sentence spec definitions quote the defining sentence instead of inventing `Step N:` comments.

## Event dispatch architecture

The dispatch algorithm operates on two domain types: `Event` and `EventTarget` — both `#[gc_struct]` platform objects.  The JsObject GC handle is only used at the Web IDL boundary (`call_user_objects_operation`).

### EventTargetAccess trait

Types that embed an `EventTarget` (Node, Window, AbortSignal, Element, etc.) implement `EventTargetAccess`:

```rust
pub(crate) trait EventTargetAccess {
    fn get_event_target(&self) -> EventTarget;            // clone of embedded EventTarget
    fn get_target_object(&self) -> Option<JsObject>;      // JsObject GC handle (None for standalone clones)
    fn get_the_parent(&self) -> Option<(JsObject, EventTarget)>;  // parent for path building
}
```

Dispatch functions (`fire_event`, `dispatch`, `dispatch_with_chain`, `dispatch_window_event`) take `target: &dyn EventTargetAccess` and `target_object: &JsObject`.  The JsObject is passed by the caller (who has it from the JS bindings layer) and stored in path entries for Web IDL callback invocation.

### Interior mutability via GcCell

All mutable fields on `Event` and `EventTarget` use `GcCell` instead of `&mut self` methods:

```rust
pub struct EventTarget {
    pub(crate) event_listener_list: GcCell<Vec<EventListener>>,
    next_listener_id: Cell<u64>,  // Cell (not GcCell) since u64 has no GC pointers
}
```

Methods take `&self` and use `borrow()`/`borrow_mut()`.  Since `GcCell` shares its underlying data through a `Gc` pointer, cloning an EventTarget and mutating the clone affects the original — no sync-back needed.

### Path entries separate domain + GC

`EventPathEntry` stores both the domain `EventTarget` (for algorithm operations) and the `JsObject` GC handle (for Web IDL callback invocation).  The `shadow_adjusted_target` (also `JsObject`) is the spec's shadow-adjusted target for setting `event.target`.

### Entry points

| Function | Takes | Creates path? | Used by |
|---|---|---|---|
| `fire_event` | `&dyn EventTargetAccess`, `&JsObject`, event_type, time, flags | Yes | Domain callers (main.rs, html_iframe_element, abort) |
| `dispatch` | `&dyn EventTargetAccess`, `&JsObject`, event_object, flags | Yes | JS bindings (dispatchEvent) |
| `dispatch_with_chain` | `ec`, `&[usize]`, event_object | Yes (from node chain) | BlitzEventDriver (UI events) |
| `dispatch_window_event` | `ec`, event_type, cancelable, time | No (simple_path) | beforeunload |

### What NOT to do

- Do not add `target_object` as a parameter — it's part of the EventTargetAccess trait.
- Do not use `&mut self` on EventTarget methods — use `GcCell` fields with `&self`.
- Do not clone Event or EventTarget and sync back — `GcCell` shares data across clones.
- Do not put JsObject-only helpers (like `resolve_event_target`) in dispatch.rs — keep domain dispatch pure.

## GcCell interior mutability pattern elsewhere

Types that currently use `&mut self` for mutation but could use `GcCell` + `&self`:

- **Node** — `child_node_ids` is read-only; other mutating methods could use GcCell.
- **Window** — `onload`, `setTimeout` callbacks stored behind GcCell.
- **Streams** — writable/readable stream state machines use `Cell<bool>`/`RefCell`; some could use GcCell for GC-traced callback fields.
- **AbortSignal** — already uses `GcCell<AbortSignalState>` at the top level; internal fields like `onabort` could move to GcCell if needed.