# readme-chain — Documentation chain collector

Provides the `readme_chain` tool and `/readme-chain` command to collect a
project's documentation chain (nested README.md files) for a given file path.
The tool helps the agent understand project conventions before editing files,
without re-sending README content the model has already seen.

## How it works

Every project using the [formal-web documentation chain](../../../AGENTS.md)
convention has a single `AGENTS.md` at the root and `README.md` files
scattered through the directory tree.

The `readme-chain` extension:

1. **Collects the chain** — walks up the directory tree from the given path,
   gathering every `README.md` on the way to the root.

2. **Filters what is already in context** — inspects the model's active
   conversation context (the session entries that survive compaction) and
   returns only the READMEs that are not already present.  A README counts as
   present when a previous `readme_chain` result included it, or when it was
   read in full with the `read` tool.  Partial or truncated reads do not
   count.

3. **Provides `/readme-chain`** — a command for human use that summarises the
   chain, marking the files read previously.

## What the chain means

The documentation chain for a file at `content/src/wasm/namespace.rs`
consists of:

```
content/README.md                  — content crate overview
content/src/wasm/README.md         — wasm domain conventions
```

`AGENTS.md` is not part of the returned chain.  The project keeps exactly one
`AGENTS.md`, at the root (see [`AGENTS.md`](../../../AGENTS.md), "Documentation
Chain"), and pi loads it as project instructions before the session starts.

When the agent calls `readme_chain({ path: "content/src/wasm/namespace.rs" })`,
it gets both README files concatenated, in order from general to specific.
Any README that was read previously is still listed in the summary — marked
`_(read previously — content omitted)_` — but its content is left out of the
returned text.

## Commands

| Command | Description |
|---|---|
| `/readme-chain [path]` | Display the documentation chain for a file or directory, marking files read previously |

## Tool

| Tool | Description |
|---|---|
| `readme_chain({ path?: string })` | Collect the documentation chain for a path and return the READMEs not already in context |

## Design notes

- **Context is derived from the session, not tracked separately.** The
  "already in context" set is rebuilt from `ctx.sessionManager` on every call,
  so it follows compaction and session changes automatically.  A README
  dropped by compaction is offered again.
- **The tool result carries `details.readmes`.** The absolute paths of the
  READMEs a call delivered are recorded there so later calls can detect them
  exactly.  Session results recorded before this payload existed are detected
  by parsing the rendered section headers.
