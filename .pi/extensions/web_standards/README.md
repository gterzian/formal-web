# web_standards — Spec Reading Extension

Lazily loads and caches web standards documents (WHATWG, W3C, etc.) so the agent can read spec content interactively during development without fetching the same resource twice. Specs are cached in memory for the lifetime of the pi session and cleared on shutdown.

## Tools

### `spec_section`

Read a full section by anchor ID. Walks flat siblings from the heading to the next same-level heading. Detects algorithm boxes and renders their top-level step structure so you know what's available.

```
spec_section(url="https://html.spec.whatwg.org/", id="session-history-entries")
```

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
