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

## CDP Browser Testing

The pi agent has built-in browser tools (`browser_navigate`, `browser_click`,
`browser_evaluate`, `browser_get_text`, `browser_screenshot`) that connect to a
CDP-compatible browser. formal-web implements a CDP server for testing:

```bash
# Start the formal-web browser with CDP enabled
cargo run --bin formal-web -- cdp --headless --port 9222 \
  --startup-url "file:///path/to/artifacts/test-page.html"

# Enumerate page targets
curl http://127.0.0.1:9222/json

# Connect to a page target via WebSocket for CDP commands
# (Node.js WebSocket API works out of the box):
```

```javascript
const ws = new WebSocket('ws://localhost:9222/devtools/page/<id>');
ws.addEventListener('open', () => {
  ws.send(JSON.stringify({
    id: 1,
    method: 'Runtime.evaluate',
    params: { expression: 'document.title' }
  }));
});
ws.addEventListener('message', (event) => {
  console.log(JSON.parse(event.data));
});
```

The built-in pi browser tools (`browser_navigate`, etc.) do not currently
connect to formal-web's CDP — they target a separate browser instance.
For testing formal-web, use direct WebSocket CDP commands as shown above
or the WebDriver interface via `formal-web webdriver`.

## web_standards — Spec Reading

The `.pi/extensions/web_standards/` extension lazily loads and caches web standards documents (WHATWG, W3C, etc.) on first use. Provides four tools for the agent to read specs interactively:

- **`spec_select`** — Run CSS selectors against a spec document to discover headings (`h2[id]`), definitions (`dfn[id]`), algorithm boxes (`div[data-algorithm]`), etc.
- **`spec_section`** — Read a full section by anchor ID. Walks flat siblings from heading to next same-level heading. Detects algorithm boxes and renders their top-level step structure.
- **`spec_algorithm`** — Read numbered steps from an algorithm box. The HTML uses nested `<ol>` without step numbers; this tool assigns numbers recursively (1, 1.1, 1.1.1, …). Supports `start`/`limit` pagination for long algorithms.
- **`spec_html`** — Return inner HTML of the first matching element. Best for self-contained blocks: tables, definition lists (`dl`), example blocks.
- **`/spec-loaded` command** — Lists all spec URLs currently cached in memory.

# Documentation Style

- Describe current architecture and behavior; keep task history out of repository docs.
- Keep README guidance general and durable; one-off implementation details belong in source or tests, not in repository docs.
- Use neutral, factual language.
- Use the `web_standards` extension tools (`spec_section`, `spec_algorithm`, `spec_select`, `spec_html`) to read spec content instead of reading local copies or fetching directly.
- Treat `vendor/` and vendored WPT resources as read-only unless the task explicitly requires vendor changes.
- The word "runtime" is forbidden in this repo. Why? Because the entire thing is a "runtime", one that implements the Web, and so the concept of runtime should never be used to model or document some component of what is basically one big runtime. Instead of reaching for this forbidden word, think about what the thing you want to name does, what its role in the system is, and come-up with something descriptive. 

# End-of-Task Flow

At the end of each task, run the following steps **in order**:

1. **Suggest a commit message** — Propose a commit message for changes tracked by git.

2. **Run task-appropriate verification** — Run only the verification steps that are relevant to the changes made. If the task involves changes to browser implementation code, run the following; otherwise skip them:
   - **Default WPT run** — Runs the Web Platform Tests suite to check for regressions in browser behavior. Appropriate for changes to content, DOM, HTML, or Web IDL implementation code.
   - **`./verification/verify-navigation.sh`** — Builds and launches the formal-web browser with embedded TLA+ verification, tests hyperlink navigation via WebDriver, and validates shutdown-time model checking. Appropriate for changes to navigation, session history, embedder, or content-process code.

3. **Collect the session is automatic** — Session collection happens automatically on shutdown via the `pi-share-hf` extension. You no longer need a manual collection step. However, you can still use `collect_session` mid-task to checkpoint before a risky operation or before starting a new session.
