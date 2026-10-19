import { readdir, readFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { getDoc } from "./cache.mjs";
import { findParentSectionEl, findRenderableContainer } from "./render.mjs";

const DOC_ANCHOR_PATTERN = /^\s*\/\/\/\s*<((?:https?):\/\/[^>\s]+)>/;
const STEP_PATTERN = /^\s*\/\/\s*Step\s+([0-9]+(?:\.[0-9]+)*):\s*(.*)$/;
const COMMENT_PATTERN = /^\s*\/\/\s?(.*)$/;
const DECLARATION_PATTERN =
  /^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(fn|struct|enum|trait|impl|union|type|const|static)\b/;

const SKIP_DIRECTORIES = new Set([
  "target",
  "vendor",
  ".git",
  "node_modules",
  ".pi",
  ".agents",
]);

async function collectRustFiles(path) {
  const entries = await readdir(path, { withFileTypes: true }).catch(() => null);
  if (entries === null) {
    return path.endsWith(".rs") ? [path] : [];
  }

  const files = [];
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (SKIP_DIRECTORIES.has(entry.name)) continue;
      files.push(...(await collectRustFiles(join(path, entry.name))));
    } else if (entry.isFile() && entry.name.endsWith(".rs")) {
      files.push(join(path, entry.name));
    }
  }
  return files;
}

/** Parse a Rust file into anchor declarations and their step comments. */
function parseAnchors(file, source) {
  const lines = source.split("\n");
  const records = [];
  let pendingAnchor = null;
  let currentRecord = null;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];

    const anchorMatch = DOC_ANCHOR_PATTERN.exec(line);
    if (anchorMatch) {
      pendingAnchor = { url: anchorMatch[1], line: index + 1 };
      continue;
    }

    const declarationMatch = DECLARATION_PATTERN.exec(line);
    if (declarationMatch) {
      if (pendingAnchor) {
        const fragmentIndex = pendingAnchor.url.lastIndexOf("#");
        if (fragmentIndex !== -1 && fragmentIndex < pendingAnchor.url.length - 1) {
          const record = {
            file,
            line: pendingAnchor.line,
            anchor: pendingAnchor.url.slice(fragmentIndex + 1),
            url: pendingAnchor.url.slice(0, fragmentIndex),
            steps: [],
          };
          records.push(record);
          currentRecord = record;
        } else {
          currentRecord = null;
        }
      } else {
        currentRecord = null;
      }
      pendingAnchor = null;
      continue;
    }

    const stepMatch = STEP_PATTERN.exec(line);
    if (stepMatch && currentRecord) {
      const collected = collectStepText(lines, index, stepMatch[2]);
      currentRecord.steps.push({
        line: index + 1,
        number: stepMatch[1],
        text: collected.text,
      });
      index = collected.lastIndex;
    }
  }

  return records;
}

function collectStepText(lines, startIndex, firstLine) {
  const parts = [firstLine.trim()];
  let index = startIndex + 1;
  for (; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim() === "") break;
    if (STEP_PATTERN.test(line)) break;
    const commentMatch = COMMENT_PATTERN.exec(line);
    if (!commentMatch) break;
    const comment = commentMatch[1].trim();
    if (comment.startsWith("Note:") || comment.startsWith("TODO")) break;
    parts.push(comment);
  }
  return { text: parts.join(" "), lastIndex: index - 1 };
}

