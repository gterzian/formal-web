**Documenting your work:**
Follow these exact conventions so code <-> spec mapping is clear and reviewable.

- Web standards are present under the top-level directory `/web_standards/`, currently those are: Console, Dom, Fetch, and HTML. 

- Method- & type-level doc
  - Method-level: the method's top doc-comment must contain *only* the canonical spec anchor (e.g. `/// <https://webmachinelearning.github.io/webnn/#dom-ml-createcontext>`).
    - Do NOT add parenthetical notes or extra prose in top doc-comments (for example, `(internal helper)`) — these add noise and are disallowed. Keep top-level doc-comments anchor-only.
    If the algorithm implementation is broken-up into multiple method or functions, you can add a note below the anchor to explain which part of the algo the current code corresponds to.
    - Internal helpers that contain `Step N:` comments must also have an anchor-only top doc-comment for the algorithm they continue.

  - Functions & spec-algorithms
    - Follow the structure of the spec. For example, if the spec defines an interface method and then from it calls into another algorithm, then you should also implement that algorithm either with a seperate method(if you need to access state of the dom struct), or just a function.

- In-body per-line spec mapping
  - Inside the function body annotate *each relevant line of code* with a
    single comment of the exact form `Step N: <spec prose>` (use `Step 5.1`,
    `Step 5.2` for sub-steps). Avoid pasting entire algorithm blocks.
    Quote the spec step verbatim in the code comment.
  - If the spec step does not map 1:1 to code, add `// Note: ...` explaining
    the divergence and reference the spec anchor. If the spec's preliminary
    steps (for example `Step 1`/`Step 2` that establish `global`/`realm`) are
    implicit in Rust (e.g. via `self.global()`), still include `Step 1:` and
    `Step 2:` comments and follow them with a `// Note:` explaining the
    implicit mapping.
  - Do *not* use shorthand/aggregation comments such as `Steps 1-5: same precondition
    checks as the non-BYOB variant.` — every algorithm step referenced in the
    spec must appear explicitly (Step N) in the implementation, even when the
    code is identical to another overload. This makes reviewer-to-spec
    mapping unambiguous and prevents accidental divergence.  - Internal slots / struct members: document the field with a single-line
    doc-comment that contains *only* the canonical spec anchor in angle
    brackets. Prefer an *internal-slot* anchor when the spec provides one
    (e.g. `#dom-foo-xyz-slot`). If no `-slot` anchor exists, link the field
    to the attribute getter or the interface/internal-slots section that
    documents the internal slot (for `MLContext.[[accelerated]]` prefer
    `#dom-mlcontext-accelerated-slot`; fall back to `#dom-mlcontext-accelerated`
    only when a `-slot` anchor is not present). Example:

      ```rust
      /// <https://webmachinelearning.github.io/webnn/#dom-mlcontext-accelerated-slot>
      accelerated: Cell<bool>,
      ```

    - Do not add additional prose when documenting internal-slot fields, unless necessary to explain something that doesn't map directly to a spec concept. For example, you can document that a member represents an interface implementation but implemented using composition.

    - When a stored value comes from a WebIDL dictionary (for example
      `MLTensorDescriptor`), link the field to the specific dictionary-member
      anchor (for example `#api-mltensordescriptor` / `{{MLTensorDescriptor/readable}}`) so
      the source of truth is obvious.

- TODOs and in-parallel steps
  - For any unimplemented spec step, add a `TODO: {optional short description}`
    immediately below the usual `Step N:` comment. 
  - IMPORTANT: if the TODO corresponds to an *in-parallel* step that would
    resolve a Promise, do *not* resolve the Promise in the stub — return
    the Promise unresolved and leave resolution to the future queued task.

  - Assertions & invariants
    - Do **not** use `panic!` for runtime checks in `components/script` code. Use
      `debug_assert!` for internal invariants that should only fire during
      development (for example `debug_assert!(false, "unexpected state")`).
    - If an invariant can be reached in release builds, return a `Result`/`Error`
      or provide a safe fallback rather than panicking. Library code should
      never abort the process in production.
    - When a helper function implements a spec algorithm and an impossible
      branch is present, prefer `debug_assert!` + a safe release fallback (see
      `mlgraphbuilder::create_an_mloperand` for an example).

