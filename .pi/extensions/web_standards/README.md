# web_standards — Spec Reading Extension

Lazily loads and caches web standards documents (WHATWG, W3C, etc.) so the agent can read spec content interactively during development without fetching the same resource twice. Specs are cached in memory for the lifetime of the pi session and cleared on shutdown.

## Tools

### `spec_lookup` (recommended entry point)

Look up any named anchor (dfn, heading, or element with an `id`) in a spec and return its rendered content. This is the recommended first tool to reach for when navigating a spec:

- **Definition (`dfn[id]`):** Returns the definition text, its parent section heading, and surrounding context.
- **Section heading (`h2[id]`, `h3[id]`, etc.):** Returns the full section content including all algorithm boxes with their top-level step structure — same behavior as `spec_section`.
- **Algorithm box (`div[data-algorithm]`):** Returns the algorithm header and step structure.

```
spec_lookup(url="https://html.spec.whatwg.org/", id="window-open-steps")
spec_lookup(url="https://html.spec.whatwg.org/", id="the-rules-for-choosing-a-navigable")
spec_lookup(url="https://html.spec.whatwg.org/", id="navigating-across-documents")
```

If you know a keyword but not the exact id, use `spec_search_id` first.

### `spec_search_id`

Search across all elements with an `id` attribute for a substring match. Returns a list of matching ids with their tag and first line of text. Use this to discover anchor IDs when you know a keyword:

```
spec_search_id(url="https://html.spec.whatwg.org/", query="window-open")
spec_search_id(url="https://html.spec.whatwg.org/", query="choosing-a-navigable")
```

### `spec_section`

Read a full section by anchor ID. Walks flat siblings from the heading to the next same-level heading. Detects algorithm boxes and renders their top-level step structure so you know what's available.

```
spec_section(url="https://html.spec.whatwg.org/", id="session-history-entries")
```

Prefer `spec_lookup` over `spec_section` for initial discovery — `spec_lookup` also handles non-heading anchors (dfn elements, algorithm boxes).

### `spec_algorithm`

Read numbered steps from an algorithm box. The HTML uses nested `<ol>` elements without step numbers — the browser renders them. This tool assigns numbers recursively (1, 1.1, 1.1.1, 1.2, 2, …) based on position. Supports `start`/`limit` pagination for long algorithms.

Find the algorithm either by `sectionId` (algorithm near that heading) or by `name` (matching the `data-algorithm` attribute). Many unnamed algorithm boxes have `data-algorithm=""` — use `sectionId` for those.

```
spec_algorithm(url="https://html.spec.whatwg.org/", sectionId="navigate")
spec_algorithm(url="https://dom.spec.whatwg.org/", name="queue-a-mutation-record")
```

### `spec_select`

Run a CSS selector against a spec document and return matched elements with their tag, id, and text. Accepts an optional `attrs` array to include extra attributes per match. Good for discovery:

- **Headings:** `h2[id],h3[id],h4[id],h5[id]`
- **Definitions:** `dfn[id]`
- **Algorithm boxes:** `div[data-algorithm]`

```
spec_select(url="https://html.spec.whatwg.org/", selector="h2[id],h3[id]", limit=20)
```

### `spec_html`

Return the inner HTML of the first element matching a CSS selector. Best for self-contained blocks: tables, definition lists (`<dl>`), example blocks. For algorithm boxes use `spec_algorithm` instead — it renders numbered steps.

```
spec_html(url="https://html.spec.whatwg.org/", selector="dl#domtokenlist")
```

## Commands

- **`/spec-loaded`** — Lists all spec URLs currently cached in memory.

## Workflow

1. **`spec_search_id`** — Find the exact id by searching a keyword.
2. **`spec_lookup`** — Read the anchor's content (definition, section, or algorithm).
3. **`spec_algorithm`** — Drill into an algorithm's numbered steps.

## Supported Specs

Any spec that serves a complete, queryable HTML document. Common targets:

| Spec | URL |
|------|-----|
| HTML | `https://html.spec.whatwg.org/` |
| DOM | `https://dom.spec.whatwg.org/` |
| Fetch | `https://fetch.spec.whatwg.org/` |
| Streams | `https://streams.spec.whatwg.org/` |
| URL | `https://url.spec.whatwg.org/` |
| Web IDL | `https://webidl.spec.whatwg.org/` |
| Infra | `https://infra.spec.whatwg.org/` |
| Console | `https://console.spec.whatwg.org/` |

## Implementation Notes

- Uses [cheerio](https://github.com/cheeriojs/cheerio) for server-side HTML parsing and traversal.
- Fetches with `Accept-Encoding: identity` to avoid gzip issues in the pi runtime's fetch implementation.
- Algorithm step numbering is computed by walking the nested `<ol>` structure — the HTML spec provides no step numbers in the markup.
- All downloaded spec HTML stays in memory for the session and is cleared on `session_shutdown`.
