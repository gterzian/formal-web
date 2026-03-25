When learning anything new about project, update this file with the lesson.

When the user gives general project or documentation instructions, record them here proactively even
if they may later be split into more modular instruction files.

- Lean library modules live under `FormalWeb/` and are re-exported from `FormalWeb.lean`.
- Use CamelCase filenames such as `FormalWeb/UserAgent.lean` for modules like `FormalWeb.UserAgent`.
- Keep structure docstrings tied to the spec only when the type itself has a direct spec concept; put spec links on individual structure fields when the spec concept belongs to the member.
- Prefer single-line doc comments for spec-only documentation such as `/-- https://html.spec.whatwg.org/multipage/#anchor -/`.
- Keep docs minimal: default to just the spec link when a precise spec anchor exists, and add prose only when it carries real modeling information.
- For spec algorithms, document the Lean function with the spec link and annotate the body with `Step n: ...` comments using verbatim spec prose.
- For partially implemented algorithm steps in Lean, put a `-- TODO:` comment immediately below the corresponding step comment.
- When a spec algorithm calls another algorithm, model that callee as a separate Lean function.
- Prefer pure state-transition signatures that thread `UserAgent` and return any produced values, so the model can evolve toward a labeled transition system.
- Lean uses `-- TODO:` comments, not `// TODO:` comments.