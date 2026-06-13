# readme-chain — Documentation chain collector

Provides the `readme_chain` tool and `/readme-chain` command to collect a
project's documentation chain (AGENTS.md → nested README.md files) for a
given file path.  The tool helps the agent understand project conventions
before editing files.

## How it works

Every project using the [formal-web documentation chain](../../AGENTS.md)
convention has an `AGENTS.md` at the root and `README.md` files scattered
through the directory tree.

The `readme-chain` extension:

1. **Tracks** which directories have been "introduced" by having their
   README chain read — either explicitly via `readme_chain()` or implicitly
   when the agent reads a `README.md` or `AGENTS.md` file.

2. **Provides the `readme_chain` tool** — a custom LLM-callable tool that
   walks up the directory tree from a given file path, collects all
   `AGENTS.md` and `README.md` files, and returns their full contents.
   The agent is prompted to call this before editing files in unfamiliar
   directories.

3. **Provides `/readme-chain`** — a command for human use that summarises
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
  consulted set is reset.
- **Vendor/config paths are ignored.** Files under `node_modules/`,
  `vendor/`, `target/`, `.pi/`, and `.git/` never trigger auto-consult.
- **Reading a README auto-consults.** When the agent reads a `README.md` or
  `AGENTS.md` file, that directory is automatically marked as consulted
  without needing a separate `readme_chain` call.
