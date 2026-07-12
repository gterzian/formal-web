# Rule Number One

Only ever perform an action if it directly relates to a coding task in the current repository.

# Rule Number Two: External Network

Never navigate to external domains or make network requests to external
hosts without explicit prior approval from the user.  Use only local
resources (localhost, file://, in-repository artifacts).

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

# grep caution

When using `grep` (or `rg`/`find`), **never** search paths outside the repository root or under `vendor/` without explicit narrowing. In particular, avoid searching `~/.cargo/registry/` or other system-wide locations — those directories are large and the search will hang indefinitely. Instead, use `cargo doc` and check the generated docs, or browse the relevant source files directly with `read`.

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
2. **Anchor URLs** — Every function, struct, associated constant, and
   constant definition top doc comment has **only** the correct spec anchor
   URL (`<https://html.spec.whatwg.org/#...>`).  No description, no step
   summary, no prose.  **Zero prose — not a single explanatory sentence.**
   If the function name is not enough context, the spec IS the documentation.
   Explanatory doc comments on spec-implementing functions are violations.

   | ❌ Wrong | ✅ Right |
   |---|---|
   | `/// <…>\n/// Queues a microtask via Boa's enqueue_job API.` | `/// <https://html.spec.whatwg.org/#queue-a-microtask>` |
   | `/// <…>\n/// Content-process portion of the algorithm. …` | `/// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>` |
   | `/// <…>\n/// Result of the rules for choosing a navigable. …` | `/// <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>` |

   Constants like `NETWORK_EMPTY`, `HAVE_NOTHING`, and
   `MEDIA_ERR_ABORTED` are spec-defined IDL enum values and must carry their
   spec anchor (`#dom-media-networkstate`, `#dom-media-readystate`,
   `#dom-mediaerror-media_err_aborted` etc.) just like any method or struct.
3. **`// Note:` only for discrepancies** — A `// Note:` following the anchor URL
   on a separate line is the **only** exception to the no-prose rule, and only
   for genuine discrepancies between the code and the spec (e.g. steps merged,
   split across processes, browser-engine specific refactoring).  Such notes
   must be countable on two hands across the entire codebase — fewer than ten.
   Design notes, architecture rationales, and implementation plans belong in
   the README chain, not in doc comments or `// Note:`.

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

The formal-web project implements a web browser from scratch from separate processes coordinated by the user agent. The main `formal-web` binary runs the embedder directly in-process and launches dedicated `formal-web-content` and `formal-web-net` helper processes from the `content` and `net` packages. It delegates to these processes through the `webview` and `user_agent` layers, keeps paint payloads on shared-memory transport, and uses typed IPC messages for metadata and handles. Navigation completion uses explicit content-to-embedder commit signaling.

TLA+ models under `verification/` verify critical algorithms (e.g. navigation). The TLA+ Toolbox jar is at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar`. Verification artifacts go in temporary directories.

Plans and temporary task notes go under `scratchpad/`.

## Commands

- `rustup toolchain install 1.94.0` — installs the pinned Rust toolchain.

## Build Architecture

The root `Cargo.toml` defines a `[workspace]` with all project packages as
members.  `cargo build --release` builds everything in one invocation with
shared dependency resolution and incremental compilation.

### Components

- **Root binary** (`formal-web`): runs the embedder directly in-process, creating the window and event loop.
- **Embedder crate** (`embedder`): a library used by the root binary that owns the winit event loop, window, chrome, and automation plumbing. A standalone `formal-web-embedder` binary is also produced for direct use.
- **Helper processes** (`formal-web-content`, `formal-web-net`, `formal-web-media`): spawned by the embedder.
- **`js_engine` crate**: a generic JS engine trait and ECMA-262 abstract operations. Two backends: Boa (default, most operational) and JSC (macOS opt-in). WebAssembly is a separate feature (`wasm`). See `js_engine/README.md`.
- **`js_engine_macros` crate**: proc-macro companion providing `#[gc_struct]` for GC-traced platform objects.

### Feature flags

| Flag | Effect | Default |
|---|---|---|
| `boa` | Boa JS engine backend (most operational, runs WPT) | yes |
| `jsc` | JavaScriptCore backend (macOS only, experimental) | no |
| `wasm` | WebAssembly support via wasmtime (opt-in, Boa only) | no |
| `media` | Video/audio playback support | yes |

Boa is the primary backend for running WPT tests.  Wasm is a separate feature
to avoid pulling in wasmtime when not needed.  JSC is macOS-only and
experimental (see `js_engine/README.md` for known issues).

### Three verbs

## Build commands

### Default build (Boa, no WebAssembly)

```bash
# Check all — type-check every package
rustup run 1.94.0 cargo check

# Build all — produce all binaries
rustup run 1.94.0 cargo build --release

# Run all — launch the embedder
rustup run 1.94.0 cargo run --release

# Run WPT tests (primary verification)
rustup run 1.94.0 cargo run --release -- wpt
```

### With WebAssembly (opt-in)

```bash
rustup run 1.94.0 cargo build --release --features wasm
```

### JSC backend (macOS only)

```bash
# Build content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run WPT via JSC content process
RUST_LOG=error target/release/formal-web wpt <test-path>
```

### Without media (no video playback)

```bash
rustup run 1.94.0 cargo build --release --no-default-features
```

### Individual packages

```bash
cargo build --release -p content --bin formal-web-content
cargo build --release -p net     --bin formal-web-net
cargo build --release -p embedder --bin formal-web-embedder
```

### Media binary

```bash
# macOS: AVFoundation (default) — no special flags needed
cargo build --release -p media --bin formal-web-media

# macOS: GStreamer (opt-in)
cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-gstreamer

# Linux: GStreamer (only backend) — no special flags needed
cargo build --release -p media --bin formal-web-media
```

### External dependencies: blitz and anyrender

Blitz crates (blitz-traits, blitz-dom, blitz-paint, blitz-html, stylo_taffy,
debug_timer) come from a git dependency on
<https://github.com/gterzian/blitz> (rev `954b41f`).

AnyRender crates (anyrender, anyrender_vello, anyrender_vello_cpu,
anyrender_svg, wgpu_context) are sourced from crates.io at the versions
required by the blitz workspace (0.10, 0.10.1, 0.12.1, 0.11.0, 0.6.0
respectively).

### IPC wire format consistency

The helper processes (`formal-web-content`, `formal-web-net`,
`formal-web-media`) are separate workspace member binaries, **not** in
the root binary's dependency tree.  `cargo run --release` rebuilds only
the root binary and its transitive library deps — it does **not** rebuild
the helper binaries.

As long as Rust types (`IpcSender<T>`, message enums) stay the same,
stale helper binaries are harmless — parent and child share the same
serde-driven wire format.  But changes to the `ipc/` crate that alter
the **wire envelope** (e.g. wrapping messages in a new tuple, changing
the channel type parameter) change the serialization format.  After such
a change, old helper binaries will fail to deserialize, producing
`DeserializeUnexpectedEnd` errors.  Cargo cannot detect this because the
wire format is an implicit protocol, not a type-level dependency.

**To recover from protocol mismatch:** `cargo clean` the affected
member packages and rebuild:

```bash
cargo clean -p content -p net -p media -p ipc -p user_agent -p embedder
cargo build --release
cargo run --release
```

To avoid the issue entirely after a protocol-changing edit, run a full
build before running:

```bash
cargo build --release   # rebuilds EVERY workspace binary
cargo run --release     # all processes are in sync
```

### README pruning

The `js_engine/README.md` (and any other `README.md`) tracks only:
- Things that **still need to be fixed** (unfixed bugs, pre-existing issues)
- **Dead-end investigations** for currently-unfixed issues (so future sessions
  know what was already tried and ruled out)

Do NOT document:
- Completed fixes (they're in the code and git history)
- Architecture design notes or historical session logs for fixed issues
- Infrastructure descriptions for things that already work

The goal is concise, actionable documentation: a future session should be able
to read the README and know exactly what remains to be done, and what approaches
have already failed for each remaining issue.

### Process binary search paths

When the embedder spawns a helper process, it searches the directory
containing its own executable (`target/{profile}/`).  With the workspace,
all binaries land in the shared `target/{profile}/` directory, so the
embedder finds them by default.

# Local Extensions

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

  **Truncated dfn → scroll to section.** A `<dfn>` is inline inside a `<p>`, so its algorithm `<ol>` sibling is out of reach. When the result looks incomplete, check the `Section:` line — its value is the section heading id. Look that up next. See `.pi/extensions/web_standards/README.md` for details.
- **`spec_ref_links`** — Find every place a concept is referenced in a spec. Returns the full URL for each usage site with its enclosing algorithm/section context. Use with `read` to render the full content at a specific reference location.
- **`spec_search_id`** — Search for element `id` attributes containing a given substring. Use to discover anchor IDs when you know a keyword but not the exact id.

# Naming Conventions

- Use descriptive variable names throughout. Single-letter names (`s`, `st`, `wid`, `el`, `p`,
  `cs`, `at`, `ch`) are prohibited in new code and should be expanded when touching existing
  code. A variable called `state` is always clearer than `s`.
- Exception: closure parameters in iterator chains (`.map(|x| ...)`) where the type is obvious
  from context. But even there, prefer short but meaningful names like `tab` over `t`.
- **Never use fully qualified paths** — no `crate::foo::bar::baz(...)` anywhere.
  Import with `use` at the top of the file and call unqualified.
  The only exception is disambiguating between two crates that export the same name,
  and even then prefer `use ... as` renaming.
- Do not bulk-rename existing code with scripts — it creates merge conflicts, breaks history,
  and introduces subtle bugs when renames are inconsistent. Rename incrementally when
  modifying nearby code.
- **No wildcard imports** — `use foo::bar::*` is prohibited. Every import must list the
  specific types or traits used. This makes dependencies clear at every module boundary.

# Statics and Atomics

Never use a `static` or atomic when a local variable or parameter will do.
Statics and atomics are only justified for genuinely cross-thread shared
mutable state (e.g. a counter accessed from multiple OS threads).  Do not
reach for them as a convenience — a plain local is simpler, testable, and
ever correct.

# Never Assume Test Failures Are Pre-Existing

Every test failure is a regression until proven otherwise.  A failure
is NOT "pre-existing to the current session" — it might pre-exist on
the current branch, but that still means the branch has a bug that
needs fixing.  Never dismiss a failure as "pre-existing" without first
verifying the test baseline (e.g., reverting changes and running the
same test).  If you do not know the baseline, say so — do not
fabricate one.

When investigating a failing test, ask: did this test ever pass on
this branch?  If you changed code that a test exercises, that test is
your responsibility until it passes.  Dismissing failures as "not yet
implemented" is a form of speculation: you are guessing that the
feature never worked, instead of checking whether it did.

# Spec Fidelity

- Describe current architecture and behavior; keep task history out of repository docs.
- Keep README guidance general and durable; one-off implementation details belong in source or tests, not in repository docs.
- Use neutral, factual language.
- Use the `web_standards` extension tools (`spec_lookup`, `spec_ref_links`, `spec_search_id`) to read spec content instead of reading local copies or fetching directly. This is not a one-shot lookup — consult the spec **iteratively** as you write code: start by reading the algorithm to understand the structure, implement the corresponding code, then re-read the spec and compare each step against what you wrote. The spec is the source of truth for both the algorithm logic and the documentation annotations (`// Step N:`, anchor URLs, `// Note:` for discrepancies) that code must carry. The end-of-task spec-mapping review (step 4 below) is the final checkpoint that every algorithm in the changeset is consistently implemented and properly annotated.
- **Reference URLs vs canonical URLs.** In web standards, every definition (`#dfn-foo`) has corresponding reference links (`#ref-for-dfn-foo`, `#ref-for-dfn-foo①`, …) at each usage site. When documenting code that implements a specific algorithm step, prefer the *reference URL* over the canonical concept URL — your code implements "the thing as used in a particular algorithm", not the thing itself. Use `spec_ref_links` to find all reference URLs for a concept.
- Treat `vendor/` and vendored WPT resources as read-only unless the task explicitly requires vendor changes.
- The words "runtime", "sidecar", and "carrier" are forbidden in this repo.
- **Method doc comments:** A method that implements a spec algorithm should have only the spec link as its doc comment. All explanation, step references, and context belong in `//` comments inside the method body. A `// Note:` below the link is acceptable only for brief continuations of the algorithm that cannot be expressed as body comments. Why? Because the entire thing is a runtime, one that implements the Web, and so neither concept should ever be used to model or document some component of what is basically one big integrated system. No component is more or less of a "sidecar" than any other — each plays a specific role. Instead of reaching for these forbidden words, think about what the thing you want to name does, what its role in the system is, and come up with something descriptive.
- **Document only verified facts.** Never speculate about root causes, fixes, or
  explanations for observed behavior unless you have confirmed them through
  instrumentation, debugging, or testing.  When documenting an issue, state
  only what was observed, what was tried, and what was ruled out.  A statement
  like "this might be caused by X" is speculation unless X was verified.
  Prefer phrasing like "symptom: X works then crashes; Y was tried and failed;
  Z was not investigated" over "the issue is likely due to X".

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
- **Never use `let _ = ...`** to silence a `Result`. Every `Result` carries diagnostic
  information; silent discarding makes multi-process failures impossible to debug.
  Always use `if let Err(error) = fallible_operation() { error!("...: {error}"); }`.
  The error message must identify the operation.
- The only exception is IPC `send()` on reply channels (e.g. `reply.send(...)`, `waiter.send(...)`) where a closed receiver is an expected condition (client disconnected) rather than a system error.
- Avoid bare `.expect()` and `.unwrap()` on `Result` — prefer propagating the error with `?` or logging with `error!` and recovering.
- Use `.ok()` only when the `None`/`Err` case carries no diagnostic value (e.g. parsing an optional value from a fallible source where `None` is a valid "not present" signal).
- The `ConsoleSink::Stderr` variant in `content/src/js/bindings/console.rs` is exempt — it implements the browser Console API output destination, not error logging.

# Session Investigation Documentation

Every session that investigates an open bug or unexpected behavior must
log its findings in the relevant `README.md` file under a
"Session investigation log" section. The log must follow these rules:

1. **Factual only** — Document what was done, what was instrumented, and
   what tests were run. No speculation about what the fix might be.
2. **Document dead ends** — Explicitly state what was ruled out and why.
   The next session needs to know what NOT to retry.
3. **No speculative solutions** — If you found a fix, implement it and
   update the issue status. If you did not find a fix, do not suggest what
   the fix might be. That is for the next session to discover.
4. **Include investigation scope** — Which files were changed (even for
   instrumentation), which single test was used to verify, and what the
   instrumentation confirmed.

Example structure:

```
### <date> — <issue description>

**Files changed:** <list>
**Instrumentation added:** <what and where>
**What was confirmed:** <facts>
**What was ruled out:** <dead ends>
**Not investigated:** <what remains to check>
```

Location: Add the log to the lowest-level README.md in the directory
hierarchy that owns the feature being investigated (e.g., `content/src/streams/README.md`
for stream issues, `js_engine/README.md` for JS engine issues).

# End-of-Task Flow

At the end of each task, run the following steps **in order**:

1. **Tear down browser/CDP infra** — Kill any formal-web processes
   (`pkill -f "formal-web"`)`, CDP servers, or other processes that
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
   - Does every domain method, function, struct, and associated
     constant top doc comment have **only** the spec anchor URL
     (`<https://html.spec.whatwg.org/#...>`)?  No description, no step
     summary, no prose, no "Implements the spec algorithm" boilerplate.
     Constants (`NETWORK_EMPTY`, `HAVE_NOTHING`, `MEDIA_ERR_ABORTED`)
     are spec IDL values and must carry their anchor just like any
     method.
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

6. **Prune READMEs** — Strip completed fixes and historical session logs from
   the documentation chain. The README should track only remaining work and
   dead-end investigations for currently-unfixed issues (see "README pruning"
   above).

7. **Run all verification steps** — Every end-of-task run executes ALL verification steps unconditionally. Do not skip any step based on a subjective assessment of "relevance" — changes to seemingly unrelated files (test pages, configuration, documentation) routinely break downstream steps in this multi-process system. Running everything catches regressions the agent cannot predict.

   **Migration override:** Phase E is complete — content crate compiles on
   both JSC and Boa. Standard verification steps (WPT, navigation
   verification, clippy, fmt) should now be run. However, the content crate
   is still not functional on JSC (`run_content_process` returns an error),
   so only the Boa backend path is verified.

   - **Default WPT run** —

     ```bash
     rustup run 1.94.0 cargo run --release -- wpt
     ```

     The WPT runner requires a working Python 3 with a functioning `ssl` module and `venv` support. If the run fails with a Python-related error, check `tests/wpt_runner/README.md` for debugging guidance.

   - **Navigation verification** — Validates hyperlink navigation and shutdown-time TLA+ model checking via WebDriver:

     ```bash
     ./verification/verify-navigation.sh
     ```

7. **Suggest a commit message** — Whenever asked for a commit message (whether at end-of-task or any other time), propose a message for the current `git diff HEAD` (the uncommitted changes), not for the entire session's work.  Run `git diff --stat HEAD` to see what changed, and `git diff HEAD` to read the diff before writing the message.

8. Review the entire session (your entire context window) and make sure that Rule Number One was respected (see top of file), and if not alert the user.


# Forbidden commands

- Do not use Git except for reading history.
- Do not use scripts to edit source code.