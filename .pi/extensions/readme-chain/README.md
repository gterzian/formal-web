# readme-chain — Documentation chain reminder for pi

Ensures the agent consults the project's documentation chain (AGENTS.md →
nested README.md files) before editing source files.  Prevents the common
mistake of working on code without understanding the conventions that apply
to that part of the codebase.

## How it works

Every project using the [formal-web documentation chain](../../AGENTS.md)
convention has an `AGENTS.md` at the root and `README.md` files scattered
through the directory tree that record project-specific conventions,
architecture decisions, and implementation rules.

The `readme-chain` extension:

1. **Tracks** which directories have been "introduced" by having their
   README chain read — either explicitly via `readme_chain()` or implicitly
   when the agent reads a `README.md` or `AGENTS.md` file.

2. **Reminds** the agent when it tries to `edit`, `write`, or `read` a
   source file in a directory whose chain hasn't been consulted yet.
   The reminder fires once per prompt (resets on each new user turn) to
   avoid noise.

3. **Provides the `readme_chain` tool** — a custom LLM-callable tool that
   walks up the directory tree from a given file path, collects all
   `AGENTS.md` and `README.md` files, and returns their full contents.
   The agent is prompted to call this before editing files in unfamiliar
   directories.

4. **Provides `/readme-chain`** — a command for human use that summarises
   the chain without reading the full contents.

## What the chain means

The documentation chain for a file at `content/src/wasm/namespace.rs`
consists of:

```
AGENTS.md                          — project-wide rules and conventions
content/README.md                  — content crate overview
content/src/wasm/README.md         — wasm domain conventions
```

When the agent calls `readme_chain({ path: "content/src/wasm/namespace.rs" })`,
it gets all three files concatenated, in order from general to specific.
This ensures the agent understands:

- **AGENTS.md**: Three-layer architecture, step annotations, naming rules,
  error logging requirements, end-of-task flow.
- **content/README.md**: Crate layout, where domain vs bindings code lives.
- **content/src/wasm/README.md**: Module structure, domain/binding split
  specific to wasm, currently-working features, gaps.

## Commands

| Command | Description |
|---|---|
| `/readme-chain [path]` | Display the documentation chain for a file or directory |

## Tool

| Tool | Description |
|---|---|
| `readme_chain({ path?: string })` | Collect and return the full documentation chain for a path |

## Design notes

- **Per-session state only.** Extensions are reloaded on `/reload`, so the
  consulted set is reset. This is deliberate: the agent should re-check the
  chain after a reload.
- **Reminder fires once per turn.** `turn_start` resets the `notifiedOnce`
  flag, so each new prompt gets at most one reminder. This avoids spamming
  when the agent works through multiple files in sequence.
- **Vendor/config paths are ignored.** Files under `node_modules/`,
  `vendor/`, `target/`, `.pi/`, and `.git/` never trigger reminders.
- **Reading a README auto-consults.** When the agent reads a `README.md` or
  `AGENTS.md` file, that directory is automatically marked as consulted
  without needing a separate `readme_chain` call.