- Formatting rules
  - Always leave a blank line after a `Step + code` or `Step + TODO` block.
    (Exception: you do not need to add an extra blank line before the method's
    closing brace solely to satisfy this rule.)
  - Keep comments short and place them on their own line above the code they
    document.

**JavaScript runtime**

- `content/src/main.rs` and sibling root modules such as `content/src/html.rs` own the HTML Standard entry points that resume embedder-driven algorithms, create documents, and trigger HTML-defined load/rendering steps.

- `DispatchEvent` runtime commands may carry a retained batch of serialized UI events rather than a single raw input; `content/src/main.rs` should dispatch that batch in order instead of assuming a one-command-per-event bridge.

- `content/src/html` owns HTML parsing, hyperlink-following helpers, document loading entry points, parser-script collection, and HTML global-object carriers such as `GlobalScope` and `Window`, while `content/src/boa` owns microtask checkpoints and the bridge from Blitz UI events into JavaScript event dispatch.

- `content/src/dom` stores the native data carried by JavaScript-visible `Window`, `Node`, `Document`, `Element`, `EventTarget`, `Event`, and `UIEvent` objects. `BaseDocument` remains the authoritative DOM state; the JavaScript wrappers do not store shadow DOM data.

- When `content` disables Blitz default features, keep `blitz-dom/system_fonts` enabled. Without that feature the DOM still mutates and resolves, but Parley shapes zero glyph runs and HTML text paints as empty layouts.

- `content/src/webidl` owns Web IDL algorithms such as callback-interface conversion and `call a user object's operation`, so DOM dispatch can invoke listeners without reaching into Boa primitives directly.

- Run microtask checkpoints at task boundaries such as completed script evaluation, timer execution, and UI event dispatch instead of immediately after every Rust-to-JavaScript callback return; callback-driven stream algorithms rely on the surrounding synchronous specification step finishing before queued promise reactions run.

- Never call into JavaScript while holding a mutable `BaseDocument` borrow or guard that JavaScript bindings could try to re-borrow. Pass a document wrapper into Blitz and let it take short-lived borrows around its own native phases.

- If `update the rendering` is noted while a document still has pending critical resources, keep that rendering opportunity pending and resume it from the corresponding fetch completion instead of painting a stale frame.

- When a JavaScript-visible Web IDL attribute or algorithm is implemented for a DOM type, keep the spec-linked method on the corresponding `content/src/dom` type and have `content/src/boa/bindings` delegate to that method instead of embedding the algorithm in the binding layer.

- When a carrier-side helper owns a named HTML algorithm or a specific suffix of its steps, prefer the spec algorithm name for the helper when practical and note exactly which steps that helper continues instead of repeating a bare anchor.

- For HTML mixins such as `HyperlinkElementUtils`, keep the carrier-side algorithms on a shared trait and register the shared Web IDL surface from a binding helper instead of duplicating mixin methods on each concrete element binding.

- When one spec section invokes a concept algorithm defined elsewhere, link the shared trait/helper to the concept anchor and link the concrete element implementation to the invoking section's reference anchor (for example `api-for-a-and-area-elements:*` for anchor-specific uses of hyperlink URL algorithms).

- When a content runtime type models an HTML execution concept, document it against the corresponding HTML concept anchor such as `#environment`, `#environment-settings-object`, or `#global-object` instead of folding that state into the nearest exposed interface name.

- Keep the Boa host state on `EnvironmentSettingsObject`, and keep per-global caches such as the `Document` wrapper identity, node wrapper identity, and animation frame callback state on `GlobalScope`.

- Keep binding-related tooling in `content` only when it is part of the maintained workflow. Remove inactive generators, generated outputs, and orphaned Web IDL inputs instead of leaving them in the tree.

- Initial document parsing collects classic scripts after the tree build, starts external script fetches as they are discovered, and runs the queued classic scripts in document order only after the document's deferred-load continuation observes that scripts and critical resources are ready or have timed out. `innerHTML` parsing uses the same sink with scripting disabled so fragment parsing does not execute scripts.

- Parser-script collection must match classic scripts by normalized `type` essence and skip non-classic data blocks such as `application/json`, `speculationrules`, and module scripts.

