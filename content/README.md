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
- `content/src/js` holds generic JS engine integration (via the `js_engine` trait), wrapper identity, and JavaScript dispatch glue.
- `content/src/webidl` holds shared Web IDL callback and promise algorithms (implements Web IDL §3 JavaScript binding).
- `content/src/streams` holds native Streams [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) and Streams Standard algorithms.
- `content/src/infra` holds shared Infra Standard helpers.

## Three-layer architecture

Every Web-exposed feature follows a three-layer split (domain → Web IDL infra →
JS bindings glue).  See `content/src/js/bindings/README.md` for the definitive
description with examples and common mistakes.

## Spec Documentation

- **Anchor-only doc comments.** Every function, struct, associated constant,
  and constant definition has **only** the spec anchor URL in its doc comment.
  Zero prose — not a single explanatory sentence.  Examples:
  - `/// <https://dom.spec.whatwg.org/#concept-event-dispatch>`
  - `/// <https://html.spec.whatwg.org/#global-object>`
  - `/// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>`
  - `/// <https://streams.spec.whatwg.org/#writablestream-state>`
  - Any prose following the anchor is a violation.  The spec is the documentation; if the function name is not enough context, the algorithm lives in the spec.
  - The only exception is a `// Note:` on a separate line below the anchor,
    and only for genuine spec discrepancies (split-process, browser-engine
    refactoring).  Such notes must be fewer than ten across the codebase.
- Inside function bodies, map relevant code with verbatim `Step N:` comments.
- Use `Note:` comments inside the function body for representation or mapping details that are not obvious from the spec text.  Design notes and architecture rationales belong in the README chain.
- Put unimplemented work in `TODO:` directly below the related `Step N:` comment.
- `WebIdlInterface` implementations live in `content/src/js/bindings/` — these define *which members* an interface exposes.  Domain methods on the corresponding Rust struct (in `content/src/<domain>/`) implement *what those members do*.