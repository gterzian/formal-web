import type { Cheerio, CheerioAPI } from "cheerio";

export interface SpecLink {
  text: string;
  href: string;
}

export interface RefContext {
  type: string;
  name: string;
  sectionId?: string;
}

export interface RenderableContainer {
  container: Cheerio<any>;
  type: "algorithm" | "section";
  sectionLevel?: number;
}

export const DOC_REMINDER = `

── Spec Doc Reminder ──
When implementing from this spec:
• Prefix each algorithm step with // Step N: <first words of spec step>
• Top doc comment on implementing functions/structs: spec anchor URL
• // Note: only for discrepancies between code and spec text
• Re-read the spec and compare against your code iteratively
`;

/** Collect all external links within a selection, deduplicated by href and text. */
export function collectLinks(
  $: CheerioAPI,
  $root: Cheerio<any>
): SpecLink[] {
  const seen = new Set<string>();
  const links: SpecLink[] = [];
  $root.find("a[href]").each((_index, element) => {
    const $element = $(element);
    const href = $element.attr("href")?.trim() || "";
    if (!href || href.startsWith("#")) return;
    const text = $element.text().trim();
    if (!text) return;
    const key = `${href}::${text}`;
    if (seen.has(key)) return;
    seen.add(key);
    links.push({ text, href });
  });
  return links;
}

/** Merge links into an existing list without duplicating href/text pairs. */
export function mergeLinks(target: SpecLink[], incoming: SpecLink[]): void {
  for (const link of incoming) {
    if (
      !target.some(
        (existing) => existing.href === link.href && existing.text === link.text
      )
    ) {
      target.push(link);
    }
  }
}

export function formatLinkTable(links: SpecLink[]): string {
  if (links.length === 0) return "";
  const maxLen = Math.max(...links.map((link) => link.text.length), "Term".length);
  const header = `┌─ ${"Term".padEnd(maxLen)} ── ${"Link".padEnd(70)} ─┐`;
  const separator = `│${"".padEnd(maxLen + 74, "─")}│`;
  const rows = links.map(
    (link) => `│ ${link.text.padEnd(maxLen)}  ${link.href.padEnd(70)} │`
  );
  return ["", header, ...rows, separator].join("\n");
}

// ── Algorithm step rendering ──────────────────────────────────────────────────
// The HTML spec uses nested <ol> elements for algorithm steps. The <li> elements
// are NOT numbered in the HTML — the browser renders them. We assign numbers
// recursively based on position.

export function renderAlgorithmSteps(
  $: CheerioAPI,
  $orderedList: Cheerio<any>,
  parentNumber: string,
  links: SpecLink[]
): string[] {
  const lines: string[] = [];
  $orderedList.children("li").each((index, listItem) => {
    const $listItem = $(listItem);
    const number = index + 1;
    const fullNumber = parentNumber
      ? `${parentNumber}.${number}`
      : `${number}`;

    mergeLinks(links, collectLinks($, $listItem));

    // Get step text, excluding nested <ol> content and dfn panels.
    const $clone = $listItem.clone();
    $clone.find("ol").remove();
    $clone.find("div.dfn-panel").remove();
    const stepText = $clone.text().trim();

    lines.push(`  Step ${fullNumber}: ${stepText}`);

    // Recurse into nested <ol>
    const $nestedList = $listItem.children("ol");
    if ($nestedList.length) {
      const nested = renderAlgorithmSteps($, $nestedList, fullNumber, links);
      lines.push(...nested);
    }
  });
  return lines;
}

/** Render a `div[data-algorithm]` or `ol.algorithm` container. */
export function renderAlgorithmContainer(
  $: CheerioAPI,
  $algorithm: Cheerio<any>,
  links: SpecLink[]
): string {
  const algorithmTag = ($algorithm.prop("tagName") as string).toLowerCase();
  const algorithmName =
    $algorithm.attr("data-algorithm") ||
    $algorithm.attr("aria-label") ||
    "(unnamed)";
  let output = `Algorithm: ${algorithmName}`;

  if (algorithmTag === "div") {
    // div[data-algorithm] — child <p> for header, child <ol> for steps
    const $paragraph = $algorithm.children("p").first();
    const $orderedList = $algorithm.children("ol").first();
    const header = $paragraph.length ? $paragraph.text().trim() : "";
    if (header) output += `\n${header}`;
    if ($orderedList.length) {
      const steps = renderAlgorithmSteps($, $orderedList, "", links);
      if (steps.length) output += `\n${steps.join("\n")}`;
    }
  } else {
    // ol.algorithm — the list itself contains the steps
    const steps = renderAlgorithmSteps($, $algorithm, "", links);
    if (steps.length) output += `\n${steps.join("\n")}`;
  }

  return output;
}

