import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import {
  truncateHead,
  DEFAULT_MAX_BYTES,
  DEFAULT_MAX_LINES,
} from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { load, type CheerioAPI, type Cheerio } from "cheerio";

// ── Truncation ────────────────────────────────────────────────────────────────

function truncate(text: string): string {
  const { content, truncated, outputLines, totalLines } = truncateHead(text, {
    maxBytes: DEFAULT_MAX_BYTES,
    maxLines: DEFAULT_MAX_LINES,
  });
  return content + (truncated ? `\n\n[Truncated: ${outputLines}/${totalLines} lines]` : "");
}

// ── Link extraction ───────────────────────────────────────────────────────────
// Collects all <a href="..."> elements within a cheerio selection, returning
// a deduplicated list of { text, href } pairs.

function collectLinks($: CheerioAPI, $root: Cheerio<any>): { text: string; href: string }[] {
  const seen = new Set<string>();
  const links: { text: string; href: string }[] = [];
  $root.find("a[href]").each((_, el) => {
    const $el = $(el);
    const href = $el.attr("href")?.trim() || "";
    if (!href || href.startsWith("#")) return; // skip internal anchors
    const text = $el.text().trim();
    if (!text) return;
    const key = `${href}::${text}`;
    if (seen.has(key)) return;
    seen.add(key);
    links.push({ text, href });
  });
  return links;
}

function formatLinkTable(links: { text: string; href: string }[]): string {
  if (links.length === 0) return "";
  const maxLen = Math.max(...links.map((l) => l.text.length), "Term".length);
  const header = `┌─ ${"Term".padEnd(maxLen)} ── ${"Link".padEnd(70)} ─┐`;
  const sep = `│${"".padEnd(maxLen + 74, "─")}│`;
  const rows = links.map(
    (l) => `│ ${l.text.padEnd(maxLen)}  ${l.href.padEnd(70)} │`
  );
  return ["", header, ...rows, sep].join("\n");
}

// ── Doc style reminder appended to every successful result ──────────────────────

const DOC_REMINDER = `

── Spec Doc Reminder ──
When implementing from this spec:
• Prefix each algorithm step with // Step N: <first words of spec step>
• Top doc comment on implementing functions/structs: spec anchor URL
• // Note: only for discrepancies between code and spec text
• Re-read the spec and compare against your code iteratively
`;

// ── Algorithm step rendering ──────────────────────────────────────────────────
// The HTML spec uses nested <ol> elements for algorithm steps. The <li> elements
// are NOT numbered in the HTML — the browser renders them. We assign numbers
// recursively based on position.

function renderAlgorithmSteps(
  $: CheerioAPI,
  $ol: Cheerio<any>,
  parentNum: string,
  links: { text: string; href: string }[]
): string[] {
  const lines: string[] = [];
  $ol.children("li").each((i, li) => {
    const $li = $(li);
    const num = i + 1;
    const fullNum = parentNum ? `${parentNum}.${num}` : `${num}`;

    // Collect links inside this step before cloning
    collectLinks($, $li).forEach((l) => {
      if (!links.some((existing) => existing.href === l.href && existing.text === l.text)) {
        links.push(l);
      }
    });

    // Get step text, excluding nested <ol> content and dfn panels.
    const $clone = $li.clone();
    $clone.find("ol").remove();
    $clone.find("div.dfn-panel").remove();
    let stepText = $clone.text().trim();

    lines.push(`  Step ${fullNum}: ${stepText}`);

    // Recurse into nested <ol>
    const $nestedOl = $li.children("ol");
    if ($nestedOl.length) {
      const nested = renderAlgorithmSteps($, $nestedOl, fullNum, links);
      lines.push(...nested);
    }
  });
  return lines;
}

// ── Shared sibling-walking logic ──────────────────────────────────────────────
// Walk forward siblings from a starting element, collecting algorithm boxes
// (with full recursive step rendering) and text content. Stops at:
//   - the next heading (h1-h6), optionally respecting a heading level threshold
//   - the next <dfn> with an id attribute (a new named definition)

