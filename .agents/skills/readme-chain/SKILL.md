---
name: readme-chain
description: Read the README.md documentation chain for a file or directory, ordered from the project root to the most specific directory, without re-reading what is already in context. Use before editing or reviewing a file whose directory README chain has not been read in this session.
---

# readme-chain

The chain and its order are defined in `AGENTS.md` ("Documentation Chain").
This skill ships instructions only: no script runs, you build the chain
yourself and read the files with the `read` tool.

## Build the chain

1. Take the directory holding the target file (or the target directory).
2. Walk up to the project root: the nearest ancestor containing `AGENTS.md`.
3. Reverse that walk to get the `README.md` files from general to specific.
   `AGENTS.md` is not read here — pi loads it before the session starts.

A `README.md` under `vendor/` is not part of the chain. Where a directory has
no `README.md`, its guidance is in the nearest README above it.

## Do not duplicate

Your context already holds what you read earlier in this session; a second
copy adds tokens and no information.

- Read a file only if its content is not already in context. A `read` with
  `offset`/`limit`, or a result the `read` tool truncated, leaves part of the
  file out — re-read those in full before relying on them.
- The chain for a path stops at the first directory whose `README.md` you
  already read in full: everything above it is in context.
- Never re-read a README to re-confirm it. Re-read only what compaction
  dropped or what you read partially.