- Register a newly created content document before running parser-discovered scripts or firing `load` so later dispatch and rendering commands do not lose the document id when a page script throws.

- For content-side work, keep the default WPT runner configuration scoped to the suites under active development by editing `tests/wpt/include.ini` and `tests/formal/include.ini`, record known expected failures under `tests/wpt/meta` with `disabled:` reasons that name the missing feature or blocking bug, and finish by running `cargo run -- test-wpt` with no path so the default selection reports zero unexpected results. When enabling streams coverage, leave readable byte stream and BYOB coverage disabled until those controllers and readers are implemented.

# Boa GC — Field Ownership Cheat Sheet

## The Basic Rule

Derive `Trace` and `Finalize` on every struct that lives inside a JS object. That's it for most cases.

```rust
#[derive(Trace, Finalize)]
struct Animal {
    name: String,       // plain Rust type — trace is a no-op
    age: u32,
    callback: JsValue,  // GC-managed — traced automatically via derive
}
```

---

## Choosing the Right Wrapper

| Need | Use |
|---|---|
| Immutable, single owner | `T` |
| Mutable, single owner | `GcRefCell<T>` |
| Immutable, shared | `Gc<T>` |
| Mutable, shared | `Gc<GcRefCell<T>>` |

Same mental model as plain Rust: `T` / `RefCell<T>` / `Rc<T>` / `Rc<RefCell<T>>` — just with GC-aware versions.

---

## No Reflector Needed

Unlike SpiderMonkey-based bindings, Boa does **not** require a back-reference to the JS wrapper stored on your struct. The JS object owns your Rust data — not the other way around.

---

## `#[unsafe_ignore_trace]`

If a field can't implement `Trace` (e.g. a raw pointer or third-party type), opt it out:

```rust
#[derive(Trace, Finalize)]
struct Foo {
    #[unsafe_ignore_trace]
    ptr: *mut SomeExternalThing, // GC won't trace this — fine for non-GC data
}
```

---

## `Gc<T>` — Only When Needed

Reach for `Gc<T>` only when:
- Multiple GC-tracked objects share ownership of the same data
- You have (or might have) reference cycles — `Rc` would leak, `Gc` won't

For typical `Class` implementations with no sharing, you'll never need it.

---

## Common Mistakes to Avoid

### Don't derive `Trace`/`Finalize` unnecessarily
Only derive them if the struct contains GC-managed fields (`JsValue`, `JsObject`, `Gc<T>`, etc.) or is itself stored inside a GC-managed object. A plain Rust struct with no GC fields doesn't need them.

### Don't wrap things in `Gc<GcRefCell<T>>` unless they are actually shared
`Gc<T>` is like `Rc<T>` — only reach for it when multiple structs need to point at the same allocation. If you just need owned mutable data, `GcRefCell<T>` alone is sufficient.

### Don't add trivial helper methods for single field access
If a method just does `self.some_cell.borrow_mut() = value`, delete the method and use the `GcRefCell` directly at the call site. The indirection adds noise without value.

### Don't store a reflector back-reference on your struct
A type like `WritableStream` doesn't need to hold a reference to its own JS wrapper object. It will either be owned by a `JsValue`, or by another struct that derives `Trace` — the GC handles reachability without any back-pointer.

### `data_constructor`'s `this` is not the new instance
The `this` passed into `data_constructor` is the **constructor function object**, not the newly created JS object. Don't use it to set up the instance. Just return `Self` — Boa takes care of wiring the returned Rust data to the new JS object. Helpers like `construct_writable_stream` that try to manually do this are unnecessary in the normal `Class` flow.

---

## `GcRefCell` Footgun: Re-entrancy

If you hold a `GcRefCell` borrow and then call back into JS, a re-entrant access to the same cell will **panic**. Always clone out first:

```rust
// WRONG — borrow held across JS call
let handler = self.callback.borrow();
context.call(&handler, ...)?; // potential re-entrant borrow_mut → PANIC

// RIGHT — clone first, then drop borrow
let handler = self.callback.borrow().clone(); // JsValue clone is cheap
context.call(&handler, ...)?; // safe
```