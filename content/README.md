**Documenting your work:**
Follow these exact conventions so code <-> spec mapping is clear and reviewable.

- Web standards are present under `web_standards`, currently those are: Console, Dom, Fetch, and HTML. 

- Method- & type-level doc
  - Method-level: the method's top doc-comment must contain *only* the canonical spec anchor (e.g. `/// <https://webmachinelearning.github.io/webnn/#dom-ml-createcontext>`).
    - Do NOT add parenthetical notes or extra prose in top doc-comments (for example, `(internal helper)`) — these add noise and are disallowed. Keep top-level doc-comments anchor-only.
    If the algorithm implementation is broken-up into multiple method or functions, you can add a note below the anchor to explain which part of the algo the current code corresponds to.

  - Functions & spec-algorithms
    - Follow the structure of the spec. For example, if the spec defines an interface method and then from it calls into another algorithm, then you should also implement that algorithm either with a seperate method(if you need to access state of the dom struct), or just a function.
    - Naming & visibility: name the function to reflect the spec algorithm (`create_an_mloperand`), keep it private by default, and move it to a shared module only if genuinely reused across components.

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

    - Distinction: generated trait *methods/attributes* map to WebIDL anchors
      like `#dom-mlcontext-accelerated` (the attribute getter); struct fields
      that back *internal slots* should link to the internal-slot anchor where
      available, otherwise link to the attribute/getter or interface anchor.
    - Do not add additional prose when documenting internal-slot fields.

    - DOM struct fields must remain private. Always add `pub(crate)` accessor
      methods (getters/setters) on the `#[dom_struct]` type for other code to
      read or modify internal-slot values. Consumers outside the defining
      module must call these accessors — do *not* access struct fields
      directly from other modules.

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