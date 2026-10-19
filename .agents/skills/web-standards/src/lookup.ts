import type { CheerioAPI } from "cheerio";
import { getDoc, type FetchOptions } from "./cache.ts";
import {
  collectLinks,
  DOC_REMINDER,
  findParentSectionEl,
  formatLinkTable,
  mergeLinks,
  renderAlgorithmSteps,
  walkSiblingContent,
  type SpecLink,
} from "./render.ts";

export interface LookupResult {
  text: string;
  details: Record<string, unknown>;
}

/** Look up a spec anchor and render its content and following algorithm boxes. */
export async function lookupAnchor(
  url: string,
  id: string,
  options: FetchOptions = {}
): Promise<LookupResult> {
  const $: CheerioAPI = await getDoc(url, options);
  const element = $(`[id="${id}"]`).first();
  if (!element.length) {
    return {
      text: `No element found with id="${id}".`,
      details: { url, id },
    };
  }

  const tagName = (element.prop("tagName") as string).toLowerCase();
  const text = element.text().trim();

  const links: SpecLink[] = [];
  mergeLinks(links, collectLinks($, element));

  // For headings: walk siblings stopping at same-or-higher heading.
  if (/^h[1-6]$/.test(tagName)) {
    const level = parseInt(tagName[1]);
    const parts = [text, ...walkSiblingContent($, element, links, level)];
    const linkTable = formatLinkTable(links);
    return {
      text: parts.join("\n\n") + linkTable + DOC_REMINDER,
      details: { url, id, tag: tagName },
    };
  }

  // For algorithm divs: render full steps directly.
  if (tagName === "div" && element.attr("data-algorithm") !== undefined) {
    const algorithmName = element.attr("data-algorithm") || "(unnamed)";
    const $paragraph = element.children("p").first();
    const $orderedList = element.children("ol").first();
    const header = $paragraph.length ? $paragraph.text().trim() : "";
    let output = `Algorithm: ${algorithmName}\n${header}`;
    if ($orderedList.length) {
      const steps = renderAlgorithmSteps($, $orderedList, "", links);
      if (steps.length) {
        output += `\n${steps.join("\n")}`;
      }
    }
    const linkTable = formatLinkTable(links);
    return {
      text: output + linkTable + DOC_REMINDER,
      details: { url, id, tag: tagName, algorithm: algorithmName },
    };
  }

  // For any other element: show the element context, then walk siblings.
  const siblingParts = walkSiblingContent($, element, links);
  const parentSection = findParentSectionEl($, element);
  const heading = parentSection
    ? `\nSection: ${parentSection.id}: ${parentSection.text}`
    : "";
  const parts = [
    `Element: <${tagName} id="${id}">` + heading + `\n\n${text}`,
    ...siblingParts,
  ];
  const linkTable = formatLinkTable(links);
  return {
    text: parts.join("\n\n") + linkTable + DOC_REMINDER,
    details: { url, id, tag: tagName },
  };
}
