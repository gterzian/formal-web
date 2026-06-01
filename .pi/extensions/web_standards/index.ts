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

// ── Algorithm step rendering ──────────────────────────────────────────────────
// The HTML spec uses nested <ol> elements for algorithm steps. The <li> elements
// are NOT numbered in the HTML — the browser renders the numbers. We assign
// numbers recursively based on position.

function renderAlgorithmSteps($: CheerioAPI, $ol: Cheerio<any>, parentNum: string): string[] {
  const lines: string[] = [];
  $ol.children("li").each((i, li) => {
    const $li = $(li);
    const num = i + 1;
    const fullNum = parentNum ? `${parentNum}.${num}` : `${num}`;

    // Get step text from direct <p> children only — excludes nested <ol> content
    const $directP = $li.children("p");
    let stepText: string;
    if ($directP.length) {
      stepText = $directP.map((_, p) => $(p).text().trim()).get().join(" ");
    } else {
      // Fallback: clone the li, strip nested <ol>, get remaining text
      const $clone = $li.clone();
      $clone.find("ol").remove();
      $clone.find("div.dfn-panel").remove();
      stepText = $clone.text().trim();
    }

    lines.push(`  Step ${fullNum}: ${stepText}`);

    // Recurse into nested <ol>
    const $nestedOl = $li.children("ol");
    if ($nestedOl.length) {
      const nested = renderAlgorithmSteps($, $nestedOl, fullNum);
      lines.push(...nested);
    }
  });
  return lines;
}

// Count total steps (recursively) in an algorithm's <ol>
function countAlgorithmSteps($: CheerioAPI, $ol: Cheerio<any>): number {
  let count = 0;
  $ol.children("li").each((_, li) => {
    count++;
    const $nestedOl = $(li).children("ol");
    if ($nestedOl.length) {
      count += countAlgorithmSteps($, $nestedOl);
    }
  });
  return count;
}

// Get clean step text from an <li> (excludes nested <ol> content)
function getStepTextSimple($: CheerioAPI, $li: Cheerio<any>): string {
  const $clone = $li.clone();
  $clone.find("ol").remove();
  $clone.find("div.dfn-panel").remove();
  return $clone.text().trim();
}

// ── Extension ─────────────────────────────────────────────────────────────────

