# File System Boundaries

Agents may read and write files freely within:

- The current repository (all files, including git-ignored ones)
- System temp directories (/tmp, $TMPDIR, or equivalent)

All other locations require explicit user approval before any write, move, or delete operation. This includes (but is not limited to):

- Files outside the repo root
- Other repositories or project directories
- Home directory dotfiles and config (~/.config, ~/.bashrc, etc.)
- Shared or system-wide directories (/usr/local, /etc, etc.)
- Files under `vendor/`, generally speaking those should not be edited unless the user directs you to do so. Those files should not be considered part of the repo (so if the user instructs to do something "across the repo", that excludes vendor).

When in doubt, ask before writing.

# Safety

Never write any unsafe code withou the user's explicit approval.

# Documentation Chain

Read repository documentation from general to specific:
1. `AGENTS.md` (top-level)
3. `<subdir>/{<nested-sub-dir/}README.md`

The above should form a chain of readmes, based on the directories where you are working for the task at hand. Essentially, when you are reading or writing to a file, you must take into account all readmes in the path to that file.

You can also update this documentation chain based on lessons learned from user feedback: update the lowest-level file that owns the rule or pattern, and avoid repeating the same guidance in multiple places.

### readme-chain extension

The `.pi/extensions/readme-chain/` extension provides:
- **`readme_chain({ path })` tool** — Call this before editing a file to fetch the full
  chain of AGENTS.md and README.md files for that file's path, from general to specific.
  Reading the chain is always preferred over relying on memory.
- **`/readme-chain [path]` command** — Lists the chain files for a path (for human use).
- **Automatic reminder** — When the agent tries to `edit`, `write`, or `read` a source file
  in a directory whose chain has not been consulted yet, a one-per-prompt warning is shown.

See `.pi/extensions/readme-chain/README.md` for full documentation.

# Algorithm Implementation

## Three-layer architecture

Every Web-exposed feature (DOM, HTML, Streams, WebAssembly) follows the same
three-layer split.  See **`content/src/js/bindings/README.md`** for the
definitive breakdown with examples and common mistakes.

| Layer | Location | Signature convention |
|---|---|---|
| **Domain** | `content/src/<domain>/` | Methods implement spec algorithms. May import some Boa types when the algorithm requires it (e.g., `Context` for promise creation). |
| **Web IDL bindings infra** | `content/src/webidl/bindings/` | Generic traits — NOT domain-specific |
| **JS bindings glue** | `content/src/js/bindings/<domain>/` | `fn(this, args, ctx) -> JsResult<JsValue>` — thin: extract JS args, call domain, wrap |

When implementing a spec algorithm, every changed file must satisfy these checks
before the task is considered done.  See `content/src/js/bindings/README.md`
for the definitive spec-annotation reference with examples and common mistakes.

1. **Step comments** — Every spec step has a `// Step N:` comment inside the
   function body quoting the **exact spec step text verbatim** — not an
   abbreviation or summary.  Step numbering must match the spec exactly.
2. **Anchor URLs** — Every function/struct top doc comment has **only** the
   correct spec anchor URL (`<https://html.spec.whatwg.org/#...>`).  No
   description, no step summary, no prose.
3. **`// Note:` only for discrepancies** — Notes explain why the code differs
   from the spec (e.g. steps merged, split across processes, browser-engine
   specific refactoring).  Design notes, architecture rationales, and
   implementation plans belong in the README chain, not in `// Note:`.

