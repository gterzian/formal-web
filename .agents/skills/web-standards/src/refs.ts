import type { Cheerio, CheerioAPI } from "cheerio";
import { getDoc, type FetchOptions } from "./cache.ts";
import {
  buildRefUrl,
  DOC_REMINDER,
  findRefContext,
  findRenderableContainer,
  renderAlgorithmContainer,
  walkSiblingContent,
  type SpecLink,
} from "./render.ts";

export interface ReferenceElement {
  element: Cheerio<any>;
  id: string;
  index: number;
}

export interface RefsResult {
  text: string;
  details: Record<string, unknown>;
}

export type ReadSelection = number | "all";

/**
 * In web standards, every <dfn id="foo"> has corresponding reference links
 * throughout the spec: <a id="ref-for-foo">, <a id="ref-for-foo①">, etc.
 * The circled-digit suffixes distinguish multiple references to the same
 * concept.
 */
export function collectReferenceElements(
  $: CheerioAPI,
  id: string
): ReferenceElement[] {
  // Two patterns are supported:
  //   Pattern 1 (WHATWG single-page): <a id="ref-for-{id}">   (e.g. Web IDL, DOM, Fetch)
  //   Pattern 2 (HTML multi-source):  <a id="{section}:{id}" href="#{id}" >  (HTML spec)
  const referenceElements: ReferenceElement[] = [];
  const seenIds = new Set<string>();

  const prefix = `ref-for-${id}`;
  $(`[id^="${prefix}"]`).each((_index, element) => {
    const $element = $(element);
    const elementId = $element.attr("id") || "";
    if (elementId.startsWith(prefix) && !seenIds.has(elementId)) {
      seenIds.add(elementId);
      referenceElements.push({ element: $element, id: elementId, index: 0 });
    }
  });

  $(`a[href="#${id}"]`).each((_index, element) => {
    const $element = $(element);
    const elementId = $element.attr("id") || "";
    // Match ids that end with ":{id}" (the section:concept pattern)
    if (elementId.endsWith(`:${id}`) && !seenIds.has(elementId)) {
      seenIds.add(elementId);
      referenceElements.push({ element: $element, id: elementId, index: 0 });
    }
  });

  referenceElements.forEach((reference, index) => {
    reference.index = index + 1;
  });
  return referenceElements;
}

export async function findReferenceLinks(
  url: string,
  id: string,
  read: ReadSelection | undefined,
  options: FetchOptions = {}
): Promise<RefsResult> {
  const $ = await getDoc(url, options);
  const referenceElements = collectReferenceElements($, id);

  if (referenceElements.length === 0) {
    return {
      text: `No reference links found for "${id}".`,
      details: { url, id },
    };
  }

  const specBase = url.replace(/#.*$/, "");

  if (read !== undefined) {
    return renderReferenceReads($, specBase, id, referenceElements, read);
  }

  // ── List mode ──────────────────────────────────────────────────────────────
  const rows = referenceElements.map((reference) => {
    const context = findRefContext($, reference.element);
    return {
      index: reference.index,
      url: buildRefUrl(specBase, reference.id),
      rawId: reference.id,
      context: `${context.type}: ${context.name}`,
    };
  });

  const urlMaxLen = Math.min(
    Math.max(...rows.map((row) => row.url.length), 40),
    80
  );
  const contextMaxLen = Math.min(
    Math.max(...rows.map((row) => row.context.length), "Context".length),
    60
  );

  const header = `┌─ # ── ${"Reference URL".padEnd(urlMaxLen)} ── ${"Context".padEnd(contextMaxLen)} ─┐`;
  const separator = `│${"".padEnd(urlMaxLen + contextMaxLen + 10, "─")}│`;

  const tableRows = rows.map((row) => {
    return `│ ${String(row.index).padStart(2)}  ${row.url.padEnd(urlMaxLen)}  ${row.context.padEnd(contextMaxLen)} │`;
  });

  const table = ["", header, ...tableRows, separator].join("\n");

  const usage =
    `\n\nTo read the content of a specific reference, ` +
    `use this tool with read=<index> or read="all".\n\n` +
    `Documentation note: When documenting code that implements a specific ` +
    `algorithm step that references this concept, use the reference URL ` +
    `(e.g. #ref-for-${id}①) rather than the canonical concept URL ` +
    `(#${id}). Your code implements 'the thing as used in a particular ` +
    `algorithm', not the thing itself.`;

  return {
    text: table + usage + DOC_REMINDER,
    details: { url, id, total: referenceElements.length },
  };
}

function renderReferenceReads(
  $: CheerioAPI,
  specBase: string,
  id: string,
  referenceElements: ReferenceElement[],
  read: ReadSelection
): RefsResult {
  let indices: number[];
  if (read === "all") {
    indices = referenceElements.map((_reference, index) => index);
  } else {
    indices = [read - 1];
    if (indices[0] < 0 || indices[0] >= referenceElements.length) {
      return {
        text: `Index ${read} out of range. There are ${referenceElements.length} references.`,
        details: { url: specBase, id },
      };
    }
  }

  const blocks: string[] = [];
  for (const index of indices) {
    const reference = referenceElements[index];
    const referenceUrl = buildRefUrl(specBase, reference.id);
    const container = findRenderableContainer($, reference.element);

    if (!container) {
      blocks.push(
        `[${reference.index}/${referenceElements.length}] ${referenceUrl}\n(context not found)`
      );
      continue;
    }

    if (container.type === "algorithm") {
      const links: SpecLink[] = [];
      const output = renderAlgorithmContainer($, container.container, links);
      blocks.push(
        `[${reference.index}/${referenceElements.length}] ${referenceUrl}\n${output}`
      );
      continue;
    }

    // Section heading
    const $heading = container.container;
    const level = container.sectionLevel!;
    const links: SpecLink[] = [];
    const text = $heading.text().trim();
    const siblingParts = walkSiblingContent($, $heading, links, level);
    blocks.push(
      `[${reference.index}/${referenceElements.length}] ${referenceUrl}\n${text}\n${siblingParts.join("\n\n")}`
    );
  }

  return {
    text: blocks.join("\n\n---\n\n") + DOC_REMINDER,
    details: {
      url: specBase,
      id,
      total: referenceElements.length,
      read: read === "all" ? referenceElements.length : indices.length,
    },
  };
}
