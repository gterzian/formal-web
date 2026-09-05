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

# Search loops over large outputs

Never run multi-line `awk`/`sed` loops that walk the entire output of a compiler or test run line by line (e.g. `awk '/error/{...} while(...)'` over a full `cargo check` log). These run at 100% CPU and take far too long to complete. When you need to inspect compiler errors or other large logs:

- Read the log file in chunks with the `read` tool (it truncates at 2000 lines / 50KB with an offset to continue).
- Use single simple `grep` invocations with narrow patterns (e.g. `grep -E "error\[E" file | head`) — one pass, no loops.
- Prefer filtering at the source: run the command with `2>&1 | tail -N` or pipe to `grep` once, then save the output to a file and `read` it.

# Long sleeps in bash commands

Avoid `sleep` commands longer than a few seconds in bash. A long `sleep N` (N > 5) inside a tool call blocks the session for no benefit: run the command that produces the result (a test run, a build, a poll) with an appropriate `timeout` on the tool call instead, and let it finish naturally. Prefer running the real command to completion over sleep-then-kill patterns; if a long-running command must be backgrounded, wait on its output file with a short poll loop rather than a single fixed sleep.

# Documentation Chain

Read repository documentation from general to specific:

1. `AGENTS.md` (top-level).
2. Every `README.md` found by walking from the repo root down to the
   directory containing the file(s) your task touches, in that order.

Concatenate all of them; together they form the "doc chain" for a task.

## The placement invariant

Every README must sit at the **lowest point in the tree that still covers
all the code beneath it**. Before adding or editing guidance, ask:

- **Is this true for everything below this directory, and only things below
  this directory?** If code in a sibling directory needs the same rule, the
  guidance is too specific for this level — move it up (or leave it out and
  let the parent README cover it).
- **Does this only apply to a subset of what's below this directory?** If
  so, it's too broad for this level — move it down to the README that
  actually owns that subset, or add a new lower README if none exists yet.

`AGENTS.md` sits above everything, so anything placed there is implicitly
claimed to apply repo-wide. Before adding something to `AGENTS.md`, check
whether it's actually scoped to one crate or subtree — if so, it belongs in
that subtree's README instead, not at the root.

## Duplication rule

Do not duplicate guidance between a README and any README already above it
in the same chain (including `AGENTS.md`, which is above everything) — a
task that loads the lower README always loads the higher one too, so nothing
is gained and the two copies will drift out of sync. If a lower README needs
to reference something explained above it, point to the file that owns it
("see `<path>/README.md` for ...") rather than restating it.

Duplication between two READMEs is only acceptable when they can appear in
*separate* doc chains — i.e. a task could plausibly load one without the
other. Example: guidance that applies to a web standard's implementation can
reasonably appear in both `content/README.md` and another crate's README if
a task could touch that standard's code in one crate without touching the
other. When you do duplicate for this reason, keep the duplicate short and
put the full/definitive version in exactly one file, referenced by name from
the other.

## Keeping the chain honest over time

When editing a README, check whether the same fact now also lives in a
README above it in the chain (most commonly `AGENTS.md` itself) — this
happens easily as guidance gets elaborated locally after being introduced
briefly at a higher level. If so, collapse to one copy at the correct level
per the placement invariant above.

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
three-layer split (domain → Web IDL infra → JS bindings glue), with code
placement dictated by which spec the algorithm comes from.  See
`content/src/js/bindings/README.md` for the definitive breakdown with
examples and common mistakes.

When implementing a spec algorithm, every changed file must satisfy these checks
before the task is considered done.  See `content/src/js/bindings/README.md`
for the definitive spec-annotation reference with examples and common mistakes.