4. **Mirror spec sub-algorithms as separate functions** — When a spec algorithm
   calls a named sub-algorithm (e.g. "instantiate the core of a WebAssembly
   module", "initialize an instance object"), create a dedicated function with
   its own anchor URL and step comments.  Do not inline sub-algorithm logic
   into the parent function.

5. **No catch-all utility files** — Name domain modules by spec capability, not
   by `utils.rs`/`functions.rs`/`helpers.rs`.  Each file should correspond to
   a well-defined spec concept or algorithm group.

See the "Spec-mapping review" step under "End-of-Task Flow" for the full
review checklist.

# Project Structure

The formal-web project implements a web browser from scratch from separate processes coordinated by the user agent. The main `formal-web` binary launches dedicated `formal-web-content` and `formal-web-net` processes from the `content` and `net` packages. The embedder delegates to these processes through the `webview` and `user_agent` layers, keeps paint payloads on shared-memory transport, and uses typed IPC messages for metadata and handles. Navigation completion uses explicit content-to-embedder commit signaling.

TLA+ models under `verification/` verify critical algorithms (e.g. navigation). The TLA+ Toolbox jar is at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar`. Verification artifacts go in temporary directories.

Plans and temporary task notes go under `scratchpad/`.

## Commands

- `rustup toolchain install 1.94.0` — installs the pinned Rust toolchain.
- `rustup run 1.94.0 cargo check` — type-checks the workspace.
- `rustup run 1.94.0 cargo run --release` — default windowed embedder.
- `rustup run 1.94.0 cargo run --release -- --headless` — headless mode.
- `rustup run 1.94.0 cargo run --release -- --verify` — with trace recording and shutdown-time TLA+ validation.
- `rustup run 1.94.0 cargo run --release -- webdriver --headless` — WebDriver server.
- `rustup run 1.94.0 cargo run --release -- cdp --headless` — CDP server.
- `rustup run 1.94.0 cargo run --release -- webdriver --headless --cdp-port 9222` — WebDriver and CDP together.
- `rustup run 1.94.0 cargo run --release -- wpt` — runs the default WPT and local formal test selection.
- `rustup run 1.94.0 cargo run --release -- wpt formal/load-event-fires.html` — runs one selected test.
- `./verification/verify-navigation.sh` — headless navigation workflow validated against the TLA+ `Navigation` spec.
- `./verification/verify-rendering.sh` — headless screenshot-based rendering verification (startup artifact + cross-origin iframe).
- `rustup run 1.94.0 cargo run -- validate-tla --logs /path/to/logs --json` — validates a saved trace log directory.

# Local Extensions

## pi-share-hf — Session Collection

The `.pi/extensions/pi-share-hf/` extension archives pi sessions to `.pi/collected-sessions/`.

- **Auto-collection on shutdown:** When pi exits, the session is automatically saved to a unique file in `.pi/collected-sessions/`. No manual action is needed.
- **`/collect-session` command** — Interactive command to archive the current session at any point.
- **`upload_session` tool** — Stub only; not yet implemented. Will eventually upload collected sessions to a remote destination (e.g. Hugging Face dataset).

## pi-browser — CDP Browser Tools

The `.pi/extensions/browser/` extension wraps formal-web's CDP server into
agent-callable tools for live interactive debugging during feature development.

- **`browser_navigate`** — Navigate to a URL and wait for load.
- **`browser_evaluate`** — Run a JavaScript expression in the page context.
- **`browser_click`** — Click an element by CSS selector.
- **`browser_type`** — Type text into an input.
- **`browser_hover`** — Hover over an element for CSS `:hover` testing.
- **`browser_get_text`** — Read visible text from the page or a selector.
- **`browser_get_attribute`** — Read a DOM attribute value.
- **`browser_get_computed_style`** — Read a computed CSS property.
- **`browser_screenshot`** — Capture a PNG screenshot.
- **`browser_capture_console`** — Collect console output for N milliseconds.
- **`browser_history_back`** — Go back in browser history.
- **`browser_reload`** — Reload the current page.
- **`/browser-connect [port]`** — Connect to a CDP endpoint.
- **`/browser-status`** — Show connection state and targets.

See `.pi/extensions/browser/README.md` for tool details, command reference,
formal-web CDP specifics, and the future roadmap.

## Testing with formal-web

Formal-web supports three testing interfaces. See `automation/README.md`
for detailed documentation.

## web_standards — Spec Reading

The `.pi/extensions/web_standards/` extension lazily loads and caches web standards documents (WHATWG, W3C, etc.) on first use. Provides three tools for the agent to read specs interactively:

- **`spec_lookup`** — Look up a named anchor in a spec by its `id` attribute. Returns the element's tag, rendered content, and walks forward siblings to show algorithm boxes (with full recursive step numbering) until the next heading or named definition. This is the primary tool for reading spec content.
- **`spec_ref_links`** — Find every place a concept is referenced in a spec. Returns the full URL for each usage site with its enclosing algorithm/section context. Use with `read` to render the full content at a specific reference location.
- **`spec_search_id`** — Search for element `id` attributes containing a given substring. Use to discover anchor IDs when you know a keyword but not the exact id.

# Naming Conventions

- Use descriptive variable names throughout. Single-letter names (`s`, `st`, `wid`, `el`, `p`,
  `cs`, `at`, `ch`) are prohibited in new code and should be expanded when touching existing
  code. A variable called `state` is always clearer than `s`.
- Exception: closure parameters in iterator chains (`.map(|x| ...)`) where the type is obvious
  from context. But even there, prefer short but meaningful names like `tab` over `t`.
- **Never use fully qualified paths** like `crate::wasm::namespace::compile_fn(...)` in
  binding function bodies. Import with `use` at the top of the file and call unqualified.
- Do not bulk-rename existing code with scripts — it creates merge conflicts, breaks history,
  and introduces subtle bugs when renames are inconsistent. Rename incrementally when
  modifying nearby code.

# Spec Fidelity

- Describe current architecture and behavior; keep task history out of repository docs.
- Keep README guidance general and durable; one-off implementation details belong in source or tests, not in repository docs.
- Use neutral, factual language.
- Use the `web_standards` extension tools (`spec_lookup`, `spec_ref_links`, `spec_search_id`) to read spec content instead of reading local copies or fetching directly. This is not a one-shot lookup — consult the spec **iteratively** as you write code: start by reading the algorithm to understand the structure, implement the corresponding code, then re-read the spec and compare each step against what you wrote. The spec is the source of truth for both the algorithm logic and the documentation annotations (`// Step N:`, anchor URLs, `// Note:` for discrepancies) that code must carry. The end-of-task spec-mapping review (step 4 below) is the final checkpoint that every algorithm in the changeset is consistently implemented and properly annotated.
- **Reference URLs vs canonical URLs.** In web standards, every definition (`#dfn-foo`) has corresponding reference links (`#ref-for-dfn-foo`, `#ref-for-dfn-foo①`, …) at each usage site. When documenting code that implements a specific algorithm step, prefer the *reference URL* over the canonical concept URL — your code implements "the thing as used in a particular algorithm", not the thing itself. Use `spec_ref_links` to find all reference URLs for a concept.
- Treat `vendor/` and vendored WPT resources as read-only unless the task explicitly requires vendor changes.
- The words "runtime", "sidecar", and "carrier" are forbidden in this repo.
- **Method doc comments:** A method that implements a spec algorithm should have only the spec link as its doc comment. All explanation, step references, and context belong in `//` comments inside the method body. A `// Note:` below the link is acceptable only for brief continuations of the algorithm that cannot be expressed as body comments. Why? Because the entire thing is a runtime, one that implements the Web, and so neither concept should ever be used to model or document some component of what is basically one big integrated system. No component is more or less of a "sidecar" than any other — each plays a specific role. Instead of reaching for these forbidden words, think about what the thing you want to name does, what its role in the system is, and come up with something descriptive.

# Error Logging

# Logging

The project uses the standard `log` crate with `env_logger` for structured logging. All crates depend on `log`; binary crates also depend on `env_logger` and call `env_logger::init()` at startup.

## Log levels by category

| Level | When to use |
|---|---|
| `error!` | Operation failures, system errors, unexpected conditions that need investigation |
| `warn!` | Non-critical issues, unimplemented features, recoverable problems |
| `info!` | Lifecycle events, startup/shutdown, test summaries |
| `debug!` | Debug traces enabled by toggle (e.g. `render-state`, `timer-debug`, `stream-debug`, `cdp`) |
| `trace!` | Very verbose debugging enabled by toggle (e.g. `input-debug`, `startup-debug`) |

## Rules

- Errors must always be logged before being discarded. A `Result` value must never be silently dropped anywhere in the codebase — every `Result<_, E>` carries diagnostic information that can help debug failures in this multi-process system.
- Use `if let Err(error) = fallible_operation() { error!("...: {error}"); }` instead of `let _ = fallible_operation();`. The error message should identify the operation and include the error.
- The only exception is IPC `send()` on reply channels (e.g. `reply.send(...)`, `waiter.send(...)`) where a closed receiver is an expected condition (client disconnected) rather than a system error.
- Avoid bare `.expect()` and `.unwrap()` on `Result` — prefer propagating the error with `?` or logging with `error!` and recovering.
- Use `.ok()` only when the `None`/`Err` case carries no diagnostic value (e.g. parsing an optional value from a fallible source where `None` is a valid "not present" signal).
- The `ConsoleSink::Stderr` variant in `content/src/js/bindings/console.rs` is exempt — it implements the browser Console API output destination, not error logging.

# End-of-Task Flow

At the end of each task, run the following steps **in order**:

1. **Tear down browser/CDP infra** — Kill any formal-web embedder processes
   (`pkill -f "formal-web-embedder")`, CDP servers, or other processes that
   were started during the session. Leftover processes can block ports and
   interfere with subsequent tasks.

2. **Run `cargo clippy`** — Lint the workspace (excluding vendor) and fix any
   warnings before committing. Run from the project root:

   ```bash
   rustup run 1.94.0 cargo clippy --workspace --all-targets
   ```

   Fix all warnings that appear (patch and vendored warnings can be ignored;
   focus on code-level warnings). The `vendor/` directory is excluded from
   this repository's scope and should not be linted or modified.

3. **Run `cargo fmt`** — Format the project's code before committing. Run
   from the project root: `cargo fmt`. This only formats the root package
   (there is no workspace defined, so `vendor/` sub-crates are not affected).
   Never run `cargo fmt` with `--all` or from inside a `vendor/` directory,
   as vendored formatting changes must not be committed.

4. **Spec-mapping review** — First, **re-read the documentation chain**
   (`content/src/js/bindings/README.md`, `AGENTS.md` Algorithm Implementation
   section, `content/README.md`, and any domain-specific READMEs) to
   re-familiarize yourself with the exact rules for anchor URLs, step
   comments, Note conventions, and the three-layer architecture (domain
   method vs JS binding function).  Then review all changed files in that
   light.  For each algorithm implemented:
   - Does the code map to the spec algorithm correctly at the conceptual
     level?  Read the spec algorithm, understand what each step does
     architecturally (which component owns which state, which side effects
     happen where), and verify the implementation reflects that split.
   - Is the algorithm in the right layer?  Domain implementations go in
     `content/src/<domain>/`.  JS binding functions (thin arg-extraction +
     delegation) go in `content/src/js/bindings/<domain>/`.  Only domain
     functions get spec annotations — binding functions have none (they are
     plumbing, not algorithm steps).
   - Does every domain method have `// Step N:` comments quoting the
     **exact spec step text verbatim** (not an abbreviation)?  Step
     numbering must match the spec exactly.
   - Does every domain method/function top doc comment have **only** the
     spec anchor URL (`<https://html.spec.whatwg.org/#...>`)?  No
     description, no step summary, no prose, no "Implements the spec
     algorithm" boilerplate.
   - Are binding function bodies free of fully qualified paths like
     `crate::wasm::namespace::fn_name(...)`?  Import with `use` at the
     top and call unqualified.
   - Are `Note:` comments used only for discrepancies between the code and
     the spec text (never for design notes, implementation plans, or
     architecture rationales — those belong in the README chain)?
   - Are dead or `#[allow(dead_code)]` items justified with a `// Note:` or
     `// TODO:` explaining the gap?
   Fix any issues found.

5. Think very hard about any general lessons learned in the session, and what parts of the documentation chain should be updated to reflect such general lessons, and then also update it. 

6. **Run task-appropriate verification** — Run only the verification steps that are relevant to the changes made. If the task involves changes to browser implementation code, run the following; otherwise skip them:
   - **Default WPT run** — Runs the Web Platform Tests suite (`tests/wpt/include.ini`) to check for regressions in browser behavior. Appropriate for changes to content, DOM, HTML, or Web IDL implementation code.

     ```bash
     rustup run 1.94.0 cargo run --release -- wpt
     ```

     The WPT runner requires a working Python 3 with a functioning `ssl` module and `venv` support. If the run fails with a Python-related error, check `tests/wpt_runner/README.md` for debugging guidance.

   - **`./verification/verify-navigation.sh`** — Builds and launches the formal-web browser with embedded TLA+ verification, tests hyperlink navigation via WebDriver, and validates shutdown-time model checking. Appropriate for changes to navigation, session history, embedder, or content-process code.

7. **Suggest a commit message** — Propose a commit message for changes tracked by git.

8. Do NOT use `collect_session` — that tool has been removed. Sessions are collected automatically on shutdown.


# Forbidden commands

- Do not use Git except for reading history.
- Do not use scripts to edit source code.