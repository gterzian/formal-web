import { getDoc } from "./cache.mjs";
import {
  collectLinks,
  DOC_REMINDER,
  findParentSectionEl,
  findRefContext,
  findRenderableContainer,
  formatLinkTable,
  mergeLinks,
  renderAlgorithmContainer,
  renderAlgorithmSteps,
  walkSiblingContent,
  buildRefUrl,
} from "./render.mjs";
import { formatVerifyReport, verifyPaths } from "./verify.mjs";

const DEFAULT_MAX_LINES = 400;

// ── Lookup ────────────────────────────────────────────────────────────────────

async function lookupAnchor(url, id, options) {
  const $ = await getDoc(url, options);
  const element = $(`[id="${id}"]`).first();
  if (!element.length) {
    return { text: `No element found with id="${id}".` };
  }

  const tagName = (element.prop("tagName") || "").toLowerCase();
  // A <dfn>'s own text is only the term; the sentence that defines it lives in
  // the enclosing <p>, <dd>, or <dt>.
  const text =
    tagName === "dfn"
      ? (element.closest("p, dd, dt").text().trim() || element.text().trim())
      : element.text().trim();

  const links = [];
  mergeLinks(links, collectLinks($, element));

  // For headings: walk siblings stopping at same-or-higher heading.
  if (/^h[1-6]$/.test(tagName)) {
    const level = parseInt(tagName[1]);
    const parts = [text, ...walkSiblingContent($, element, links, level)];
    return { text: parts.join("\n\n") + formatLinkTable(links) + DOC_REMINDER };
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
      if (steps.length) output += `\n${steps.join("\n")}`;
    }
    return { text: output + formatLinkTable(links) + DOC_REMINDER };
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
  return { text: parts.join("\n\n") + formatLinkTable(links) + DOC_REMINDER };
}

// ── Reference links ───────────────────────────────────────────────────────────
// In web standards, every <dfn id="foo"> has corresponding reference links
// throughout the spec: <a id="ref-for-foo">, <a id="ref-for-foo①">, etc.
// The circled-digit suffixes distinguish multiple references to the same
// concept.

function collectReferenceElements($, id) {
  // Two patterns are supported:
  //   Pattern 1 (WHATWG single-page): <a id="ref-for-{id}">   (e.g. Web IDL, DOM, Fetch)
  //   Pattern 2 (HTML multi-source):  <a id="{section}:{id}" href="#{id}" >  (HTML spec)
  const referenceElements = [];
  const seenIds = new Set();

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

function renderReferenceReads($, specBase, id, referenceElements, read) {
  let indices;
  if (read === "all") {
    indices = referenceElements.map((_reference, index) => index);
  } else {
    indices = [read - 1];
    if (indices[0] < 0 || indices[0] >= referenceElements.length) {
      return {
        text: `Index ${read} out of range. There are ${referenceElements.length} references.`,
      };
    }
  }

  const blocks = [];
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
      const links = [];
      const output = renderAlgorithmContainer($, container.container, links);
      blocks.push(
        `[${reference.index}/${referenceElements.length}] ${referenceUrl}\n${output}`
      );
      continue;
    }

    // Section heading
    const $heading = container.container;
    const links = [];
    const text = $heading.text().trim();
    const siblingParts = walkSiblingContent(
      $,
      $heading,
      links,
      container.sectionLevel
    );
    blocks.push(
      `[${reference.index}/${referenceElements.length}] ${referenceUrl}\n${text}\n${siblingParts.join("\n\n")}`
    );
  }

  return { text: blocks.join("\n\n---\n\n") + DOC_REMINDER };
}

async function findReferenceLinks(url, id, read, options) {
  const $ = await getDoc(url, options);
  const referenceElements = collectReferenceElements($, id);

  if (referenceElements.length === 0) {
    return { text: `No reference links found for "${id}".` };
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
    `use this command with read=<index> or read="all".\n\n` +
    `Documentation note: When documenting code that implements a specific ` +
    `algorithm step that references this concept, use the reference URL ` +
    `(e.g. #ref-for-${id}①) rather than the canonical concept URL ` +
    `(#${id}). Your code implements 'the thing as used in a particular ` +
    `algorithm', not the thing itself.`;

  return { text: table + usage + DOC_REMINDER };
}

// ── Search ────────────────────────────────────────────────────────────────────

async function searchSpecIds(url, query, limit, options) {
  const $ = await getDoc(url, options);
  const matches = [];

  $("[id]").each((_index, element) => {
    const $element = $(element);
    const id = $element.attr("id") || "";
    if (id.toLowerCase().includes(query.toLowerCase())) {
      const tag = (
        element.type === "tag" ? element.name : ""
      ).toLowerCase();
      const text = $element.text().trim().slice(0, 120);
      matches.push({ tag, id, text });
    }
  });

  if (matches.length === 0) {
    return { text: `No ids matching "${query}" found.` };
  }

  const shown = matches.slice(0, limit);
  const note =
    matches.length > limit
      ? `\n[Showing ${limit} of ${matches.length} matches — refine your query for more precise results]`
      : "";

  const output = shown
    .map((match) => `#${match.id}\n  <${match.tag}> ${match.text}`)
    .join("\n\n");

  return { text: output + note + DOC_REMINDER };
}

// ── CLI ───────────────────────────────────────────────────────────────────────

function parseArguments(args) {
  const parsed = {
    positional: [],
    offset: 1,
    maxLines: DEFAULT_MAX_LINES,
    refresh: false,
    limit: 30,
    read: undefined,
    json: false,
  };

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--refresh") {
      parsed.refresh = true;
      continue;
    }
    if (argument === "--json") {
      parsed.json = true;
      continue;
    }
    if (argument === "--offset") {
      parsed.offset = parsePositiveInteger(args[++index], "--offset");
      continue;
    }
    if (argument === "--max-lines") {
      parsed.maxLines = parsePositiveInteger(args[++index], "--max-lines");
      continue;
    }
    if (argument === "--limit") {
      parsed.limit = parsePositiveInteger(args[++index], "--limit");
      continue;
    }
    if (argument === "--read") {
      const value = args[++index];
      if (value === undefined) {
        throw new Error("Missing value for --read");
      }
      if (value === "all") {
        parsed.read = "all";
      } else {
        const parsedIndex = Number.parseInt(value, 10);
        if (!Number.isInteger(parsedIndex) || parsedIndex < 1) {
          throw new Error(`Invalid --read value: ${value}`);
        }
        parsed.read = parsedIndex;
      }
      continue;
    }
    parsed.positional.push(argument);
  }

  return parsed;
}

