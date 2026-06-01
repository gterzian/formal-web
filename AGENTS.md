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

# Project Structure

The formal-web project implements a web browser from scratch, with the main `formal-web` binary launching dedicated `formal-web-content` and `formal-web-net` sidecars from the `content` and `net` packages. The embedder coordinates these sidecars, keeps paint payloads on shared-memory transport, and uses typed IPC messages for metadata and handles. Navigation completion uses explicit content-to-embedder commit signaling.

TLA+ models under `verification/` verify critical algorithms (e.g. navigation). The TLA+ Toolbox jar is at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar`. Verification artifacts go in temporary directories.

Plans and temporary task notes go under `scratchpad/`.

# Local Extensions

## pi-share-hf — Session Collection

The `.pi/extensions/pi-share-hf/` extension archives pi sessions to `.pi/collected-sessions/`.

- **Auto-collection on shutdown:** When pi exits, the session is automatically saved to a unique file in `.pi/collected-sessions/`. No manual action is needed.
- **`collect_session` tool** — Available for manual collection mid-task, when you want to checkpoint before a risky operation or before a `/new` session. Does not upload or share session data.
- **`/collect-session` command** — Interactive equivalent of the above.
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

The `.pi/extensions/web_standards/` extension lazily loads and caches web standards documents (WHATWG, W3C, etc.) on first use. Provides four tools for the agent to read specs interactively:

- **`spec_select`** — Run CSS selectors against a spec document to discover headings (`h2[id]`), definitions (`dfn[id]`), algorithm boxes (`div[data-algorithm]`), etc.
- **`spec_section`** — Read a full section by anchor ID. Walks flat siblings from heading to next same-level heading. Detects algorithm boxes and renders their top-level step structure.
- **`spec_algorithm`** — Read numbered steps from an algorithm box. The HTML uses nested `<ol>` without step numbers; this tool assigns numbers recursively (1, 1.1, 1.1.1, …). Supports `start`/`limit` pagination for long algorithms.
- **`spec_html`** — Return inner HTML of the first matching element. Best for self-contained blocks: tables, definition lists (`dl`), example blocks.
- **`/spec-loaded` command** — Lists all spec URLs currently cached in memory.

# Naming Conventions

- Use descriptive variable names throughout. Single-letter names (`s`, `st`, `wid`, `el`, `p`,
  `cs`, `at`, `ch`) are prohibited in new code and should be expanded when touching existing
  code. A variable called `state` is always clearer than `s`.
- Exception: closure parameters in iterator chains (`.map(|x| ...)`) where the type is obvious
  from context. But even there, prefer short but meaningful names like `tab` over `t`.
- Do not bulk-rename existing code with scripts — it creates merge conflicts, breaks history,
  and introduces subtle bugs when renames are inconsistent. Rename incrementally when
  modifying nearby code.

# Documentation Style

- Describe current architecture and behavior; keep task history out of repository docs.
- Keep README guidance general and durable; one-off implementation details belong in source or tests, not in repository docs.
- Use neutral, factual language.
- Use the `web_standards` extension tools (`spec_section`, `spec_algorithm`, `spec_select`, `spec_html`) to read spec content instead of reading local copies or fetching directly.
- Treat `vendor/` and vendored WPT resources as read-only unless the task explicitly requires vendor changes.
- The word "runtime" is forbidden in this repo. Why? Because the entire thing is a "runtime", one that implements the Web, and so the concept of runtime should never be used to model or document some component of what is basically one big runtime. Instead of reaching for this forbidden word, think about what the thing you want to name does, what its role in the system is, and come-up with something descriptive.

# Error Logging

Errors must always be logged before being discarded. A `Result` value must never be silently dropped anywhere in the codebase — every `Result<_, E>` carries diagnostic information that can help debug failures in this multi-process system.

- Use `if let Err(error) = fallible_operation() { eprintln!("...: {error}"); }` instead of `let _ = fallible_operation();`. The error message should identify the operation and include the error.
- The only exception is IPC `send()` on reply channels (e.g. `reply.send(...)`, `waiter.send(...)`) where a closed receiver is an expected condition (client disconnected) rather than a system error.
- Avoid bare `.expect()` and `.unwrap()` on `Result` — prefer propagating the error with `?` or logging with `eprintln!` and recovering.
- Use `.ok()` only when the `None`/`Err` case carries no diagnostic value (e.g. parsing an optional value from a fallible source where `None` is a valid "not present" signal). 

# End-of-Task Flow

At the end of each task, run the following steps **in order**:

1. **Tear down browser/CDP infra** — Kill any formal-web embedder processes
   (`pkill -f "formal-web-embedder")`, CDP servers, or other sidecars that
   were started during the session. Leftover processes can block ports and
   interfere with subsequent tasks.

2. **Run `cargo fmt`** — Format the entire project before committing. Run
   from the project root: `cargo fmt`. This covers all crates in the workspace.

3. **Suggest a commit message** — Propose a commit message for changes tracked by git.

4. **Run task-appropriate verification** — Run only the verification steps that are relevant to the changes made. If the task involves changes to browser implementation code, run the following; otherwise skip them:
   - **Default WPT run** — Runs the Web Platform Tests suite to check for regressions in browser behavior. Appropriate for changes to content, DOM, HTML, or Web IDL implementation code.
   - **`./verification/verify-navigation.sh`** — Builds and launches the formal-web browser with embedded TLA+ verification, tests hyperlink navigation via WebDriver, and validates shutdown-time model checking. Appropriate for changes to navigation, session history, embedder, or content-process code.
4. DO NOT collect pi sessions; those are collected automatically on shutdown.