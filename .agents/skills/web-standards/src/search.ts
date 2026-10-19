import type { CheerioAPI } from "cheerio";
import { getDoc, type FetchOptions } from "./cache.ts";
import { DOC_REMINDER } from "./render.ts";

export interface SearchResult {
  text: string;
  details: Record<string, unknown>;
}

interface SearchMatch {
  tag: string;
  id: string;
  text: string;
}

/** Search a spec for ids containing a keyword. */
export async function searchSpecIds(
  url: string,
  query: string,
  limit: number,
  options: FetchOptions = {}
): Promise<SearchResult> {
  const $: CheerioAPI = await getDoc(url, options);
  const matches: SearchMatch[] = [];

  $("[id]").each((_index, element) => {
    const $element = $(element);
    const id = $element.attr("id") || "";
    if (id.toLowerCase().includes(query.toLowerCase())) {
      const tag = (
        element.type === "tag" ? (element as { name: string }).name : ""
      ).toLowerCase();
      const text = $element.text().trim().slice(0, 120);
      matches.push({ tag, id, text });
    }
  });

  if (matches.length === 0) {
    return {
      text: `No ids matching "${query}" found.`,
      details: { url, query },
    };
  }

  const shown = matches.slice(0, limit);
  const note =
    matches.length > limit
      ? `\n[Showing ${limit} of ${matches.length} matches — refine your query for more precise results]`
      : "";

  const output = shown
    .map((match) => `#${match.id}\n  <${match.tag}> ${match.text}`)
    .join("\n\n");

  return {
    text: output + note + DOC_REMINDER,
    details: {
      url,
      query,
      total: matches.length,
      returned: shown.length,
    },
  };
}