1. **Step comments** — Every spec step has a `// Step N:` comment inside the
   function body quoting the **exact spec step text verbatim** — not an
   abbreviation or summary.  Step numbering must match the spec exactly.
   Blank lines separate code BLOCKS, not comments from code:
   - NO blank line between the function/block opening `{` and the first step comment.
   - NO blank line between a step comment and its immediately following code.
   - Blank line AFTER the code, before the next step's comment.
   **Per-step notes, not summaries.**  Each step gets its own `// Step N:`
   comment, in step order, followed — when the step needs explanation (where
   it runs, what is missing, a re-ordering) — by a `// Note:` below that
   step's comment.  When the same note applies to several consecutive steps,
   group those step comments together and write a single `// Note:` below
   the group.  Do NOT write summarizing comments that describe the overall
   split in place of per-step documentation (e.g. a preamble saying "steps
   X, Y, Z run here; the rest run in the user agent") — the per-step
   comments and their notes carry that information.  A function-level
   contract that is not tied to any step (e.g. keeping a returned value
   alive) belongs in the note of the step that returns it.
   **Mirror the split on both sides.**  When an algorithm is split between
   processes (e.g. content runs the document-owning steps, the user agent
   the navigable-owning ones), each side documents **every** step of the
   same algorithm with the same step numbers, and each step's note states
   where it ran ("ran in content", "runs in the user agent", "not
   implemented") — never "steps X-Y are handled elsewhere" without per-step
   comments.  A step that ran entirely on the other side still gets its
   `// Step N:` comment and a note saying so, so the two halves read as
   mirrors of each other.
2. **Anchor URLs** — Every function, struct, associated constant, and
   constant definition top doc comment has **only** the correct spec anchor
   URL (`<https://html.spec.whatwg.org/#...>`).  No description, no step
   summary, no prose.  **Zero prose — not a single explanatory sentence.**
   If the function name is not enough context, the spec IS the documentation.
   Explanatory doc comments on spec-implementing functions are violations.

   The Rust function name MUST match the spec algorithm name, with `_`
   separators replacing spaces and hyphens.  For example, "steps to fire
   beforeunload" becomes `steps_to_fire_beforeunload`, not `fire_global_event`.
   The spec→code mapping must be discoverable by name alone.

   The anchor is a claim that the item **is** that algorithm.  It must name
   the algorithm the item implements, never an algorithm that merely calls
   it or contains the step it runs: a function carrying `#run-a-worker`
   must be run-a-worker.  A helper that stands in for a named sub-algorithm
   the spec calls into (e.g. the fetch that run-a-worker performs) is a
   partial implementation of that sub-algorithm: it gets the sub-algorithm's
   anchor (`#fetch-a-classic-worker-script`), with what is missing
   documented in body `// Note:`s — never the calling algorithm's anchor.
   Code that is not a spec algorithm (IPC commands, enum variants, async
   continuations of a split algorithm, state fields, bindings glue) carries
   no algorithm anchor; its doc comment says what it does and names the
   exact step it serves.

   | ❌ Wrong | ✅ Right |
   |---|---|
   | `/// <…>\n/// Queues a microtask via Boa's enqueue_job API.` | `/// <https://html.spec.whatwg.org/#queue-a-microtask>` |
   | `/// <…>\n/// Content-process portion of the algorithm. …` | `/// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>` |
   | `/// <…>\n/// Result of the rules for choosing a navigable. …` | `/// <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>` |
   | `/// <https://html.spec.whatwg.org/#run-a-worker>` on `start_script_fetch`, a fetch helper that run-a-worker only calls | The sub-algorithm the helper stands in for (`#fetch-a-classic-worker-script`), with the missing parts in body `// Note:`s |

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

6. **`// Note:` for spec discrepancies only** — Inside a function body, `// Note:`
   is for discrepancies between the code and the spec (steps merged across
   processes, browser-engine refactoring).  Design notes, architecture
   rationales, and implementation plans belong in the README chain, not in
   `// Note:` comments.  Count notes on two hands across the codebase.

7. **Domain methods take IDL types, never `JsValue`** — The binding layer converts
   JavaScript values to proper Web IDL types (DOMString → String, EventListener?
   → Option<Callback>, etc.).  Domain methods receive these IDL types and
   implement the spec algorithm.  The only exception is `&mut dyn ExecutionContext<T>`
   when the algorithm needs ECMA-262 operations (promise creation, property access).

8. **Reflectors are set by the Web IDL layer automatically** — The `reflector`
   field on `EventTarget` and `Event` is set by `create_interface_instance`
   via `PostCreateReflector::set_reflector`.  Domain code must never touch
   reflectors.  The binding layer must never set reflectors manually.

9. **Event path building is the caller's responsibility** — `dispatch_with_path`
   takes a pre-built `&[EventPathItem]`.  The caller (binding, HTML event handler)
   resolves JsObjects to EventTargets and builds the path.  The dispatch algorithm
   in `dispatch.rs` operates on domain types only — no JsObject appears in the
   path or the dispatch loop.

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
- **Embedder** (`embedder/`, three independent crates sharing only the `webview`
  crate API): the root `embedder` crate is a thin dispatcher — CLI entry
  points plus the windowed-backend selection (AppKit on macOS by default,
  winit elsewhere). `mac-embedder` is the self-contained AppKit app (no
  winit/Blitz/GPU dependencies). `winit-embedder` holds the winit windowed
  app (Blitz chrome, gated behind its `windowed` feature) **and** the
  headless app used by WPT/WebDriver/CDP. A standalone
  `formal-web-embedder` binary is also produced for direct use.

  **Automation always runs on the winit embedder — never the AppKit one.**
  WPT, WebDriver, CDP, and the TLA+ verification scripts route through
  `winit-embedder` exclusively.  See `embedder/README.md` ("Windowed
  backend selection") for which automation entry points dispatch to winit,
  what the `winit_embedder` feature gates, and why the AppKit app never
  pulls winit.
- **Helper processes** (`formal-web-content`, `formal-web-net`, `formal-web-media`, `formal-web-graphics`): spawned by the embedder.
  - `formal-web-graphics` owns per-webview compositors and video/audio playback (media backend).
    It receives `PaintFrame` and `VideoFrame` payloads and sends back composed scenes with
    `FrameHitInfo` for hit-testing.
- **`js_engine` crate**: a generic JS engine trait and ECMA-262 abstract operations. Three backends: V8 (default, runs WPT), Boa (opt-in), and JSC (macOS opt-in). The `wasm` feature is the Wasmtime-based WebAssembly implementation for the Boa engine only (V8 and JSC implement WebAssembly natively). See `js_engine/README.md`.
- **`js_engine_macros` crate**: proc-macro companion providing `#[gc_struct]` for GC-traced platform objects.

### Feature flags

| Flag | Effect | Default |
|---|---|---|
| `v8` | V8 backend via `rusty_v8` (macOS arm64, runs WPT) | **yes** |
| `boa` | Boa JS engine backend (opt-in; hosts the `wasm` feature) | no |
| `jsc` | JavaScriptCore backend (macOS only, experimental) | no |
| `wasm` | Wasmtime-based WebAssembly implementation (opt-in, Boa only) | no |
| `media` | Video/audio playback support | yes |
| `winit_embedder` | Build the winit **windowed** embedder on macOS (the AppKit backend is the default headed one there and the only one built without this feature); no-op elsewhere, where winit is the only option. On macOS the winit embedder always builds **headless-only** (no graphics deps) for automation (WPT/WebDriver/CDP/verification); this feature adds its windowed app, which headed automation also requires — automation never runs on the AppKit backend | no |

V8 is the default backend for running WPT tests.  Wasm is a separate feature
(and Boa-only) to avoid pulling in wasmtime when not needed.  JSC is
macOS-only and experimental (see `js_engine/src/jsc/README.md` for known
issues).

## Build commands

### Default build (V8)

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

### Boa (opt-in)

```bash
rustup run 1.94.0 cargo build --release --no-default-features --features boa,media
rustup run 1.94.0 cargo run --release --no-default-features --features boa,media -- wpt
```

### With WebAssembly (opt-in, Boa only)

The `wasm` feature is the Wasmtime-based WebAssembly implementation for the
Boa engine (V8 and JSC implement WebAssembly natively — no feature needed).

```bash
rustup run 1.94.0 cargo build --release --no-default-features --features boa,wasm,media
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
rustup run 1.94.0 cargo build --release --no-default-features --features v8
rustup run 1.94.0 cargo run --release --no-default-features --features v8
```

### Winit embedder (opt-in, macOS only)

```bash
rustup run 1.94.0 cargo build --release --features winit_embedder
rustup run 1.94.0 cargo run --release --features winit_embedder
```

On macOS the AppKit backend is the default and the winit backend is not
compiled unless `winit_embedder` is enabled. On other platforms the winit
backend is the only option and needs no feature. Automation (WebDriver,
CDP, WPT, TLA+ verification) always uses the winit embedder: headless
automation works on any build, and headed automation on macOS needs this
feature (the AppKit backend is never used for automation).

### Individual packages

```bash
cargo build --release -p content --bin formal-web-content
cargo build --release -p net     --bin formal-web-net
cargo build --release -p embedder --bin formal-web-embedder
```

### Media / Graphics binary

```bash
# macOS: AVFoundation (default) — no special flags needed
cargo build --release -p media --bin formal-web-media

# macOS: GStreamer (opt-in)
cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-gstreamer

# Linux: GStreamer (only backend) — no special flags needed
cargo build --release -p media --bin formal-web-media

# Graphics process (composition + media backend). The surface backend is
# zero-copy IOSurface by default on macOS; build with `--features cpu_readback`
# for the CPU readback backend there. Off macOS the CPU readback backend is
# the only one.
cargo build --release -p graphics --bin formal-web-graphics
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

# GcCell Borrow Discipline

Never call an engine method (any `ec` operation) while a `GcCell` borrow
(`borrow`/`borrow_mut` guard) is live — shared or mutable.  See
`content/README.md` ("GcCell borrow discipline") for the mechanism, the
approved patterns, and the remaining exception class.

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

Git writes are forbidden here, so "revert and re-run" is not available.
Establish the baseline instead by (1) reading the committed version of
the exact code path the test exercises — `git show HEAD:<path>` is a
read of history and is allowed — and confirming whether the changeset
touched it, and (2) reproducing the failing behavior directly through
the CDP browser tools on a minimal page, so the observed cause is a
fact rather than an inference from the test result.  Report both.

# Spec Fidelity

- Keep README guidance general and durable; one-off implementation details belong in source or tests, not in repository docs.
- Use neutral, factual language.
- Use the `web_standards` extension tools (`spec_lookup`, `spec_ref_links`, `spec_search_id`) to read spec content instead of reading local copies or fetching directly. This is not a one-shot lookup — consult the spec **iteratively** as you write code: start by reading the algorithm to understand the structure, implement the corresponding code, then re-read the spec and compare each step against what you wrote. The spec is the source of truth for both the algorithm logic and the documentation annotations (`// Step N:`, anchor URLs, `// Note:` for discrepancies) that code must carry. The end-of-task spec-mapping review (step 4 below) is the final checkpoint that every algorithm in the changeset is consistently implemented and properly annotated.
- **Reference URLs vs canonical URLs.** In web standards, every definition (`#dfn-foo`) has corresponding reference links (`#ref-for-dfn-foo`, `#ref-for-dfn-foo①`, …) at each usage site. When documenting code that implements a specific algorithm step, prefer the *reference URL* over the canonical concept URL — your code implements "the thing as used in a particular algorithm", not the thing itself. Use `spec_ref_links` to find all reference URLs for a concept.
- Treat `vendor/` and vendored WPT resources as read-only unless the task explicitly requires vendor changes.
- The words "runtime", "sidecar", "carrier", "root", and "domain document" (or "domain_document") are forbidden in this repo.
- **Method doc comments:** A method that implements a spec algorithm has only the spec link as its doc comment — no `/// Note:` continuation above the method. All explanation, step references, and context belong in `//` comments inside the method body, as notes below the relevant steps.
- **Document only verified facts.** Never speculate about root causes, fixes, or
  explanations for observed behavior unless you have confirmed them through
  instrumentation, debugging, or testing.  When documenting an issue, state
  only what was observed, what was tried, and what was ruled out.  A statement
  like "this might be caused by X" is speculation unless X was verified.
  Prefer phrasing like "symptom: X works then crashes; Y was tried and failed;
  Z was not investigated" over "the issue is likely due to X".

# Dead Code and Comments

- **Never use `#[allow(dead_code)]` on fields or functions.** A field stored only for
  RAII cleanup must be explicitly used during shutdown (send shutdown signal, wait for
  acknowledgement, join the child process). Remove the dead code instead of annotating
  around it.
- **Remove unused bindings; never silence them with `_`-prefixed names or `let _ =`.**
  If a pattern field, parameter, or local variable is not used, delete it: elide
  unneeded struct/enum fields with `..` in the pattern, drop the binding or parameter,
  and remove any code that existed only to consume it. An unused binding kept alive as
  `_foo`, `_bar` or `let _ = ...` is dead code that silently accumulates — the warning
  is a request to delete the binding, not to rename it.
- **In code and doc comments, describe the code as it is now — never as it was,
  never against an earlier design.**  No migration or design-history narration:
  no "now comes from X instead of Y", "previously maintained by Z", "no
  cluster-side forwarding and no worker registry in the content process", "no
  longer protected".  Test a clause: if deleting it leaves the sentence
  complete and accurate, it only contrasts with history — delete it, and say
  what the code does: "the user agent routes port tasks directly to this thread
  over the agent's own channel" needs no tail.  Keep clauses that carry live
  state ("a worker this realm no longer owns — it closed — drops the message")
  or name the real external constraint they exist for (a spec requirement, a
  trait bound).  Rewrite or delete comments that are out of date.  The
  one comparison a comment may draw is a `// Note:` spec-discrepancy
  annotation: code against spec text, never against an earlier design.
- **No state-change framing or counterfactual prose.**  Name entities as they
  are at the point of the code — "the outgoing document", not "the previous
  document" — and state behavior without justifying it by what would happen if
  the code did something else ("reporting a child id would make the embedder
  request a redraw...").

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
- **Remove development-time tracing when the work lands.** Toggle-gated `debug!`/`trace!` logs added to trace a feature being implemented (e.g. per-message routing traces) must be removed once the feature works; they are not durable instrumentation.  Keep only toggles that serve ongoing diagnostics of a subsystem (e.g. `render-state`, `timer-debug`, `input-debug`).
- **Never use `let _ = ...`** to silence a `Result`. Every `Result` carries diagnostic
  information; silent discarding makes multi-process failures impossible to debug.
  Always use `if let Err(error) = fallible_operation() { error!("...: {error}"); }`.
  The error message must identify the operation.
- The only exception is IPC `send()` on reply channels (e.g. `reply.send(...)`, `waiter.send(...)`) where a closed receiver is an expected condition (client disconnected) rather than a system error.
- Avoid bare `.expect()` and `.unwrap()` on `Result` — prefer propagating the error with `?` or logging with `error!` and recovering.
- Use `.ok()` only when the `None`/`Err` case carries no diagnostic value (e.g. parsing an optional value from a fallible source where `None` is a valid "not present" signal).
- The `ConsoleSink::Stderr` variant in `content/src/js/bindings/console.rs` is exempt — it implements the browser Console API output destination, not error logging.

# README Documentation Policy

READMEs document the code as it is now: architecture, conventions, and work
still to be done.  They never document its history — past changes, completed
fixes, design iterations, session logs — and never frame the current design
against an earlier one.  The "describe the code as it is now" rule for
comments (under Dead Code and Comments) governs README prose as well: say
what exists, affirmatively; no "no X registry", "no cluster-side
forwarding", "no longer ...", "previously ..." contrasts.

A README tracks only:
- Things that **still need to be fixed** (unfixed bugs, pre-existing issues)
- **Dead-end investigations** for currently-unfixed issues, so future
  sessions know what was already tried and ruled out

Do NOT document:
- Completed fixes, design iterations, or session logs — they live in the
  code and git history
- Infrastructure descriptions for things that already work
- Anything already obvious from the code and its comments.  In particular,
  the step-by-step split of a spec algorithm across processes (which steps
  run in content, which in the user agent) is carried by the `// Step N:`
  comments and notes on each side — the README must not duplicate it.

The only past-tense content allowed is a note about a failed fix attempt for
a still-unfixed issue — the symptom, what was tried, and what was ruled out.
Once an issue is fixed, its notes are removed.  "Document only verified
facts" applies throughout.

# End-of-Task Flow

At the end of each task, run the following steps **in order**:

1. **Tear down browser/CDP infra** — Kill any formal-web processes
   (`pkill -f "formal-web"`)`, CDP servers, or other processes that
   were started during the session. Leftover processes can block ports and
   interfere with subsequent tasks.

2. **Remove session artifacts** — Delete any temporary test files,
   screenshots, test pages, or other debug artifacts created during the
   session under the repo root.  These are not part of the project and
   should not be committed.  Exception: artifacts placed under
   `scratchpad/` are intentional and may be kept.

3. **Run `cargo clippy`** — Lint the workspace (excluding vendor) and fix any
   warnings before committing. Run from the project root:

   ```bash
   rustup run 1.94.0 cargo clippy --workspace --all-targets
   ```

   Fix all warnings that appear (patch and vendored warnings can be ignored;
   focus on code-level warnings). The `vendor/` directory is excluded from
   this repository's scope and should not be linted or modified.

4. **Run `cargo fmt`** — Format the project's code before committing. Run
   from the project root: `cargo fmt`. This formats the workspace's own
   packages only — `vendor/` sub-crates are not workspace members and are
   never reformatted. Never run `cargo fmt` with `--all` or from inside a
   `vendor/` directory, as vendored formatting changes must not be committed.

5. **Spec-mapping review** — First, **re-read the documentation chain**
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
   - Is every anchor a claim that the item **is** that algorithm — never
     the anchor of an algorithm that merely calls the item or contains
     the step it runs?  Partial stand-ins for a sub-algorithm the spec
     calls into carry the sub-algorithm's own anchor with the missing
     parts in body `// Note:`s; plumbing carries no algorithm anchor.
   - Are binding function bodies free of fully qualified paths like
     `crate::wasm::namespace::fn_name(...)`?  Import with `use` at the
     top and call unqualified.
   - Are `Note:` comments used only for discrepancies between the code and
     the spec text (never for design notes, implementation plans, or
     architecture rationales — those belong in the README chain)?
   - Are dead or `#[allow(dead_code)]` items justified with a `// Note:` or
     `// TODO:` explaining the gap?
   Fix any issues found.

6. Think very hard about any general lessons learned in the session, and what parts of the documentation chain should be updated to reflect such general lessons, and then also update it. 

7. **Prune READMEs** — Strip completed fixes and historical session logs from
   the documentation chain. The README should track only remaining work and
   dead-end investigations for currently-unfixed issues (see "README
   Documentation Policy" above).

8. **Promote newly-passing WPT tests to the default selection** — When a
   WPT test was previously disabled or unselected and now passes, make that
   permanent so it runs as a standard from now on:
   - Remove its `disabled:` entry from `tests/wpt/meta/` (the metadata
     `disabled` reason is what keeps a test out of the default run — stale
     `disabled` entries silently hide new passes).
   - If the test lives in a directory that `tests/wpt/include.ini` does not
     select, add a `[path/to/test]` / `skip: false` entry so it is collected.
   - If a test cannot pass on every supported engine (V8 and Boa are the
     two that must pass; JSC is ignored) or on every feature build, keep it
     disabled and update its `disabled` reason to state exactly which
     dependency is missing — never leave a stale reason describing the old
     failure.
   - Update any README "known failures" entry for the fixed test (step 7).

9. **Run all verification steps** — Every end-of-task run executes ALL verification steps unconditionally. Do not skip any step based on a subjective assessment of "relevance" — changes to seemingly unrelated files (test pages, configuration, documentation) routinely break downstream steps in this multi-process system. Running everything catches regressions the agent cannot predict.

   Two engines run WPT: V8 (default) and Boa (opt-in). JSC is experimental
   (content crate compiles but `run_content_process` returns an error at
   runtime).  Default verification runs use the V8 backend (the default
   features); run Boa with `--no-default-features --features boa,media`.

   - **Default WPT run** —

     ```bash
     rustup run 1.94.0 cargo run --release -- wpt
     ```

     The WPT runner requires a working Python 3 with a functioning `ssl` module and `venv` support. If the run fails with a Python-related error, check `tests/wpt_runner/README.md` for debugging guidance.

   - **Spec verification** — Validates all TLA+ spec traces (Navigation, RenderingOpportunity, etc.) via the headless verification script (no GUI needed, fully automated):

     ```bash
     JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home ./verification/verify-specs.sh
     ```

     TLC runs on Java; set `JAVA_HOME` to a JDK home when the macOS `/usr/bin/java` stub cannot locate a JVM (see `verification/README.md`, "TLA+/TLC gotchas").

     The script starts the embedder headless with TLA+ tracing, runs a minimal WebDriver session, collects trace events, and validates them against TLA+ models.

10. **Suggest a commit message** — Whenever asked for a commit message (whether at end-of-task or any other time), propose a message for the current `git diff HEAD` (the uncommitted changes), not for the entire session's work.  Run `git diff --stat HEAD` to see what changed, and `git diff HEAD` to read the diff before writing the message.

11. Review the entire session (your entire context window) and make sure that Rule Number One was respected (see top of file), and if not alert the user.


# Forbidden commands

- Do not use Git except for reading history.
- Do not use scripts to edit source code.