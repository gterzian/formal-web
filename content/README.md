# content crate

The content crate owns the content process: DOM and HTML algorithms, document parsing and lifecycle, Boa integration, Streams and Web IDL bridges, and the typed IPC boundary back to the embedder and user agent.

## Layout

- `content/src/main.rs` and the root modules resume embedder-driven HTML algorithms and content IPC entry points.
- `content/src/dom` holds native DOM carriers and DOM Standard algorithm implementations.
- `content/src/html` holds parser, document lifecycle, navigation helpers, and HTML global-object carriers.
- `content/src/boa` holds Boa integration, wrapper identity, and JavaScript dispatch glue.
- `content/src/webidl` holds shared Web IDL callback and promise algorithms.
- `content/src/streams` holds native stream carriers and Streams Standard algorithms.
- `content/src/infra` holds shared Infra Standard helpers.

## Spec Documentation

- Keep the top doc comment anchor-only, for example:
  - `/// <https://dom.spec.whatwg.org/#concept-event-dispatch>`
  - `/// <https://html.spec.whatwg.org/#global-object>`
  - `/// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>`
  - `/// <https://streams.spec.whatwg.org/#writablestream-state>`
- Inside function bodies, map relevant code with verbatim `Step N:` comments.
- Use `Note:` comments only for representation or mapping details that are not obvious from the spec text.
- Put unimplemented work in `TODO:` directly below the related `Step N:` comment.
- Keep `content/src/boa/bindings` thin: argument conversion and downcasting live there, while stateful algorithms live on the owning carrier type.