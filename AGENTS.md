**README Documentation Chain (read in order)**
- `AGENTS.md` — top-level agent orientation and cross-cutting rules (this file).
- `<component>/README.md` — component-level guidance and the consolidation point for component-wide lessons (for example `components/script/README.md`).
- `<component>/<subdir>/README.md` — subsystem-level guidance.

**Agent pre-task checklist (MANDATORY)**
- Read and add to your working context the README chain *for the task* in this order: `AGENTS.md` → `<component>/README.md` → `<component>/<subdir>/README.md` .
- Follow the documented conventions in those READMEs (for example the `components/script/README.md` "Documenting your work" rules) when implementing and commenting code.

Example — working on `content`
- Read in order: `AGENTS.md` → `content/README.md`.
- Add content-specific lessons to `content/README.md`.
- If a lesson applies to the project as a whole, add a single short line to `AGENTS.md` (do not copy the full prose into the subsystem README).
- Do not write changelogs in those files, only long-lasting documentation.

**Guidance on adding documentation**
Whenever the user corrects your code, besides fixing the code, if there
is a general lesson to document, add prose to the lowest-level possible `{README, AGENTS}.md` file. 

Principle: add lessons to the *lowest* README that makes sense. Do **not** duplicate or copy the same prose across multiple README files — put the lesson where it belongs.

**Prose & README style:**
- Document the *current* design/state only — do **not** leave change‑history or "I did X" comments in source or README files (for example, avoid comments like "create a single sender"). Historical context belongs in the PR description or a changelog, not inline.
- Use neutral, factual language. Avoid subjective or minimizing words such as "small", "tiny", "minimal", "just", or "only" when describing a component or its responsibilities.

- Coalesce high-frequency embedder input such as pointer moves and wheel bursts before forwarding them into content, and note a rendering opportunity once per flushed batch rather than once per raw event.
- Move large paint-scene payloads across the content/embedder boundary via `IpcSharedMemory`, and keep the typed IPC message focused on control metadata and shared-memory handles.
- Keep cross-frame paint resources such as fonts in a transport registry keyed by stable identifiers, send new blobs via shared memory when first used for a content-runtime namespace, and keep recorded scenes focused on lightweight references.
- Track navigation completion with an explicit content-to-embedder commit signal instead of inferring it from paint delivery; stale content can repaint while `beforeunload` or replacement navigation is still pending.
- When compositing fixed embedder chrome above scrollable content, append the content scene first and the chrome scene last so scrolled content cannot overpaint fixed controls.
- Build dedicated `formal-web-content` and `formal-web-net` executables from the owning `content` and `net` packages, and launch those sidecars directly from the user agent instead of respawning the main `formal-web` executable with role flags.
- When a runner or tool launches sibling sidecars from `target/{debug,release}`, build the `formal-web`, `content`, and `net` packages for that profile so the spawned executables stay in sync.
- Model cross-component browser identities such as documents, browsing contexts, browsing-context groups, agents, agent clusters, event loops, and beforeunload checks as UUID newtypes in `ipc_messages`, allocate them at the use site, and reserve raw integer counters for process-local handles or transport sequencing only.
- Treat vendored third-party code and WPT resources as read-only unless the task explicitly calls for vendor changes; debug compatibility issues from local code or scratchpad artifacts instead.
- Web standards should be available locally under `web_standards`, so before fetching a web standard over the internet, check that folder first. 
- Keep TLC outside the repository; verification uses the local TLA+ Toolbox jar at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar` instead of a vendored `tla2tools.jar`.
- Treat build errors introduced or uncovered during a task as part of the task, and leave the project with a clean successful build before finishing.

At the end of a task, always confirm `cargo run --release` builds the project successfully, always run the wpt tests without a path, always run `./verification/verify-navigation.sh`, and then exit the terminal.

Basic validation for each task should exercise the headless StartupExample navigation flow through WebDriver by loading `artifacts/StartupExample.html`, clicking `a.article-link`, and polling until the current URL reaches `artifacts/navigated.html`. The `./verification/verify-navigation.sh` script performs that flow with verification enabled.

For tasks that change navigation tracing or the `Navigation` TLA model, also exercise a navigation via WebDriver, preferably headless, with verification enabled so shutdown validation runs against the recorded trace. Use `cargo run -- validate-tla --logs ...` only for focused diagnosis against preserved logs, and keep that validation flow separate from WPT runs.

Plans, TODOS, and temporary task logs belong in `scratchpad`. Verification session logs and TLC working traces belong under the system temp directory and should be removed after validation finishes.

At the end of each task, output as part of your closing comment a good possible commit message for the work done.