// ── Shared sibling-walking logic ──────────────────────────────────────────────
// Walk forward siblings from a starting element, collecting algorithm boxes
// (with full recursive step rendering) and text content. Stops at:
//   - the next heading (h1-h6), optionally respecting a heading level threshold
//   - the next <dfn> with an id attribute (a new named definition)

export function walkSiblingContent(
  $: CheerioAPI,
  startElement: Cheerio<any>,
  links: SpecLink[],
  stopAtLevel?: number
): string[] {
  const parts: string[] = [];
  let element = startElement.next();
  while (element.length) {
    const tag = (element.prop("tagName") as string | undefined)?.toLowerCase();

    // Stop at the next heading.
    if (tag && /^h[1-6]$/.test(tag)) {
      if (stopAtLevel === undefined || parseInt(tag[1]) <= stopAtLevel) break;
    }

    // Stop at the next named dfn (new definition boundary).
    if (tag === "dfn" && element.attr("id")) break;

    mergeLinks(links, collectLinks($, element));

    if (tag === "div" && element.attr("data-algorithm") !== undefined) {
      // Render algorithm box with full recursive step numbering.
      const algorithmName = element.attr("data-algorithm") || "(unnamed)";
      const $paragraph = element.children("p").first();
      const $orderedList = element.children("ol").first();
      const header = $paragraph.length ? $paragraph.text().trim() : "";
      let algorithmBlock = `\n── Algorithm: ${algorithmName} ──\n${header}`;
      if ($orderedList.length) {
        const steps = renderAlgorithmSteps($, $orderedList, "", links);
        algorithmBlock += `\n${steps.join("\n")}`;
      }
      parts.push(algorithmBlock);
    } else {
      const text = element.text().trim();
      if (text) parts.push(text);
    }

    element = element.next();
  }
  return parts;
}

/** Find the nearest enclosing section heading for an element. */
export function findParentSectionEl(
  $: CheerioAPI,
  element: Cheerio<any>
): { id: string; text: string } | undefined {
  let parent = element.parent();
  while (parent.length) {
    const parentTag = (parent.prop("tagName") as string).toLowerCase();
    if (/^h[1-6]$/.test(parentTag) && parent.attr("id")) {
      return { id: parent.attr("id")!, text: parent.text().trim() };
    }
    const previousId = parent.prevAll("[id]").first();
    if (
      previousId.length &&
      /^h[1-6]$/.test((previousId.prop("tagName") as string).toLowerCase())
    ) {
      return {
        id: previousId.attr("id")!,
        text: previousId.text().trim(),
      };
    }
    parent = parent.parent();
  }
  return undefined;
}

/**
 * Walk up the ancestor chain from a ref-for link, checking each ancestor
 * (and its preceding siblings) for the nearest enclosing heading or
 * algorithm box.
 */
export function findRefContext(
  $: CheerioAPI,
  $element: Cheerio<any>
): RefContext {
  let ancestor = $element.parent();
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
    const previousHeading = ancestor
      .prevAll("h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]")
      .first();
    if (previousHeading.length) {
      return {
        type: "section",
        name: previousHeading.text().trim(),
        sectionId: previousHeading.attr("id"),
      };
    }
    ancestor = ancestor.parent();
  }
  return { type: "unknown", name: "(top-level)" };
}

/** Same ancestor walk as findRefContext, returning the renderable container. */
export function findRenderableContainer(
  $: CheerioAPI,
  $element: Cheerio<any>
): RenderableContainer | null {
  let ancestor = $element.parent();
  while (ancestor.length) {
    const tag = (ancestor.prop("tagName") as string).toLowerCase();
    if (
      (tag === "div" && ancestor.attr("data-algorithm")) ||
      (tag === "ol" && ancestor.attr("class") === "algorithm")
    ) {
      return { container: ancestor, type: "algorithm" };
    }
    const previousHeading = ancestor
      .prevAll("h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]")
      .first();
    if (previousHeading.length) {
      const level = parseInt(
        (previousHeading.prop("tagName") as string)[1]
      );
      return {
        container: previousHeading,
        type: "section",
        sectionLevel: level,
      };
    }
    ancestor = ancestor.parent();
  }
  return null;
}

/** Percent-encode non-ASCII characters (circled digits) for the URL fragment. */
export function buildRefUrl(specBase: string, refId: string): string {
  return `${specBase}#${encodeURI(refId)}`;
}