function walkSiblingContent(
  $: CheerioAPI,
  startEl: Cheerio<any>,
  links: { text: string; href: string }[],
  stopAtLevel?: number
): string[] {
  const parts: string[] = [];
  let el = startEl.next();
  while (el.length) {
    const t = (el.prop("tagName") as string | undefined)?.toLowerCase();

    // Stop at the next heading.
    if (t && /^h[1-6]$/.test(t)) {
      if (stopAtLevel === undefined || parseInt(t[1]) <= stopAtLevel) break;
    }

    // Stop at the next named dfn (new definition boundary).
    if (t === "dfn" && el.attr("id")) break;

    // Collect links from this sibling
    collectLinks($, el).forEach((l) => {
      if (!links.some((existing) => existing.href === l.href && existing.text === l.text)) {
        links.push(l);
      }
    });

    if (t === "div" && el.attr("data-algorithm") !== undefined) {
      // Render algorithm box with full recursive step numbering.
      const algoName = el.attr("data-algorithm") || "(unnamed)";
      const $p = el.children("p").first();
      const $ol = el.children("ol").first();
      const header = $p.length ? $p.text().trim() : "";
      let algoBlock = `\n── Algorithm: ${algoName} ──\n${header}`;
      if ($ol.length) {
        const steps = renderAlgorithmSteps($, $ol, "", links);
        algoBlock += "\n" + steps.join("\n");
      }
      parts.push(algoBlock);
    } else {
      const text = el.text().trim();
      if (text) parts.push(text);
    }

    el = el.next();
  }
  return parts;
}

// ── Helper: find parent section heading ───────────────────────────────────────

function findParentSectionEl(
  $: CheerioAPI,
  el: Cheerio<any>
): { id: string; text: string } | undefined {
  let parent = el.parent();
  while (parent.length) {
    const pt = (parent.prop("tagName") as string).toLowerCase();
    if (/^h[1-6]$/.test(pt) && parent.attr("id")) {
      return { id: parent.attr("id")!, text: parent.text().trim() };
    }
    const prevId = parent.prevAll(`[id]`).first();
    if (
      prevId.length &&
      /^h[1-6]$/.test((prevId.prop("tagName") as string).toLowerCase())
    ) {
      return {
        id: prevId.attr("id")!,
        text: prevId.text().trim(),
      };
    }
    parent = parent.parent();
  }
  return undefined;
}

// ── Extension ─────────────────────────────────────────────────────────────────

