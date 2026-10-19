---
name: web-standards
description: Read exact text from WHATWG and W3C specifications by anchor id, find anchor ids by keyword, and list where a concept is referenced. Use whenever implementing, annotating, citing or reviewing code against a web standard (HTML, DOM, Fetch, Streams, URL, Web IDL, Infra, WebRTC), including every `// Step N:` comment and spec link.
compatibility: Requires Node.js 22 or newer and network access to the spec hosts.
---

# web-standards

Spec text is the source of truth for both algorithm logic and the annotations
code must carry. Use this skill before writing, annotating or reviewing any
spec-implementing code, and again after writing it to compare each step.

Run the script from this skill directory (`scripts/spec.mjs`), or with the full
path from the repository root. Fetched specs are cached on disk for 24 hours.

## Commands

```
node scripts/spec.mjs lookup <spec-url> <id>
node scripts/spec.mjs refs   <spec-url> <id> [--read <n>|all]
node scripts/spec.mjs search <spec-url> <keyword> [--limit <n>]
node scripts/spec.mjs verify <path...>
```

The `lookup`, `refs` and `search` commands accept `--offset <n>` and
`--max-lines <n>` (default 400) for paging and `--refresh` to bypass the
cache. A truncated result ends with the `--offset` for the next page.

## Workflow

1. `search` for a keyword when the exact id is unknown.
2. `lookup` the id to read the definition, section or algorithm.
3. `refs` to list every usage site of a concept.
4. `refs ... --read <n>` to read the algorithm at a specific usage site.

Look up any cross-reference by its exact URL fragment. Split a full URL before
`#` as `<spec-url>` and after `#` as `<id>`.

**Truncated dfn → scroll to section.** A `<dfn>` is inline inside a `<p>`, so
`lookup` on it cannot reach the algorithm `<ol>` that is a sibling of the
`<p>`. When the result looks incomplete, read the `Section:` line and look up
that heading id next. For example, `queue-a-microtask` truncates; `queuing-tasks`
renders the full algorithm.

**Prefer reference URLs.** When documenting a step that uses a concept, use the
reference URL (`#ref-for-dfn-platform-object①`) over the canonical definition
URL (`#dfn-platform-object`): the code implements the thing as used in that
algorithm, not the concept in the abstract.

**Re-read iteratively.** Read the algorithm, implement it, then read it again
and compare every step. Do not rely on a single lookup.

## Annotation conventions

The conventions live in `AGENTS.md` under "Algorithm Implementation" and
"Spec Fidelity" — step comments, anchor URLs, `// Note:` for discrepancies,
the three-layer architecture, and the spec-mapping review. Follow them
exactly; this skill only supplies the spec text.

## Verify annotations

`spec.mjs verify <path...>` scans Rust sources for spec URLs in doc comments
and `// Step N:` step comments. It checks that each anchor exists and that
each step comment matches the first words of the numbered spec step. It exits
non-zero when it finds a mismatch.

```
node scripts/spec.mjs verify content/src/dom/event.rs
node scripts/spec.mjs verify content/src --refresh
```

## Boa API mapping

`references/boa-api-mapping.md` maps ECMAScript operations referenced by web
specs to Boa's Rust API, with the search methodology for locating an anchor in
Boa's source. Read it when implementing Web IDL or HTML algorithms with Boa.
