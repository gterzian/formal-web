# rust-analyzer Pi Extension

Spawns [`rust-analyzer`](https://rust-analyzer.github.io/) as a child process
and communicates via LSP over stdio. Provides 15 agent-callable tools for
Rust code analysis, navigation, and refactoring.

## Requirements

- **rust-analyzer binary** on PATH, or set `RA_PATH` env var. The extension
  also checks common install locations (`~/.cargo/bin/`,
  `/opt/homebrew/bin/`).
- A Rust workspace (`Cargo.toml`) in or above the project root.

Install rust-analyzer:
```bash
rustup component add rust-analyzer
```

## Tool Reference

### ra_file_structure — File outline

Shows all symbols (structs, enums, functions, traits, impl blocks, modules)
defined in a file, with their line numbers. Cheaper than workspace symbol
search for understanding a single file.

```typescript
ra_file_structure({ file: "src/main.rs" })
// → Struct  Cli  (line 6)
//   Field   verify  (line 10)
//   Field   headless  (line 13)
//   Enum    CommandKind  (line 20)
//   …
//   Function  main  (line 140)
```

### ra_diagnostics — Errors, warnings, Clippy

Returns compiler errors, warnings, and Clippy lints for a Rust file. Faster
than `cargo check` for spot checks — no linking needed.

```typescript
ra_diagnostics({ file: "content/src/main.rs" })
// → [ERROR] content/src/main.rs:1:1
//     unresolved module, can't find module file (E0583)
```

### ra_hover — Type info and docs

Shows type information, documentation, and trait implementations for a symbol
at a given position (1-based line and column).

```typescript
ra_hover({ file: "automation/src/lib.rs", line: 88, character: 11 })
// → automation
//   pub trait AutomationHost
//   Is dyn-compatible
```

### ra_definition — Go to definition

Navigates to where a symbol is defined. Handles cross-crate references
including stdlib symbols.

```typescript
ra_definition({ file: "src/main.rs", line: 6, character: 8 })
// → src/main.rs:6:8
```

### ra_type_definition — Go to type's definition

Navigates to the definition of a value's *type* rather than the binding
site. For `let foo: MyStruct = bar()`, goes to `struct MyStruct`.

### ra_implementation — Find impl blocks

Finds all `impl MyTrait for …` or `impl MyStruct` blocks in the project.
Useful for finding all implementors of a trait.

```typescript
ra_implementation({ file: "automation/src/lib.rs", line: 88, character: 11 })
// → automation/src/lib.rs:121:1
//   automation/src/lib.rs:254:1
```

### ra_references — AST-aware cross-references

Finds all usages of a symbol across the entire workspace, including vendor
crates. Uses rust-analyzer's AST index — no false positives from strings
or comments like `grep`/`rg`.

```typescript
ra_references({ file: "content/src/main.rs", line: 324, character: 8 })
// → vendor/boa/utils/small_btree/src/lib.rs:31-31
//   tests/wpt_runner/src/lib.rs:42-42
//   user_agent/src/user_agent.rs:103-103
//   … (potentially hundreds of results)
```

Use `include_declaration: false` to exclude the definition site.

### ra_rename — Project-wide rename

Returns a `WorkspaceEdit` that renames a symbol across all files. Does NOT
write files — the agent reviews and applies the edits with the built-in
`write`/`edit` tools.

```typescript
ra_rename({ file: "src/main.rs", line: 6, character: 8, new_name: "Foo" })
// → Rename edits (apply with write/edit tool):
//   src/main.rs: 3 edit(s)
//   ...full WorkspaceEdit...
```

### ra_symbols — Fuzzy workspace search

Searches for functions, structs, enums, traits, and other symbols by name
across the workspace. Append `#` to search all symbol kinds; append `*` to
include dependencies.

```typescript
ra_symbols({ query: "Embedder" })
// → Interface  Embedder  webview/src/lib.rs:23
//   Interface  Embedder  user_agent/src/user_agent.rs:115
//   Struct     EmbeddedModuleEntry  vendor/boa/…/embedded.rs:30
```

### ra_inlay_hints — Inferred type annotations

Returns inferred types and parameter labels for a range of lines. Useful
for understanding what types the compiler infers without reading through
trait impls.

```typescript
ra_inlay_hints({ file: "src/main.rs", start_line: 50, end_line: 60 })
// → line 53:5  :Result<(), String>
```

### ra_expand_macro — Macro expansion

Fully expands a macro invocation (derive macros, proc macros, function-like
macros) and shows the generated code. Essential for understanding what
derive macros like `#[derive(Parser, Debug)]` actually produce.

```typescript
ra_expand_macro({ file: "src/main.rs", line: 6, character: 8 })
// → // Expansion of: derive(Parser, Debug)
//   impl clap::Parser for Cli { … }
```

### ra_code_actions — Available refactors and fixes

Lists quick-fixes, refactors, and assists available at a position. Includes:
add missing match arms, auto-import, fill struct fields, extract function,
inline variable, add derives, and more.

```typescript
ra_code_actions({ file: "src/main.rs", line: 20, character: 1 })
// → [0] Add #[derive(Debug)]  (quickfix)
//   [1] Add missing match arms  (refactor)
```

### ra_apply_action — Apply a code action

Applies a specific code action from `ra_code_actions` by index. Returns a
`WorkspaceEdit` for the agent to review and apply.

```typescript
ra_apply_action({ file: "src/main.rs", line: 20, character: 1, action_index: 0 })
```

### ra_ssr — Structural Search & Replace

Pattern-based refactoring across the workspace. Pattern: `before ==>> after`,
`$name` wildcards match any expression, type, or path. More reliable than
text-based search/replace for Rust code.

```typescript
ra_ssr({ query: "$x.clone() ==>> Arc::clone(&$x)", file: "src/main.rs" })
// → SSR matches:
//   src/main.rs: 2 edit(s)
```

### ra_call_hierarchy — Callers and callees

Shows who calls a function (incoming) and what it calls (outgoing). Better
than grepping for understanding control flow — it's AST-aware and handles
cross-crate calls.

```typescript
ra_call_hierarchy({ file: "src/main.rs", line: 53, character: 4 })
// → Function: run_embedder_process  (src/main.rs:53:1)
//   Incoming calls (3 caller(s)):
//     run_embedder_default    src/main.rs:86:1
//     run_embedder_webdriver  src/main.rs:97:1
//     run_embedder_cdp        src/main.rs:122:1
//   Outgoing calls (12 callee(s)):
//     new       …/std/src/process.rs:606
//     arg       …/std/src/process.rs:670
//     …
```

Use `direction: "incoming"` or `direction: "outgoing"` to filter.

## Configuration

The LSP server is configured via `initializationOptions` in the `initialize`
handshake. All settings are hard-coded in `index.ts` and optimized for
**fast first load during agentic workflows** — rapid edit-compile-test
cycles.

### Default Settings

| Setting | Value | Why |
|---|---|---|
| `checkOnSave` | `false` | Prevents `cargo check` from competing for Cargo.lock on every save |
| `cargo.buildScripts.rebuildOnSave` | `false` | Stops proc-macro / build script rebuilds when files change in quick succession |
| `cargo.autoreload` | `false` | Prevents re-running `cargo metadata` when `Cargo.toml` changes — especially noisy when an agent touches deps repeatedly |
| `cargo.allTargets` | `false` | Skips tests, benches, and examples during analysis — faster metadata and check runs |
| `numThreads` | `8` | More parallel indexing workers for large workspaces (~1 GB vendor code) |
| `cachePriming.numThreads` | `4` | Faster cache warm-up on project load |

No separate `targetDir` is set — RA shares the main `target/` directory
with its 6.4 GB of prebuilt artifacts. This means first load after a
restart is near-instant instead of compiling from scratch.

### Customizing

Users who want different defaults (e.g., enabling `checkOnSave` during
non-agentic editing) should edit the `initializationOptions` block in
`index.ts` and run `/ra-restart` to apply.

## Commands

| Command | Description |
|---------|-------------|
| `/ra-status` | Show whether rust-analyzer is connected and the project loading state |
| `/ra-restart` | Kill and re-spawn the LSP server (useful after `Cargo.toml` changes) |
| `/ra-wait` | Block until the project finishes loading. Use before tool calls on large workspaces |
| `/ra-loading-state` | Quick check: "loaded" or "loading..." |

## Project Loading

The formal-web project has a complex workspace with ~1 GB of vendor code
(blitz, anyrender, boa), edition 2024, and patched dependencies
build script. Rust-analyzer takes **1-2 minutes** to fully index on first
start after pi launches.

### Loading cycle

1. **Start**: `ra: loading...`
2. **During**: `ra: loading — 30s` / `ra: loading (fetching crate) — 45s`
   Progress info from stderr logging and `window/progress` notifications
3. **Ready**: `ra: ready`

### Behavior during loading

Tools do **not block** during loading. If `ra_file_structure` or
`ra_references` is called before the project is fully indexed, RA returns
what it has — potentially null or partial results. The tool reports the
data as-is rather than throwing an error. Call the tool again later once
the status shows `ra: ready`.

Every tool output is annotated with a loading status banner when the
project isn't fully loaded yet, for example:

```
[rust-analyzer loading: fetching crates — 15s]
<tool results...>
```

The agent can use this information to decide whether to retry or proceed
with partial results. Tools that would return empty results during loading
report the loading status instead of throwing a "not found" error.

### First start vs reloads

| Event | RA behavior |
|-------|-------------|
| `pi start` | Spawns RA, indexes from scratch |
| `/reload` | RA keeps running — reconnected via `globalThis` |
| `pi exit` | RA shut down cleanly via LSP `shutdown`/`exit` |
| `/ra-restart` | Kills RA, spawns fresh, re-indexes |
| `/new` / `/resume` / `/fork` | RA shut down, spawned fresh in new session |

## Architecture

### LSP transport

The extension spawns `rust-analyzer` as a child process and communicates over
stdio using the Language Server Protocol (LSP). Messages are framed with
`Content-Length: N\r\n\r\n` headers, JSON-RPC 2.0 body.

### Client lifecycle

The `RustAnalyzerClient` instance is stored on `globalThis` (not module-level),
so it survives pi's extension reload mechanism (jiti). On `/reload` the new
extension module retrieves the existing client, updates its status callback,
and resumes. On pi exit or session switches, `clearRaClient()` sends LSP
`shutdown` and `exit`, then nulls the reference.

### Readiness detection

After initialization, the client polls for readiness:

1. **First 30s** (15 attempts): `workspace/symbol` with query `"main"` —
   cheap if the project is small, returns `null` quickly if not done
2. **30s–5min** (attempts 16–150): opens `src/main.rs` and tries
   `textDocument/semanticTokens/full` and `textDocument/hover` —
   file-level operations that need only partial parsing
3. **5min+**: forces `_projectLoaded = true` — tools proceed and may
   get partial results

Additionally, a `window/progress` notification with `kind: "end"` marks the
project as ready immediately.

### Error handling

- `WorkspaceEdit`-returning tools (`ra_rename`, `ra_apply_action`, `ra_ssr`)
  never write files directly. They return the edit set for the agent to
  review and apply with the built-in `write`/`edit` tools.
- LSP request failures are caught and logged. Tools return whatever data
  RA provides (possibly null/empty) rather than throwing unhelpful errors.
- Stale RA processes (orphaned after a pi crash) are detected via `alive`
  getter on the client (`proc.exitCode === null && !proc.killed`) and
  replaced with a fresh spawn. Orphan processes are cleaned up with
  `pkill -f "rust-analyzer"` before starting a new one.

## Workflow Patterns

### Understanding unfamiliar code

1. `ra_file_structure` on the file — see what's defined
2. `ra_hover` on key types — understand their purpose
3. `ra_references` on a function — find all callers
4. `ra_call_hierarchy` — trace control flow
5. `ra_definition` on dependencies — jump to their crate

### Debugging compile errors

1. `ra_diagnostics` on the file — find errors without running `cargo check`
2. `ra_hover` on the error span — check types
3. `ra_code_actions` on the error — see available fixes
4. `ra_apply_action` to apply a fix

### Safe refactoring

1. `ra_references` on the symbol — understand impact before changing
2. `ra_rename` or `ra_ssr` — get the edit set
3. Review the `WorkspaceEdit` in the agent's response
4. Apply with `write`/`edit` tools
5. `ra_diagnostics` on affected files — verify no new errors

### Working with macros

1. `ra_expand_macro` on a `#[derive(...)]` or function-like macro — see
   what code it generates
2. Use the expanded code to understand trait requirements or generated APIs