function parsePositiveInteger(value, flag) {
  if (value === undefined) {
    throw new Error(`Missing value for ${flag}`);
  }
  const parsed = Number.parseInt(value, 10);
  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error(`Invalid ${flag} value: ${value}`);
  }
  return parsed;
}

function paginate(text, offset, maxLines) {
  const lines = text.split("\n");
  const totalLines = lines.length;
  const start = Math.max(0, offset - 1);
  const end = Math.min(totalLines, start + maxLines);

  if (start >= totalLines) {
    return `[Offset ${offset} is past the end: the result has ${totalLines} lines.]`;
  }

  const body = lines.slice(start, end).join("\n");
  if (start === 0 && end >= totalLines) {
    return body;
  }

  return `${body}\n\n[Showing lines ${start + 1}-${end} of ${totalLines}. Continue with --offset ${end + 1}]`;
}

function requirePositional(positional, count, usage) {
  if (positional.length < count) {
    throw new Error(`Usage: ${usage}`);
  }
}

async function runLookup(parsed) {
  requirePositional(
    parsed.positional,
    2,
    "spec.mjs lookup <spec-url> <id> [--offset N] [--max-lines N] [--refresh]"
  );
  const [url, id] = parsed.positional;
  const result = await lookupAnchor(url, id, { refresh: parsed.refresh });
  process.stdout.write(
    `${paginate(result.text, parsed.offset, parsed.maxLines)}\n`
  );
  return 0;
}

async function runRefs(parsed) {
  requirePositional(
    parsed.positional,
    2,
    "spec.mjs refs <spec-url> <id> [--read <n>|all] [--offset N] [--max-lines N] [--refresh]"
  );
  const [url, id] = parsed.positional;
  const result = await findReferenceLinks(url, id, parsed.read, {
    refresh: parsed.refresh,
  });
  process.stdout.write(
    `${paginate(result.text, parsed.offset, parsed.maxLines)}\n`
  );
  return 0;
}

async function runSearch(parsed) {
  requirePositional(
    parsed.positional,
    2,
    "spec.mjs search <spec-url> <keyword> [--limit N] [--offset N] [--max-lines N] [--refresh]"
  );
  const [url, query] = parsed.positional;
  const result = await searchSpecIds(url, query, parsed.limit, {
    refresh: parsed.refresh,
  });
  process.stdout.write(
    `${paginate(result.text, parsed.offset, parsed.maxLines)}\n`
  );
  return 0;
}

async function runVerify(parsed) {
  if (parsed.positional.length === 0) {
    throw new Error("Usage: spec.mjs verify <path...> [--refresh] [--json]");
  }
  const report = await verifyPaths(parsed.positional, {
    refresh: parsed.refresh,
  });
  if (parsed.json) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } else {
    process.stdout.write(`${formatVerifyReport(report, process.cwd())}\n`);
  }
  return report.issues.length === 0 ? 0 : 1;
}

function printUsage() {
  process.stderr.write(
    [
      "Usage:",
      "  spec.mjs lookup <spec-url> <id> [--offset N] [--max-lines N] [--refresh]",
      "  spec.mjs refs   <spec-url> <id> [--read <n>|all] [--offset N] [--max-lines N] [--refresh]",
      "  spec.mjs search <spec-url> <keyword> [--limit N] [--offset N] [--max-lines N] [--refresh]",
      "  spec.mjs verify <path...> [--refresh] [--json]",
      "",
    ].join("\n")
  );
}

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  if (!command) {
    printUsage();
    return 1;
  }

  const parsed = parseArguments(rest);
  switch (command) {
    case "lookup":
      return runLookup(parsed);
    case "refs":
      return runRefs(parsed);
    case "search":
      return runSearch(parsed);
    case "verify":
      return runVerify(parsed);
    default:
      printUsage();
      return 1;
  }
}

main()
  .then((exitCode) => {
    process.exitCode = exitCode;
  })
  .catch((error) => {
    process.stderr.write(
      `${error instanceof Error ? error.message : String(error)}\n`
    );
    process.exitCode = 1;
  });