export default function (pi: ExtensionAPI) {
  const docs = new Map<string, CheerioAPI>();

  async function getDoc(
    url: string,
    signal?: AbortSignal
  ): Promise<CheerioAPI> {
    if (docs.has(url)) return docs.get(url)!;
    const res = await fetch(url, {
      signal,
      headers: { "Accept-Encoding": "identity" },
    });
    if (!res.ok)
      throw new Error(`Failed to fetch ${url}: HTTP ${res.status}`);
    const html = await res.text();
    if (
      html.length > 0 &&
      html.charCodeAt(0) === 0x1f &&
      html.charCodeAt(1) === 0x8b
    ) {
      throw new Error(
        `Fetched ${url} returned gzip-compressed data but identity encoding was requested`
      );
    }
    const $ = load(html);
    docs.set(url, $);
    return $;
  }

  pi.on("session_shutdown", async (_event, _ctx) => {
    docs.clear();
  });

  // ────────────────────────────────────────────────────────────────────────────
  // spec_lookup — find by anchor ID
  // ────────────────────────────────────────────────────────────────────────────

  pi.registerTool({
    name: "spec_lookup",
    label: "Spec: Lookup ID",
    description:
      "Look up a named anchor (dfn, heading, or any element with an id) in a " +
      "spec and return its rendered content. " +
      "For any element type this walks forward siblings to show following " +
      "algorithm boxes (with full recursive step numbering) until the next " +
      "heading or named definition. " +
      "This is the only tool you need for reading spec content. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/, " +
      "https://fetch.spec.whatwg.org/, https://webidl.spec.whatwg.org/",
    promptSnippet: "Look up a spec element by its anchor ID",
    promptGuidelines: [
      "Use spec_lookup as the primary entry point for navigating a spec. " +
      "Pass the URL and the exact id value (e.g. 'window-open-steps' or " +
      "'the-rules-for-choosing-a-navigable'). The tool returns the element's " +
      "tag, its rendered content, and walks forward siblings to show algorithm " +
      "boxes and surrounding content. " +
      "For headings (h2[id],h3[id],etc.) the walk stops at the next same-level " +
      "heading. For other elements it stops at the next heading or named dfn. " +
      "Algorithm steps are rendered with full recursive numbering. " +
      "Use spec_search_id first if you need to find which id matches a keyword. " +
      "Use spec_ref_links to find every place a concept is referenced (including " +
      "all ref-for-foo, ref-for-foo①, etc. URLs).",
    ],
    parameters: Type.Object({
      url: Type.String({
        description:
          "Full URL of the spec, e.g. https://html.spec.whatwg.org/",
      }),
      id: Type.String({
        description:
          "The exact id attribute value to look up, e.g. 'window-open-steps' or 'navigate'",
      }),
    }),
    async execute(_toolCallId, { url, id }, signal) {
      const $ = await getDoc(url, signal);
      const el = $(`[id="${id}"]`).first();
      if (!el.length) {
        return {
          content: [
            {
              type: "text" as const,
              text: `No element found with id="${id}".`,
            },
          ],
          details: {},
        };
      }

      const tagName = (el.prop("tagName") as string).toLowerCase();
      const text = el.text().trim();

      // Collect links from the target element and all walked siblings
      const links: { text: string; href: string }[] = [];
      collectLinks($, el).forEach((l) => {
        if (!links.some((existing) => existing.href === l.href && existing.text === l.text)) {
          links.push(l);
        }
      });

      // For headings: walk siblings stopping at same-or-higher heading.
      if (/^h[1-6]$/.test(tagName)) {
        const level = parseInt(tagName[1]);
        const parts = [text, ...walkSiblingContent($, el, links, level)];
        const linkTable = formatLinkTable(links);
        return {
          content: [
            {
              type: "text" as const,
              text: truncate(parts.join("\n\n") + linkTable + DOC_REMINDER),
            },
          ],
          details: { url, id, tag: tagName },
        };
      }

      // For algorithm divs: render full steps directly.
      if (tagName === "div" && el.attr("data-algorithm") !== undefined) {
        const algoName = el.attr("data-algorithm") || "(unnamed)";
        const $p = el.children("p").first();
        const $ol = el.children("ol").first();
        const header = $p.length ? $p.text().trim() : "";
        let output = `Algorithm: ${algoName}\n${header}`;
        if ($ol.length) {
          const steps = renderAlgorithmSteps($, $ol, "", links);
          if (steps.length) {
            output += "\n" + steps.join("\n");
          }
        }
        const linkTable = formatLinkTable(links);
        return {
          content: [{ type: "text" as const, text: truncate(output + linkTable + DOC_REMINDER) }],
          details: { url, id, tag: tagName, algorithm: algoName },
        };
      }

      // For any other element: show the element context, then walk siblings.
      const siblingParts = walkSiblingContent($, el, links);
      const parentSection = findParentSectionEl($, el);
      const heading = parentSection
        ? `\nSection: ${parentSection.id}: ${parentSection.text}`
        : "";
      const parts = [
        `Element: <${tagName} id="${id}">` + heading + `\n\n${text}`,
        ...siblingParts,
      ];
      const linkTable = formatLinkTable(links);
      return {
        content: [{ type: "text" as const, text: truncate(parts.join("\n\n") + linkTable + DOC_REMINDER) }],
        details: { url, id, tag: tagName },
      };
    },
  });

  // ────────────────────────────────────────────────────────────────────────────
  // spec_search_id — find ids matching a keyword
  // ────────────────────────────────────────────────────────────────────────────

  // ────────────────────────────────────────────────────────────────────────────
  // spec_ref_links — find all reference links for a concept
  // ────────────────────────────────────────────────────────────────────────────
  //
  // In web standards, every <dfn id="foo"> has corresponding reference links
  // throughout the spec: <a id="ref-for-foo">, <a id="ref-for-foo①">, etc.
  // The circled-digit suffixes (① through ⑳ and compound variants like ①①)
  // distinguish multiple references to the same concept.

  function buildRefUrl(specBase: string, refId: string): string {
    // Percent-encode non-ASCII characters (circled digits) for the URL fragment.
    // encodeURI preserves all URL-safe ASCII chars while encoding non-ASCII.
    return `${specBase}#${encodeURI(refId)}`;
  }

  /**
   * Walk up the ancestor chain from a ref-for link, checking each ancestor
   * (and its preceding siblings) for the nearest enclosing heading or
   * algorithm box. This handles:
   *   - algorithm boxes via div[data-algorithm] or ol.algorithm
   *   - section headings as preceding siblings at any ancestor level
   *
   * The key insight: in WHATWG/W3C specs, ref-for links live in paragraphs
   * that are often direct children of <main>, with the section heading as a
   * *previous sibling* rather than a parent. We check every ancestor level
   * with prevAll for headings.
   */
  function findRefContext(
    $: CheerioAPI,
    $el: Cheerio<any>
  ): { type: string; name: string; sectionId?: string } {
    let ancestor = $el.parent();
    while (ancestor.length) {
      const tag = (ancestor.prop("tagName") as string).toLowerCase();
      // Check for algorithm containers: <div data-algorithm> or <ol class="algorithm">
      if (
        (tag === "div" && ancestor.attr("data-algorithm")) ||
        (tag === "ol" && ancestor.attr("class") === "algorithm")
      ) {
        const name =
          ancestor.attr("data-algorithm") ||
          ancestor.attr("aria-label") ||
          "(unnamed algorithm)";
        return { type: "algorithm", name, sectionId: ancestor.attr("id") };
      }
      // Find nearest preceding heading with an id (skipping non-heading elements).
      const prevHeading = ancestor.prevAll(
        "h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]"
      ).first();
      if (prevHeading.length) {
        return {
          type: "section",
          name: prevHeading.text().trim(),
          sectionId: prevHeading.attr("id"),
        };
      }
      ancestor = ancestor.parent();
    }
    return { type: "unknown", name: "(top-level)" };
  }

  /**
   * Same ancestor/preceding-sibling walk as findRefContext, but returns
   * the cheerio container node so the caller can render the full content.
   */
  function findRenderableContainer(
    $: CheerioAPI,
    $el: Cheerio<any>
  ): { container: Cheerio<any>; type: "algorithm" | "section"; sectionLevel?: number } | null {
    let ancestor = $el.parent();
    while (ancestor.length) {
      const tag = (ancestor.prop("tagName") as string).toLowerCase();
      // Check for algorithm containers.
      if (
        (tag === "div" && ancestor.attr("data-algorithm")) ||
        (tag === "ol" && ancestor.attr("class") === "algorithm")
      ) {
        return { container: ancestor, type: "algorithm" };
      }
      // Find nearest preceding heading with an id.
      const prevHeading = ancestor.prevAll(
        "h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]"
      ).first();
      if (prevHeading.length) {
        const level = parseInt((prevHeading.prop("tagName") as string)[1]);
        return {
          container: prevHeading,
          type: "section",
          sectionLevel: level,
        };
      }
      ancestor = ancestor.parent();
    }
    return null;
  }

  pi.registerTool({
    name: "spec_ref_links",
    label: "Spec: Reference Links",
    description:
      "Find all reference links (ref-for-* elements) for a given concept in a " +
      "spec. In web standards, every definition (e.g. #dfn-platform-object) " +
      "has corresponding reference links throughout the spec that show each " +
      "place the concept is used. These are tagged with id attributes like " +
      "ref-for-dfn-platform-object, ref-for-dfn-platform-object①, etc. " +
      "This tool returns each occurrence with its full URL and surrounding " +
      "context (algorithm name or section heading). " +
      "Use the `read` parameter to render the full content of specific " +
      "reference locations. " +
      "Example URLs: https://html.spec.whatwg.org/, https://webidl.spec.whatwg.org/",
    promptSnippet: "Find all reference links for a concept in a spec",
    promptGuidelines: [
      "Use spec_ref_links when you need to find every place a concept is referenced. " +
      "For example, to see all places 'platform object' is referenced in the Web IDL spec, " +
      'use spec_ref_links(url="https://webidl.spec.whatwg.org/", id="dfn-platform-object"). ' +
      "When documenting code that implements a specific algorithm step, prefer using the " +
      "reference URL over the canonical concept URL, because you are documenting " +
      "'the thing as used in a particular algorithm', not the thing itself.",
    ],
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec, e.g. https://html.spec.whatwg.org/",
      }),
      id: Type.String({
        description:
          "The canonical concept anchor id to search for references to, " +
          "e.g. 'dfn-platform-object' or 'concept-element-attributes'",
      }),
      read: Type.Optional(
        Type.Union([
          Type.Number({
            description:
              "Index of a specific reference to read (1-based). Renders the " +
              "algorithm box or section containing that reference.",
          }),
          Type.Literal("all", {
            description: "Render all reference locations.",
          }),
        ])
      ),
    }),
    async execute(_toolCallId, { url, id, read }, signal) {
      const $ = await getDoc(url, signal);

      // Find all reference links for this concept. Two patterns are supported:
      //   Pattern 1 (WHATWG single-page): <a id="ref-for-{id}">   (e.g. Web IDL, DOM, Fetch)
      //   Pattern 2 (HTML multi-source):  <a id="{section}:{id}" href="#{id}" >  (HTML spec)
      const refElements: { el: Cheerio<any>; id: string; index: number }[] = [];
      const seenIds = new Set<string>();

      // Pattern 1: ref-for-{id} (with optional circled-digit suffixes)
      const prefix = `ref-for-${id}`;
      $(`[id^="${prefix}"]`).each((_, el) => {
        const $el = $(el);
        const elId = $el.attr("id") || "";
        if (elId.startsWith(prefix) && !seenIds.has(elId)) {
          seenIds.add(elId);
          refElements.push({ el: $el, id: elId, index: 0 });
        }
      });

      // Pattern 2: {section}:{id} on <a href="#{id}"> (HTML spec style)
      $(`a[href="#${id}"]`).each((_, el) => {
        const $el = $(el);
        const elId = $el.attr("id") || "";
        // Match ids that end with ":{id}" (the section:concept pattern)
        if (elId.endsWith(`:${id}`) && !seenIds.has(elId)) {
          seenIds.add(elId);
          refElements.push({ el: $el, id: elId, index: 0 });
        }
      });

      if (refElements.length === 0) {
        return {
          content: [
            { type: "text" as const, text: `No reference links found for "${id}".` },
          ],
          details: {},
        };
      }

      // Assign final indices after dedup
      refElements.forEach((ref, i) => { ref.index = i + 1; });

      const specBase = url.replace(/#.*$/, "");

      // ── Read mode ──────────────────────────────────────────────────────
      if (read) {
        let indices: number[];
        if (read === "all") {
          indices = refElements.map((_, i) => i);
        } else {
          indices = [read - 1];
          if (indices[0] < 0 || indices[0] >= refElements.length) {
            return {
              content: [
                {
                  type: "text" as const,
                  text: `Index ${read} out of range. There are ${refElements.length} references.`,
                },
              ],
              details: {},
            };
          }
        }

        const blocks: string[] = [];
        for (const idx of indices) {
          const ref = refElements[idx];
          const refUrl = buildRefUrl(specBase, ref.id);
          const container = findRenderableContainer($, ref.el);

          if (!container) {
            blocks.push(
              `[${ref.index}/${refElements.length}] ${refUrl}\n(context not found)`
            );
            continue;
          }

          if (container.type === "algorithm") {
            const $algo = container.container;
            const algoTag = ($algo.prop("tagName") as string).toLowerCase();
            const algoName =
              $algo.attr("data-algorithm") ||
              $algo.attr("aria-label") ||
              "(unnamed)";
            const links: { text: string; href: string }[] = [];
            let output = `[${ref.index}/${refElements.length}] ${refUrl}\nAlgorithm: ${algoName}`;
            if (algoTag === "div") {
              // div[data-algorithm] — child <p> for header, child <ol> for steps
              const $p = $algo.children("p").first();
              const $ol = $algo.children("ol").first();
              const header = $p.length ? $p.text().trim() : "";
              if (header) output += "\n" + header;
              if ($ol.length) {
                const steps = renderAlgorithmSteps($, $ol, "", links);
                if (steps.length) output += "\n" + steps.join("\n");
              }
            } else {
              // ol.algorithm — the list itself contains the steps
              const steps = renderAlgorithmSteps($, $algo, "", links);
              if (steps.length) output += "\n" + steps.join("\n");
            }
            blocks.push(output);
          } else {
            // Section heading
            const $heading = container.container;
            const level = container.sectionLevel!;
            const links: { text: string; href: string }[] = [];
            const text = $heading.text().trim();
            const siblingParts = walkSiblingContent(
              $,
              $heading,
              links,
              level
            );
            blocks.push(
              `[${ref.index}/${refElements.length}] ${refUrl}\n${text}\n${siblingParts.join("\n\n")}`
            );
          }
        }

        return {
          content: [
            {
              type: "text" as const,
              text: truncate(blocks.join("\n\n---\n\n")),
            },
          ],
          details: {
            url,
            id,
            total: refElements.length,
            read: read === "all" ? refElements.length : indices.length,
          },
        };
      }

      // ── List mode ──────────────────────────────────────────────────────
      const rows = refElements.map((ref) => {
        const ctx = findRefContext($, ref.el);
        return {
          index: ref.index,
          url: buildRefUrl(specBase, ref.id),
          rawId: ref.id,
          context: `${ctx.type}: ${ctx.name}`,
        };
      });

      const urlMaxLen = Math.min(Math.max(...rows.map((r) => r.url.length), 40), 80);
      const ctxMaxLen = Math.min(
        Math.max(...rows.map((r) => r.context.length), "Context".length),
        60
      );

      const header =
        `┌─ # ── ${("Reference URL").padEnd(urlMaxLen)} ── ${("Context").padEnd(ctxMaxLen)} ─┐`;
      const sep = `│${("").padEnd(urlMaxLen + ctxMaxLen + 10, "─")}│`;

      const tableRows = rows.map((r) => {
        return `│ ${String(r.index).padStart(2)}  ${r.url.padEnd(urlMaxLen)}  ${r.context.padEnd(ctxMaxLen)} │`;
      });

      const table = ["", header, ...tableRows, sep].join("\n");

      const usage =
        `\n\nTo read the content of a specific reference, ` +
        `use this tool with read=<index> or read="all".\n\n` +
        `Documentation note: When documenting code that implements a specific ` +
        `algorithm step that references this concept, use the reference URL ` +
        `(e.g. #ref-for-${id}①) rather than the canonical concept URL ` +
        `(#${id}). Your code implements 'the thing as used in a particular ` +
        `algorithm', not the thing itself.`;

      return {
        content: [{ type: "text" as const, text: truncate(table + usage) }],
        details: {
          url,
          id,
          total: refElements.length,
        },
      };
    },
  });

  // ────────────────────────────────────────────────────────────────────────────
  // spec_search_id — find ids matching a keyword
  // ────────────────────────────────────────────────────────────────────────────

  pi.registerTool({
    name: "spec_search_id",
    label: "Spec: Search IDs",
    description:
      "Search a spec for all elements whose id attribute contains a given " +
      "substring. Returns a list of matches with their tag, id, and first line " +
      "of text. Use this to discover anchor IDs when you know a keyword but not " +
      "the exact id. Then use spec_lookup with the exact id to read the content. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/",
    promptSnippet: "Search spec IDs that match a keyword",
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec",
      }),
      query: Type.String({
        description:
          "Substring to search for in id attributes, e.g. 'navigat' or 'window-open'",
      }),
      limit: Type.Optional(
        Type.Number({
          description: "Maximum number of results to return (default 30)",
        })
      ),
    }),
    async execute(_toolCallId, { url, query, limit = 30 }, signal) {
      const $ = await getDoc(url, signal);
      const matches: { tag: string; id: string; text: string }[] = [];

      $(`[id]`).each((_, el) => {
        const $el = $(el);
        const id = $el.attr("id") || "";
        if (id.toLowerCase().includes(query.toLowerCase())) {
          const tag = (
            el.type === "tag" ? (el as { name: string }).name : ""
          ).toLowerCase();
          const text = $el.text().trim().slice(0, 120);
          matches.push({ tag, id, text });
        }
      });

      if (matches.length === 0) {
        return {
          content: [
            {
              type: "text" as const,
              text: `No ids matching "${query}" found.`,
            },
          ],
          details: {},
        };
      }

      const shown = matches.slice(0, limit);
      const note =
        matches.length > limit
          ? `\n[Showing ${limit} of ${matches.length} matches — refine your query for more precise results]`
          : "";

      const output = shown
        .map((m) => `#${m.id}\n  <${m.tag}> ${m.text}`)
        .join("\n\n");

      return {
        content: [{ type: "text" as const, text: truncate(output + note + DOC_REMINDER) }],
        details: {
          url,
          query,
          total: matches.length,
          returned: shown.length,
        },
      };
    },
  });
}
