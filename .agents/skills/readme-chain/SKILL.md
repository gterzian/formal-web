---
name: readme-chain
description: Collect the README.md documentation chain for a file or directory, ordered from the project root to the most specific directory. Use before editing or reviewing any file in a directory whose README has not been read in this session.
---

# readme-chain

The documentation chain is every `README.md` between the project root and a
target path, from general to specific. Read the chain before editing a file in
a directory whose README has not been read in this session. Reading the chain
is always preferred over relying on memory.

`AGENTS.md` at the project root is always loaded and points to `SKILLS.md`,
which indexes this skill. The chain therefore starts at the first `README.md`
below the root.

## Command

```
node scripts/readme-chain.mjs [path]
node scripts/readme-chain.mjs [path] --list
node scripts/readme-chain.mjs [path] --include-root
```

With no path the script uses the current working directory.

- Default output is the chain summary followed by the contents of every
  `README.md` in the chain.
- `--list` prints only the chain summary.
- `--include-root` also includes the root `AGENTS.md` in the summary and
  contents.

The project root is the nearest ancestor containing `AGENTS.md`. Directories
named `node_modules`, `vendor`, `target`, `.pi`, `.agents` and `.git` are
skipped. The script keeps no state between runs; your own context records
which directories you have already read.
