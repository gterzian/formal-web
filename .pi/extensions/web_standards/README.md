# web_standards — Spec Reading Extension

Lazily loads and caches web standards documents (WHATWG, W3C, etc.) so the agent can read spec content interactively during development without fetching the same resource twice. Specs are cached in memory for the lifetime of the pi session and cleared on `session_shutdown`.

Three tools — `spec_lookup`, `spec_ref_links`, and `spec_search_id`.

## Tools

### `spec_lookup` (find by anchor ID)

Look up any named anchor (`dfn`, heading, or element with an `id`) in a spec and return its rendered content. For any element type this walks forward siblings to show following algorithm boxes (with full recursive step numbering) and surrounding content, stopping at the next heading or named definition.

```
spec_lookup(url="https://html.spec.whatwg.org/", id="window-open-steps")
spec_lookup(url="https://html.spec.whatwg.org/", id="the-rules-for-choosing-a-navigable")
spec_lookup(url="https://html.spec.whatwg.org/", id="navigating-across-documents")
```

Because every spec anchor (heading, dfn, span, `a`) matches an `id` attribute in the HTML, you can look up any cross-reference by its exact URL fragment. No need to distinguish element types — just pass the id.

- **Headings:** Walks to the next same-or-higher-level heading, rendering all algorithm boxes and text in between.
- **Definitions (`dfn[id]`):** Shows definition context + parent section + forward siblings until the next heading or named `dfn[id]`.
- **Algorithm boxes (`div[data-algorithm]`):** Renders the algorithm header and full recursive step numbering.
- **All other elements:** Same as definitions — shows the element context and forward content.

Algorithm steps are rendered with recursive step numbers (1, 1.1, 1.1.1, 1.2, 2, …). The HTML spec provides no step numbers in the markup — they are computed from the nested `<ol>` structure.

**Cross-reference table.** When the content contains spec cross-references (terms linked to other sections or specs), a table is appended at the bottom:

```
┌─ Term           ── Link                                                        ─┐
│ boolean         https://webidl.spec.whatwg.org/#idl-boolean                     │
│ CSSOMString     https://drafts.csswg.org/cssom-1/#cssomstring                   │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Each row pairs a term name with the anchor URL where it's defined. You can look up any of these with another `spec_lookup` call — split the URL before `#` as `url=` and after `#` as `id=`, e.g. `spec_lookup(url="https://webidl.spec.whatwg.org/", id="idl-boolean")`. This lets you follow the spec's dependency chain across specs step by step.

### `spec_ref_links` (find reference links for a concept)

Find all places a concept is referenced throughout a spec. In web standards, every
definition (`<dfn id="dfn-foo">`) has corresponding reference links
(`<a id="ref-for-dfn-foo">`, `<a id="ref-for-dfn-foo①">`, …) at each usage site.
The numbered suffixes (①, ②, … ⑳, and compound variants like ①①) distinguish
multiple references to the same concept.

```
spec_ref_links(url="https://webidl.spec.whatwg.org/", id="dfn-platform-object")
spec_ref_links(url="https://html.spec.whatwg.org/", id="concept-element-attributes")
```

**List mode** (default) shows a table of all references with their full URL and
context (enclosing algorithm or section):

```
┌─ # ── Reference URL                                               ── Context                                     ─┐
│  1  https://webidl.spec.whatwg.org/#ref-for-dfn-platform-object     algorithm: determine the type of a platform object │
│  2  https://webidl.spec.whatwg.org/#ref-for-dfn-platform-object①   algorithm: platform object -> legacy platform  │
│ ...                                                                ...                                               │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Read mode** renders the actual content (algorithm steps or section) at each
reference location. Pass `read=<index>` (1-based) or `read="all"`:

```
spec_ref_links(url="https://webidl.spec.whatwg.org/", id="dfn-platform-object", read=1)
spec_ref_links(url="https://webidl.spec.whatwg.org/", id="dfn-platform-object", read="all")
```

**Documentation convention.** When documenting code that implements a specific
algorithm step that references a concept, use the *reference URL*
(e.g. `#ref-for-dfn-platform-object①`) over the *canonical concept URL*
(`#dfn-platform-object`). Your code implements "the thing as used in a particular
algorithm", not the thing itself. The reference URL captures exactly which usage
site you are implementing.

### `spec_search_id` (find ids by keyword)

Search across all elements with an `id` attribute for a substring match. Returns a list of matching ids with their tag and first line of text. Use this to discover anchor IDs when you know a keyword but not the exact id.

```
spec_search_id(url="https://html.spec.whatwg.org/", query="window-open")
spec_search_id(url="https://html.spec.whatwg.org/", query="choosing-a-navigable")
```

Then use `spec_lookup` with the exact id to read the content.

## Workflow

1. **`spec_search_id`** — Find the exact id by searching a keyword.
2. **`spec_lookup`** — Read an anchor's content (definition, section, or algorithm).
3. **`spec_ref_links`** — Find every place a concept is referenced.
4. **`spec_ref_links` + `read`** — Read the specific algorithm that uses the concept.

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
- `spec_ref_links` matches the `ref-for-{id}` prefix on any element; the WHATWG spec uses `<a>` elements with this pattern. Circled-digit suffixes (U+2460–U+2473, compound) distinguish multiple references.
- URL fragments use `encodeURI` to percent-encode non-ASCII characters (circled digits) for correct browser navigation.
