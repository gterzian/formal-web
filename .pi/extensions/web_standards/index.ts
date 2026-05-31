import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import {
  truncateHead,
  DEFAULT_MAX_BYTES,
  DEFAULT_MAX_LINES,
} from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { load, type CheerioAPI } from "cheerio";

// ── Truncation ────────────────────────────────────────────────────────────────

function truncate(text: string): string {
  const { content, truncated, outputLines, totalLines } = truncateHead(text, {
    maxBytes: DEFAULT_MAX_BYTES,
    maxLines: DEFAULT_MAX_LINES,
  });
  return content + (truncated ? `\n\n[Truncated: ${outputLines}/${totalLines} lines]` : "");
}

// ── Extension ─────────────────────────────────────────────────────────────────

export default function (pi: ExtensionAPI) {
  // ── Lazy doc cache ───────────────────────────────────────────────────────────
  // Scoped inside the factory so each extension instance owns its own cache.

  const docs = new Map<string, CheerioAPI>();

  async function getDoc(url: string, signal?: AbortSignal): Promise<CheerioAPI> {
    if (docs.has(url)) return docs.get(url)!;
    const res = await fetch(url, { signal });
    if (!res.ok) throw new Error(`Failed to fetch ${url}: HTTP ${res.status}`);
    const $ = load(await res.text());
    docs.set(url, $);
    return $;
  }

  pi.on("session_shutdown", async (_event, _ctx) => {
    docs.clear();
  });

  // ── spec_section ─────────────────────────────────────────────────────────────

  pi.registerTool({
    name: "spec_section",
    label: "Spec: Read Section",
    description:
      "Read the full text of a spec section by its anchor ID. Finds the heading with " +
      "that ID, then collects all content up to the next same-or-higher-level heading. " +
      "Use this to read a section's prose, definition lists, and algorithm steps. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/, " +
      "https://fetch.spec.whatwg.org/, https://streams.spec.whatwg.org/, " +
      "https://url.spec.whatwg.org/, https://webidl.spec.whatwg.org/, " +
      "https://infra.spec.whatwg.org/, https://console.spec.whatwg.org/",
    promptSnippet: "Read a WHATWG spec section by its anchor ID",
    promptGuidelines: [
      "Use spec_section when you need to read a specific section of a WHATWG spec. " +
      "Pass the section's anchor ID (e.g. 'session-history-entries', 'navigate'). " +
      "Use spec_select to discover IDs first if needed.",
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

      // WHATWG specs use flat siblings, not nested <section> elements.
      // Walk .next() siblings until we hit a heading of equal or higher level.
      let el = heading.next();
      while (el.length) {
        const t = (el.prop("tagName") as string | undefined)?.toLowerCase();
        if (t && /^h[1-6]$/.test(t) && parseInt(t[1]) <= level) break;
        const text = el.text().trim();
        if (text) parts.push(text);
        el = el.next();
      }

      return {
        content: [{ type: "text" as const, text: truncate(parts.join("\n\n")) }],
        details: { url, id },
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
      "prefer spec_section. " +
      "Key patterns: headings='h2[id],h3[id],h4[id],h5[id]'; " +
      "definitions='dfn[id]'; algorithm steps='div[data-algorithm] ol > li'. " +
      "Example URLs: https://html.spec.whatwg.org/, https://dom.spec.whatwg.org/, " +
      "https://fetch.spec.whatwg.org/, https://streams.spec.whatwg.org/, " +
      "https://url.spec.whatwg.org/, https://webidl.spec.whatwg.org/, " +
      "https://infra.spec.whatwg.org/, https://console.spec.whatwg.org/",
    promptSnippet: "Select elements from a WHATWG spec using a CSS selector",
    promptGuidelines: [
      "Use spec_select to list headings, find definitions (dfn[id]), or query " +
      "algorithm steps (div[data-algorithm] ol > li) in a WHATWG spec. " +
      "Use spec_section instead when you want to read a section's full content.",
    ],
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec, e.g. https://html.spec.whatwg.org/",
      }),
      selector: Type.String({
        description:
          "CSS selector. Key patterns: 'h2[id],h3[id],h4[id],h5[id]' (headings), " +
          "'dfn[id]' (definitions), 'div[data-algorithm] ol > li' (algorithm steps)",
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
          // 2000 chars: enough for a full algorithm step or paragraph
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
      "Best for self-contained blocks: algorithm boxes ('div[data-algorithm]'), " +
      "definition lists ('dl'), tables. For narrative sections use spec_section instead. " +
      "Same URLs as spec_section apply.",
    promptSnippet: "Get inner HTML of a spec element — best for algorithm boxes and tables",
    parameters: Type.Object({
      url: Type.String({
        description: "Full URL of the spec",
      }),
      selector: Type.String({
        description:
          "CSS selector — returns first match only. " +
          "E.g. 'div[data-algorithm]' for an algorithm box, 'table' for a table.",
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
