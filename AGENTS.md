When learning anything new about project, update this file with the lesson.

When the user gives general project or documentation instructions, record them here proactively even
if they may later be split into more modular instruction files.

This file is for durable project-wide lessons and guidance only, not task-by-task work logs or progress notes.

- Lean library modules live under `FormalWeb/` and are re-exported from `FormalWeb.lean`.
- Use CamelCase filenames such as `FormalWeb/UserAgent.lean` for modules like `FormalWeb.UserAgent`.
- Keep structure docstrings tied to the spec only when the type itself has a direct spec concept; put spec links on individual structure fields when the spec concept belongs to the member.
- Prefer single-line doc comments for spec-only documentation such as `/-- https://html.spec.whatwg.org/multipage/#anchor -/`.
- Keep docs minimal: default to just the spec link when a precise spec anchor exists, and add prose only when it carries real modeling information.
- Document structure fields with spec-slot links wherever the standard exposes a corresponding slot; if a field is model-local, say so explicitly and link the closest relevant spec concept.
- For spec algorithms, document the Lean function with the spec link and annotate the body with `Step n: ...` comments using verbatim spec prose.
- Model shared spec algorithms at the least-specific spec type that the standard uses; for example, `initialize-the-navigable` should take a `Navigable`, not a `TraversableNavigable`, since the spec reuses it for child navigables.
- For partially implemented algorithm steps in Lean, put a `-- TODO:` comment immediately below the corresponding step comment.
- When a spec algorithm calls another algorithm, model that callee as a separate Lean function.
- Prefer pure state-transition signatures that thread `UserAgent` and return any produced values, so the model can evolve toward a labeled transition system.
- Model navigable parents as navigables, not traversables; when a type represents a top-level traversable, encode the null-parent invariant in the type when practical.
- The LTS should sit above helper functions: helper calls are implementation detail inside larger concurrent transitions, not necessarily one LTS step each.
- Near-term focus is modeling data and spec algorithms first; the LTS layer comes after those foundations exist.
- Long-term, there should be both an LTS model and an executable task/channel-based implementation, with proofs that the implementation refines or simulates the LTS.
- Message kinds used by the runtime are good candidates for inductive action/message types shared with or related to the LTS labels.
- Lean uses `-- TODO:` comments, not `// TODO:` comments.
- The author is not a Lean expert, so make an effort to translate user instruction freely into equivalent idiomatic Lean constructs. 
- Web standards are in a local-only folder name web_standards. Search these files to document your work as noted above.
- When searching the local HTML standard, search for the exact spec anchor string first, such as `creating-a-new-top-level-traversable`.
- For concurrent spec algorithms such as fetch-and-wait navigation steps, prefer modeling the pause point as explicit pending state on `UserAgent` plus a separate resume transition before introducing real runtime tasks or I/O.
- Introduce a small LTS action type above navigation helpers so spec-visible concurrent steps such as "begin navigation" and "fetch response arrives" are explicit labels, while helper functions remain implementation detail under those labels.
- It is acceptable for the LTS layer to factor a convenience spec helper into multiple explicit labels, such as separating top-level traversable creation from the later begin-navigation and fetch-completion steps.
- If a spec convenience helper only bundles multiple LTS-visible steps, prefer modeling those steps directly in the action system and omit the convenience helper unless it still carries independent explanatory value.
- If a spec algorithm is only a top-level entry point and is not referenced by other spec algorithms, it does not need to be preserved as a separate helper in the model when the LTS already captures its intended behavior.