export default function (pi: ExtensionAPI) {
  // ── Lazy doc cache ───────────────────────────────────────────────────────────
  // Scoped inside the factory so each extension instance owns its own cache.

  const docs = new Map<string, CheerioAPI>();

  async function getDoc(url: string, signal?: AbortSignal): Promise<CheerioAPI> {
    if (docs.has(url)) return docs.get(url)!;
    // Request identity encoding to avoid gzip compression issues in the pi runtime's fetch.
    const res = await fetch(url, { signal, headers: { "Accept-Encoding": "identity" } });
    if (!res.ok) throw new Error(`Failed to fetch ${url}: HTTP ${res.status}`);
    const html = await res.text();
    // Verify the response is valid HTML (not gzip-compressed bytes).
    if (html.length > 0 && html.charCodeAt(0) === 0x1f && html.charCodeAt(1) === 0x8b) {
      throw new Error(`Fetched ${url} returned gzip-compressed data but identity encoding was requested`);
    }
    const $ = load(html);
    docs.set(url, $);
    return $;
  }

  pi.on("session_shutdown", async (_event, _ctx) => {
    docs.clear();
  });

  // ── spec_section ─────────────────────────────────────────────────────────────
  // Reads a section by anchor ID. Walks flat siblings (WHATWG specs have no
  // wrapping <section> elements). Detects algorithm boxes and renders them
  // concisely with top-level step count, so you know what's available.

  pi.registerTool({
    name: "spec_section",
    label: "Spec: Read Section",
    description:
      "Read a spec section by its anchor ID. Finds the heading, then collects " +
      "all content up to the next same-or-higher-level heading. Algorithm boxes " +
      "are detected and rendered with their top-level step structure. Use " +
      "spec_algorithm to drill into a specific algorithm with full recursive " +
      "step numbering. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/, " +
      "https://fetch.spec.whatwg.org/, https://streams.spec.whatwg.org/, " +
      "https://url.spec.whatwg.org/, https://webidl.spec.whatwg.org/, " +
      "https://infra.spec.whatwg.org/, https://console.spec.whatwg.org/",
    promptSnippet: "Read a spec section by its anchor ID (detects algorithm boxes)",
    promptGuidelines: [
      "Use spec_section to read a section and discover what algorithms it contains. " +
      "When you see an algorithm box rendered with top-level steps, drill into it " +
      "with spec_algorithm using the sectionId or algorithm name to get full " +
      "recursive step numbering. Use spec_select to find section anchor IDs first.",
    ],
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec, e.g. https://html.spec.whatwg.org/",
      }),
      id: Type.String({
        description: "The section anchor ID, e.g. 'session-history-entries'",
      }),
    }),
    async execute(_toolCallId, { url, id }, signal) {
      const $ = await getDoc(url, signal);
      const heading = $(`[id="${id}"]`).first();
      if (!heading.length) {
        return {
          content: [{ type: "text" as const, text: `No element with id="${id}" found.` }],
          details: {},
        };
      }

      const tagName = (heading.prop("tagName") as string).toLowerCase();
      const level = parseInt(tagName[1]); // h3 -> 3, h5 -> 5, etc.
      const parts: string[] = [heading.text().trim()];

      // Walk flat siblings — WHATWG specs have no wrapping <section> elements.
      // Detect algorithm boxes and render them with structure instead of flat text.
      let el = heading.next();
      while (el.length) {
        const t = (el.prop("tagName") as string | undefined)?.toLowerCase();
        if (t && /^h[1-6]$/.test(t) && parseInt(t[1]) <= level) break;

        if (t === "div" && el.attr("data-algorithm") !== undefined) {
          // Render algorithm box concisely: header + top-level steps
          const algoName = el.attr("data-algorithm") || "(unnamed)";
          const $p = el.children("p").first();
          const $ol = el.children("ol").first();
          let algoBlock = `\n── Algorithm: ${algoName} ──\n${$p.text().trim()}`;
          if ($ol.length) {
            const totalSteps = countAlgorithmSteps($, $ol);
            // Show top-level steps only (numbered)
            $ol.children("li").each((i, li) => {
              const $li = $(li);
              const text = getStepTextSimple($, $li);
              algoBlock += `\n  Step ${i + 1}: ${text.slice(0, 200)}`;
            });
            if (totalSteps > $ol.children("li").length) {
              algoBlock += `\n  (${totalSteps} total steps including substeps — use spec_algorithm for full view)`;
            }
          }
          parts.push(algoBlock);
        } else {
          const text = el.text().trim();
          if (text) parts.push(text);
        }

        el = el.next();
      }

      return {
        content: [{ type: "text" as const, text: truncate(parts.join("\n\n")) }],
        details: { url, id },
      };
    },
  });

  // Step text helper (used by spec_section's algorithm rendering above)
  // Inline in the closure — see the anonymous function inside spec_section.

  // ── spec_algorithm ───────────────────────────────────────────────────────────
  // Read numbered steps from a spec algorithm box. Handles recursive <ol> nesting
  // and assigns step numbers (1, 1.1, 1.2, 2, 2.1.1, ...) since the HTML has no
  // numbering. Supports start/limit for paginating through long algorithms.

  pi.registerTool({
    name: "spec_algorithm",
    label: "Spec: Read Algorithm Steps",
    description:
      "Read numbered steps from a WHATWG spec algorithm box. The HTML uses " +
      "nested <ol> elements without step numbers — the browser renders them. " +
      "This tool assigns numbers recursively (1, 1.1, 1.1.1, 1.2, 2, ...) based " +
      "on position. Finds the algorithm either by sectionId (algorithm near that " +
      "anchor) or by name (matching the data-algorithm attribute). Supports " +
      "start/limit to paginate through long algorithms. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/, " +
      "https://fetch.spec.whatwg.org/, etc.",
    promptSnippet: "Read numbered algorithm steps with recursive numbering",
    promptGuidelines: [
      "Use spec_algorithm to read a spec algorithm with proper step numbering. " +
      "The HTML's nested <ol> structure is rendered as numbered steps (1, 1.1, " +
      "1.2, 2, ...). Pass sectionId to find the algorithm near a heading, or " +
      "name to match a specific data-algorithm attribute. Use start/limit to " +
      "page through long algorithms — each nested substep counts toward the limit. " +
      "Use spec_section first to discover what algorithms a section contains.",
    ],
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec",
      }),
      sectionId: Type.Optional(
        Type.String({
          description:
            "Section anchor ID to find the algorithm near. " +
            "Walks next siblings from the heading to find the first algorithm box.",
        })
      ),
      name: Type.Optional(
        Type.String({
          description:
            'Exact value of the data-algorithm attribute to match. ' +
            'E.g. "navigate", "finalize-a-cross-document-navigation". ' +
            'Many algorithm boxes have an empty data-algorithm="" — use sectionId for those.',
        })
      ),
      start: Type.Optional(
        Type.Number({
          description: "1-based step number to start from (default 1). " +
            "Nested steps count toward this — e.g. start=5 skips the first 4 steps " +
            "(including their substeps).",
        })
      ),
      limit: Type.Optional(
        Type.Number({
          description: "Maximum number of steps (including nested) to return (default 50).",
        })
      ),
    }),
    async execute(_toolCallId, { url, sectionId, name, start = 1, limit = 50 }, signal) {
      const $ = await getDoc(url, signal);
      let $algo: Cheerio<any>;

      if (name !== undefined) {
        // Match by data-algorithm attribute — handles empty string too
        $algo = $(`div[data-algorithm="${name}"]`).first();
        if (!$algo.length) {
          return {
            content: [
              {
                type: "text" as const,
                text: `No algorithm box found with data-algorithm="${name}".`,
              },
            ],
            details: {},
          };
        }
      } else if (sectionId !== undefined) {
        // Find the section heading, walk next siblings to find algorithm box
        const heading = $(`[id="${sectionId}"]`).first();
        if (!heading.length) {
          return {
            content: [
              {
                type: "text" as const,
                text: `No element with id="${sectionId}" found.`,
              },
            ],
            details: {},
          };
        }
        let el = heading.next();
        while (el.length && !el.is("div[data-algorithm]")) {
          el = el.next();
        }
        if (!el.length) {
          return {
            content: [
              {
                type: "text" as const,
                text: `No algorithm box found near section "${sectionId}".`,
              },
            ],
            details: {},
          };
        }
        $algo = el;
      } else {
        return {
          content: [
            {
              type: "text" as const,
              text: "Provide either sectionId or name to identify the algorithm box.",
            },
          ],
          details: {},
        };
      }

      // Get the algorithm header (parameter list)
      const $headerP = $algo.children("p").first();
      const header = $headerP.length ? $headerP.text().trim() : "(no header)";
      const algoName = $algo.attr("data-algorithm") || "(unnamed)";

      // Get the <ol> containing steps
      const $ol = $algo.children("ol").first();
      if (!$ol.length) {
        return {
          content: [
            {
              type: "text" as const,
              text: `Algorithm "${algoName}" has no steps (<ol> not found).`,
            },
          ],
          details: { algorithm: algoName, header },
        };
      }

      // Render all steps with recursive numbering
      const allSteps = renderAlgorithmSteps($, $ol, "");
      const total = allSteps.length;

      // Apply start/limit
      const fromIndex = Math.max(0, start - 1);
      const shownSteps = allSteps.slice(fromIndex, fromIndex + limit);
      const hasMore = fromIndex + limit < total;

      // Build output
      const label = algoName ? `Algorithm: ${algoName}` : "Algorithm";
      let output = `${label}\n${header}\n\nSteps (${total} total):\n`;
      output += shownSteps.join("\n");
      if (hasMore) {
        output += `\n\n[Showing steps ${start}-${Math.min(fromIndex + limit, total)} of ${total}. ` +
          `Use start=${fromIndex + limit + 1}&limit=${limit} to continue reading.]`;
      }

      return {
        content: [{ type: "text" as const, text: truncate(output) }],
        details: {
          url,
          algorithm: algoName,
          header: header.slice(0, 200),
          totalSteps: total,
          shownSteps: shownSteps.length,
          start,
          limit,
        },
      };
    },
  });

  // ── spec_select ──────────────────────────────────────────────────────────────

  pi.registerTool({
    name: "spec_select",
    label: "Spec: Select",
    description:
      "Run a CSS selector against a spec document and return matched elements " +
      "(tag, id, text, and optionally requested attributes). " +
      "Good for discovery and targeted queries. For reading a whole section, " +
      "prefer spec_section. For reading algorithm steps with numbering, " +
      "prefer spec_algorithm. " +
      "Key patterns: headings='h2[id],h3[id],h4[id],h5[id]'; " +
      "definitions='dfn[id]'; algorithm boxes='div[data-algorithm]'. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/, " +
      "https://fetch.spec.whatwg.org/, https://streams.spec.whatwg.org/, " +
      "https://url.spec.whatwg.org/, https://webidl.spec.whatwg.org/, " +
      "https://infra.spec.whatwg.org/, https://console.spec.whatwg.org/",
    promptSnippet: "Select elements from a spec using a CSS selector",
    promptGuidelines: [
      "Use spec_select to list headings, find definitions (dfn[id]), or locate " +
      "algorithm boxes (div[data-algorithm]) in a spec. For reading a section's " +
      "full content use spec_section. For reading numbered algorithm steps " +
      "use spec_algorithm.",
    ],
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec, e.g. https://html.spec.whatwg.org/",
      }),
      selector: Type.String({
        description:
          "CSS selector. Key patterns: 'h2[id],h3[id],h4[id],h5[id]' (headings), " +
          "'dfn[id]' (definitions), 'div[data-algorithm]' (algorithm boxes)",
      }),
      attrs: Type.Optional(
        Type.Array(Type.String(), {
          description: "Extra attributes to include per match, e.g. ['href', 'data-dfn-for']",
        })
      ),
      limit: Type.Optional(
        Type.Number({
          description: "Max matches to return (default 50)",
        })
      ),
    }),
    async execute(_toolCallId, { url, selector, attrs = [], limit = 50 }, signal) {
      const $ = await getDoc(url, signal);
      const $matches = $(selector);
      const total = $matches.length;
      const matches: object[] = [];

      $matches.slice(0, limit).each((_, el) => {
        const $el = $(el);
        const entry: Record<string, string | undefined> = {
          tag: el.type === "tag" ? (el as { name: string }).name : undefined,
          id: $el.attr("id"),
          text: $el.text().trim().slice(0, 2000) || undefined,
        };
        for (const attr of attrs) {
          entry[attr] = $el.attr(attr);
        }
        matches.push(entry);
      });

      const note = total > limit ? `\n[Showing ${limit} of ${total} matches]` : "";
      const text = JSON.stringify(matches, null, 2) + note;
      return {
        content: [{ type: "text" as const, text: truncate(text) }],
        details: { url, selector, total, returned: matches.length },
      };
    },
  });

  // ── spec_html ────────────────────────────────────────────────────────────────

  pi.registerTool({
    name: "spec_html",
    label: "Spec: Inner HTML",
    description:
      "Return the inner HTML of the first element matching a CSS selector. " +
      "For algorithm boxes use spec_algorithm instead — it renders numbered " +
      "steps from the nested <ol> structure. " +
      "Best for: definition lists ('dl'), tables, example blocks. " +
      "Same URLs as spec_section apply.",
    promptSnippet: "Get inner HTML of a spec element — tables, DLs, examples",
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec",
      }),
      selector: Type.String({
        description:
          "CSS selector — returns first match only. " +
          "E.g. 'table' for a table, 'dl' for a definition list.",
      }),
    }),
    async execute(_toolCallId, { url, selector }, signal) {
      const $ = await getDoc(url, signal);
      const el = $(selector).first();
      if (!el.length) {
        return {
          content: [{ type: "text" as const, text: `No element matched: ${selector}` }],
          details: {},
        };
      }
      const html = el.html() ?? "";
      return {
        content: [{ type: "text" as const, text: truncate(html) }],
        details: { url, selector },
      };
    },
  });

  // ── /spec-loaded ─────────────────────────────────────────────────────────────

  pi.registerCommand("spec-loaded", {
    description: "List all spec documents currently loaded in memory",
    handler: async (_args, ctx) => {
      if (docs.size === 0) {
        ctx.ui.notify("No specs loaded yet.", "info");
        return;
      }
      const lines = [...docs.keys()].map(url => `✓  ${url}`);
      ctx.ui.notify(lines.join("\n"), "info");
    },
  });
}