function normalize(value) {
  const unquoted = value.replace(/^["'“”]+|["'“”]+$/g, "");
  return unquoted
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

function extractQuotedText(comment) {
  const trimmed = comment.trim();
  // Only treat the comment as quoted when it starts with a quote; an unquoted
  // comment may still contain quoted literals such as "InvalidStateError".
  if (trimmed.startsWith('"')) {
    const lastQuote = trimmed.lastIndexOf('"');
    if (lastQuote > 0) {
      return trimmed.slice(1, lastQuote);
    }
  }
  const dashIndex = trimmed.search(/\s+[—-]\s+/);
  return dashIndex === -1 ? trimmed : trimmed.slice(0, dashIndex);
}

function firstWords(value, count) {
  return normalize(value).split(" ").slice(0, count).join(" ");
}

function squash(value) {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function stepTextMatches(comment, renderedStep) {
  const quoted = extractQuotedText(comment);
  const normalizedComment = normalize(quoted);
  const normalizedStep = normalize(renderedStep);
  if (!normalizedComment) return true;
  if (normalizedStep.startsWith(normalizedComment)) return true;
  if (normalizedComment.startsWith(normalizedStep)) return true;
  // Superscripts lose their caret when HTML is rendered to text (2^32
  // becomes 232), so compare again with all separators removed.
  const squashedStep = squash(renderedStep);
  const squashedComment = squash(quoted);
  if (
    squashedStep.startsWith(squashedComment) ||
    squashedComment.startsWith(squashedStep)
  ) {
    return true;
  }
  return firstWords(quoted, 6) === firstWords(renderedStep, 6);
}

function collectAlgorithmSteps($, $orderedList, parentNumber, map) {
  $orderedList.children("li").each((index, listItem) => {
    const $listItem = $(listItem);
    const fullNumber = parentNumber
      ? `${parentNumber}.${index + 1}`
      : `${index + 1}`;

    const $clone = $listItem.clone();
    $clone.find("ol").remove();
    $clone.find("div.dfn-panel").remove();
    if (!map.has(fullNumber)) {
      map.set(fullNumber, $clone.text().trim());
    }

    // Some generated specs (bikeshed) keep nested numbering as literal text
    // in separate paragraphs of the same list item, e.g. "2.1 Run ...".
    $listItem.find("p").each((_paragraphIndex, paragraph) => {
      const paragraphText = $(paragraph).text().trim();
      const embedded = /^(\d+(?:\.\d+)+)\.?\s+([\s\S]*)$/.exec(paragraphText);
      if (embedded && !map.has(embedded[1])) {
        map.set(embedded[1], embedded[2]);
      }
    });

    const $nestedList = $listItem.children("ol");
    if ($nestedList.length) {
      collectAlgorithmSteps($, $nestedList, fullNumber, map);
    }
  });
}

function collectFromAlgorithmContainer($, $algorithm, map) {
  const tag = ($algorithm.prop("tagName") || "").toLowerCase();
  if (tag === "ol") {
    collectAlgorithmSteps($, $algorithm, "", map);
    return;
  }
  const $orderedList = $algorithm.children("ol").first();
  if ($orderedList.length) {
    collectAlgorithmSteps($, $orderedList, "", map);
  }
}

function collectFromSection($, $heading, level, map) {
  let element = $heading.next();
  while (element.length) {
    const tag = (element.prop("tagName") || "").toLowerCase();
    if (tag && /^h[1-6]$/.test(tag) && parseInt(tag[1]) <= level) break;
    if (tag === "div" && element.attr("data-algorithm") !== undefined) {
      collectFromAlgorithmContainer($, element, map);
    } else if (tag === "ol" && element.attr("class") === "algorithm") {
      collectFromAlgorithmContainer($, element, map);
    } else if (tag === "ol") {
      // bikeshed-style specs use a plain <ol> after "must run these steps".
      const previousText = element.prev().text().trim();
      if (/run (?:these|the following) steps/i.test(previousText)) {
        collectAlgorithmSteps($, element, "", map);
      }
    }
    element = element.next();
  }
}

function collectStepsForAnchor($, anchor) {
  const map = new Map();
  const $element = $(`[id="${anchor}"]`).first();
  if (!$element.length) return map;

  const tagName = ($element.prop("tagName") || "").toLowerCase();

  if (tagName === "div" && $element.attr("data-algorithm") !== undefined) {
    collectFromAlgorithmContainer($, $element, map);
    return map;
  }
  if (tagName === "ol" && $element.attr("class") === "algorithm") {
    collectFromAlgorithmContainer($, $element, map);
    return map;
  }

  const enclosing = findRenderableContainer($, $element);
  if (enclosing?.type === "algorithm") {
    collectFromAlgorithmContainer($, enclosing.container, map);
    return map;
  }

  if (/^h[1-6]$/.test(tagName)) {
    collectFromSection($, $element, parseInt(tagName[1]), map);
    return map;
  }

  const parentSection = findParentSectionEl($, $element);
  if (parentSection) {
    const $heading = $(`[id="${parentSection.id}"]`).first();
    if ($heading.length) {
      const headingTag = ($heading.prop("tagName") || "").toLowerCase();
      collectFromSection($, $heading, parseInt(headingTag[1]), map);
    }
  }
  return map;
}

export async function verifyPaths(paths, options = {}) {
  const files = [];
  for (const path of paths) {
    files.push(...(await collectRustFiles(path)));
  }

  const records = [];
  for (const file of files) {
    const source = await readFile(file, "utf-8");
    records.push(...parseAnchors(file, source));
  }

  const issues = [];
  const documents = new Map();
  let checkedAnchors = 0;
  let checkedSteps = 0;

  for (const record of records) {
    let $ = documents.get(record.url);
    if ($ === undefined) {
      try {
        $ = await getDoc(record.url, options);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        issues.push({
          file: record.file,
          line: record.line,
          anchor: record.anchor,
          message: `could not fetch ${record.url}: ${message}`,
        });
        $ = null;
      }
      documents.set(record.url, $);
    }

    if ($ === null) continue;

    const $element = $(`[id="${record.anchor}"]`).first();
    if (!$element.length) {
      issues.push({
        file: record.file,
        line: record.line,
        anchor: record.anchor,
        message: `anchor not found in ${record.url}`,
      });
      continue;
    }

    checkedAnchors += 1;
    if (record.steps.length === 0) continue;

    const stepMap = collectStepsForAnchor($, record.anchor);
    for (const step of record.steps) {
      checkedSteps += 1;
      const rendered = stepMap.get(step.number);
      if (rendered === undefined) {
        issues.push({
          file: record.file,
          line: step.line,
          anchor: record.anchor,
          message: `step ${step.number} not found in the spec algorithm`,
        });
        continue;
      }
      if (!stepTextMatches(step.text, rendered)) {
        issues.push({
          file: record.file,
          line: step.line,
          anchor: record.anchor,
          message: `step ${step.number} text does not match the spec`,
          expected: rendered,
          actual: step.text,
        });
      }
    }
  }

  return { checkedAnchors, checkedSteps, issues };
}

export function formatVerifyReport(report, root) {
  const lines = [];
  for (const issue of report.issues) {
    lines.push(
      `${relative(root, issue.file)}:${issue.line} ${issue.anchor}: ${issue.message}`
    );
    if (issue.expected !== undefined && issue.actual !== undefined) {
      lines.push(`  spec: ${issue.expected}`);
      lines.push(`  code: ${issue.actual}`);
    }
  }
  lines.push(
    `\nChecked ${report.checkedAnchors} anchors and ${report.checkedSteps} step comments: ` +
      `${report.issues.length} issue${report.issues.length === 1 ? "" : "s"}.`
  );
  return lines.join("\n");
}
