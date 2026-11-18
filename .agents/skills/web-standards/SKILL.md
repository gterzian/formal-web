---
name: web-standards
description: Read exact text from WHATWG and W3C specifications by anchor id, find anchor ids by keyword, and list where a concept is referenced. Use whenever implementing, annotating, citing or reviewing code against a web standard (HTML, DOM, Fetch, Streams, URL, Web IDL, Infra, WebRTC), including every `// Step N:` comment and spec link.
compatibility: Requires network access to the spec hosts.
---

# web-standards

Use this skill to read spec text before writing, annotating or reviewing code
against a web standard. Fetched specs are cached on disk for two weeks.

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

**HTML Standard multipage URLs.** `https://html.spec.whatwg.org/multipage/`
is a table of contents and `/multipage/<section>.html` holds only that
section, so a fragment id is rarely on the page its URL names. Ids are
identical to the single-page rendering, so every multipage URL is read
against `https://html.spec.whatwg.org/` instead; annotate with whichever form
the file uses and look it up as written.

## Workflow

1. `search` for an anchor id when the exact id is unknown.  It matches ids
   only, not body text: a phrase that appears only in prose (e.g.
   "about:blank") returns no result even though the concept exists.  In that
   case try plausible id prefixes (`#matches-about:blank`, `#concept-...`),
   or read the section heading around a related definition and look that
   heading id up.
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

**Truncated section → read the reference instead.** Some algorithms are
rendered far from the heading they live under, so even `lookup` on the section
heading stops before the step list. `refs <spec-url> <id> --read <n>` reads the
reference site and prints the enclosing algorithms in full, step by step — that
is the reliable way to get the numbered steps of an algorithm whose anchor sits
inside another algorithm (e.g. `update-the-rendering` under the event loop
processing model).

**Step numbering is the spec's path, not the nesting depth.** Steps of an
algorithm nested inside another step carry the full dotted number
(`Step 3.14`, `Step 3.16.2.1`), and `verify` matches `// Step N:` comments
against exactly that. A comment numbered `// Step 14:` for a step the spec
numbers `3.14` fails verification.

## Annotation conventions

Annotation conventions live in `AGENTS.md` under "Algorithm Implementation"
and "Spec Fidelity". This skill only supplies the spec text.

## Verify annotations

`spec.mjs verify <path...>` scans Rust sources for spec URLs in doc comments
and `// Step N:` step comments. It checks that each anchor exists and that
each step comment matches the first words of the numbered spec step. Anchors
defined in prose (an IDL attribute or method, a concept) have no numbered
steps; they are only checked for existence. It exits non-zero when it finds a
mismatch.

```
node scripts/spec.mjs verify content/src/dom/event.rs
node scripts/spec.mjs verify content/src --refresh
```

## Boa API mapping

Read `references/boa-api-mapping.md` when implementing Web IDL or HTML
algorithms with Boa.
