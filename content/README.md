# content crate

The content crate owns the content process: DOM and HTML algorithms, document
parsing and lifecycle, generic JavaScript engine integration via the
`js_engine` trait, Streams and Web IDL bridges, and the typed IPC boundary
back to the embedder and user agent.

## Design philosophy

Content code follows the same call chains the web standards define.  When a
spec algorithm calls Web IDL (e.g. type conversion, promise manipulation),
content code routes through `content/src/webidl/`.  When a spec algorithm
calls ECMA-262 directly (e.g. realm creation, script evaluation), content
code calls the `js_engine` trait directly.  No Boa-specific APIs appear
above `js_engine/src/boa/`.  See `js_engine/README.md` for the full
design philosophy and `content/src/generic_js_test.rs` for validated
patterns.

## Layout

- `content/src/main.rs` and the root modules resume embedder-driven HTML algorithms and content IPC entry points.
- `content/src/dom` holds native DOM [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) and DOM Standard algorithm implementations.
- `content/src/html` holds parser, document lifecycle, navigation helpers, and HTML global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object).
- `content/src/js` holds the content crate's JS integration layer: type aliases pointing to the concrete `js_engine` backend, generic platform-object resolution and downcast helpers, and JavaScript dispatch glue. The `js_engine` trait itself lives in the top-level `js_engine/` crate (see its `README.md`).
- `content/src/webidl` holds shared Web IDL callback and promise algorithms (implements Web IDL §3 JavaScript binding).
- `content/src/streams` holds native Streams [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) and Streams Standard algorithms.
- `content/src/infra` holds shared Infra Standard helpers.

## Three-layer architecture

Every Web-exposed feature follows a three-layer split (domain → Web IDL infra →
JS bindings glue).  See `content/src/js/bindings/README.md` for the definitive
description with examples and common mistakes.

## Spec Documentation

### Anchor-only doc comments

Every function, struct, associated constant, and constant definition has
**only** the spec anchor URL in its doc comment. Zero prose — not a single
explanatory sentence.

```rust
/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_event(ec, path, event) { … }
```

- Any prose following the anchor is a violation.  The spec IS the documentation.
- The only exception is a `// Note:` on a separate line below the anchor,
  and only for genuine spec discrepancies (split-process, browser-engine
  refactoring).  Such notes must be fewer than ten across the codebase.

### Step comments inside function bodies

Every spec algorithm step has a `// Step N:` comment quoting the **exact spec
step text verbatim** — not an abbreviation or summary.  Step numbering must
match the spec exactly.

```rust
// Step 1: If event's dispatch flag is set, or if its initialized flag is not set,
//         then throw an "InvalidStateError" DOMException.
if *event.dispatch_flag.borrow() || !*event.initialized_flag.borrow() {
    return Err(ec.new_type_error("…"));
}

// Step 3: Return the result of dispatching event to this.

crate::dom::dispatch_event(ec, path, event)
```

- Always insert a blank line between the last `// Step N:` comment and the
  following Rust code.
- Use `// TODO: Not yet implemented.` for spec steps that are not yet
  implemented — every step must be accounted for.
- For sub-algorithms called by the spec, cross-reference with the anchor URL
  in a comment (e.g. `// <https://dom.spec.whatwg.org/#concept-event-dispatch>`).

### Function naming and algorithm structure

- Name functions after the spec algorithm they implement (e.g. `flatten_more`
  for "flatten more options", `convert_js_to_dictionary` for "convert a JavaScript
  value to dictionary").
- If you must split a spec algorithm into multiple internal helpers, provide
  a single public function with the spec's name and explain the split with a
  `// Note:`.
- When a function partially implements a spec algorithm, annotate with `// Step N:`
  for ALL steps of the algorithm. Mark missing steps with `// TODO: Not yet
  implemented.` See `html/dispatch.rs::fire_global_event` for the correct pattern.

### `// Note:` for discrepancies only

`// Note:` is for discrepancies between the code and the spec text (e.g. steps
merged across processes, browser-engine refactoring). Design notes, architecture
rationales, and implementation plans belong in the README chain, not in Notes.

### Full reference

See `content/src/js/bindings/README.md` for the complete Common Mistakes table
covering all annotation patterns, three-layer architecture rules, and the
correct treatment of infrastructure code vs spec algorithms.