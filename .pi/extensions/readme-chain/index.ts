/**
 * readme-chain — Documentation chain reminder for pi
 *
 * Tracks which parts of the project's README documentation chain are already
 * in the model's conversation context, and returns only the READMEs that are
 * not, so repeated calls never duplicate content the model has already seen.
 *
 * See README.md for full documentation.
 */

import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import * as fs from "node:fs";
import * as path from "node:path";

export default function (pi: ExtensionAPI) {
  let projectRoot: string | null = null;

  // ── Helpers ──

  /** Find the project root by locating AGENTS.md. */
  function findRoot(from: string): string | null {
    if (projectRoot) return projectRoot;
    let dir = path.resolve(from);
    while (dir !== path.parse(dir).root) {
      if (fs.existsSync(path.join(dir, "AGENTS.md"))) {
        projectRoot = dir;
        return dir;
      }
      dir = path.dirname(dir);
    }
    return null;
  }

  interface ChainEntry {
    /** Absolute directory path. */
    dir: string;
    /** Absolute path to README.md, or null if none exists at this level. */
    readme: string | null;
  }

  /** Collect the documentation chain for a file or directory path. */
  function collectChain(targetPath: string, root: string, cwd: string): ChainEntry[] {
    const chain: ChainEntry[] = [];
    const absTarget = path.resolve(cwd, targetPath);

    // AGENTS.md is excluded — the project keeps exactly one, at the root, and
    // pi loads it as project instructions before the session starts.  Only
    // README.md files are walked from the chain.

    // Determine the relative directory path from root to target.
    const relPath = path.relative(root, absTarget);
    const dirSegments = relPath.split(path.sep);
    // A path outside the project root has no documentation chain.
    if (dirSegments.some((segment) => segment === "..")) {
      return [];
    }
    // If target is a file, drop the filename — only walk directory ancestors.
    if (!fs.statSync(absTarget, { throwIfNoEntry: false })?.isDirectory()) {
      dirSegments.pop();
    }

    // Walk each directory level, collecting README.md if present.
    let current = root;
    for (const segment of dirSegments) {
      if (!segment || segment === ".") continue;
      current = path.join(current, segment);
      const readmePath = path.join(current, "README.md");
      chain.push({
        dir: current,
        readme: fs.existsSync(readmePath) ? readmePath : null,
      });
    }

    return chain;
  }

  /** Format the chain as a bullet list of relative paths, marking consulted files. */
  function formatChainSummary(
    chain: ChainEntry[],
    cwd: string,
    inContext: Set<string>,
  ): string {
    return chain
      .map((entry) => {
        const source = entry.readme ?? entry.dir;
        const label = path.relative(cwd, source);
        const suffix =
          entry.readme && inContext.has(entry.readme)
            ? " _(read previously — content omitted)_"
            : "";
        return `  - \`${label}\`${suffix}`;
      })
      .join("\n");
  }

  /** Read and concatenate the contents of the given chain entries. */
  function readChainContents(chain: ChainEntry[], cwd: string): string {
    const parts: string[] = [];
    for (const entry of chain) {
      if (entry.readme) {
        const label = path.relative(cwd, entry.readme);
        const content = fs.readFileSync(entry.readme, "utf-8");
        parts.push(`## ${label}\n\n${content}`);
      }
    }
    return parts.join("\n\n---\n\n");
  }

  /**
   * Absolute paths of README files whose content is already in the model's
   * active context.  The session entries are read from
   * `buildContextEntries()`, which honors compaction, so a README dropped by
   * compaction is offered again on the next call.
   */
  function readmesInContext(ctx: ExtensionContext, cwd: string): Set<string> {
    const readmes = new Set<string>();
    const entries = ctx.sessionManager.buildContextEntries();

    // A read only delivers a file when its result is present and untruncated.
    // Correlate read tool calls with their results by tool call id.
    const readCalls = new Map<
      string,
      { filePath: string; offset: unknown; limit: unknown }
    >();
    for (const entry of entries) {
      if (entry.type !== "message") continue;
      const message = entry.message;
      if (message.role !== "assistant") continue;
      for (const block of message.content) {
        if (block.type !== "toolCall" || block.name !== "read") continue;
        const args = block.arguments as {
          path?: unknown;
          offset?: unknown;
          limit?: unknown;
        };
        if (typeof args.path !== "string") continue;
        readCalls.set(block.id, {
          filePath: args.path,
          offset: args.offset,
          limit: args.limit,
        });
      }
    }

    for (const entry of entries) {
      if (entry.type !== "message") continue;
      const message = entry.message;
      if (message.role !== "toolResult") continue;

      if (message.toolName === "read") {
        const call = readCalls.get(message.toolCallId);
        if (!call) continue;
        // A partial or truncated read leaves part of the file out of context.
        if (call.offset !== undefined || call.limit !== undefined) continue;
        const details = message.details as
          | { truncation?: { truncated?: boolean } }
          | undefined;
        if (details?.truncation?.truncated) continue;
        if (path.basename(call.filePath) !== "README.md") continue;
        readmes.add(path.resolve(cwd, call.filePath));
        continue;
      }

      if (message.toolName === "readme_chain") {
        const recorded = (message.details as { readmes?: unknown } | undefined)?.readmes;
        if (Array.isArray(recorded)) {
          for (const readme of recorded) {
            if (typeof readme === "string") readmes.add(readme);
          }
          continue;
        }
        // Fallback for results recorded before the details payload existed:
        // read the section headers the tool rendered.
        for (const block of message.content) {
          if (block.type !== "text") continue;
          for (const match of block.text.matchAll(/^## (.+)$/gm)) {
            const candidate = path.resolve(cwd, match[1].trim());
            if (path.basename(candidate) === "README.md" && fs.existsSync(candidate)) {
              readmes.add(candidate);
            }
          }
        }
      }
    }

    return readmes;
  }

  /** Produce the readme_chain response and the READMEs it delivered. */
  function getChainResponse(
    targetPath: string,
    cwd: string,
    ctx: ExtensionContext,
  ): { text: string; readmes: string[] } {
    const root = findRoot(cwd);
    if (!root) {
      return {
        text: "No AGENTS.md found — there is no documentation chain defined for this project.",
        readmes: [],
      };
    }
    const chain = collectChain(targetPath, root, cwd);
    if (chain.length === 0) {
      return {
        text: `No README.md files found in the chain for \`${targetPath}\`.`,
        readmes: [],
      };
    }

    const inContext = readmesInContext(ctx, cwd);
    const pending = chain.filter(
      (entry): entry is ChainEntry & { readme: string } =>
        entry.readme !== null && !inContext.has(entry.readme),
    );
    const header =
      `## Documentation chain for \`${targetPath}\`\n\n` +
      `${formatChainSummary(chain, cwd, inContext)}\n\n`;

    if (pending.length === 0) {
      return {
        text: `${header}All README.md files in this chain were read previously.`,
        readmes: [],
      };
    }

    const alreadyInContext = chain.filter((entry) => entry.readme !== null).length -
      pending.length;
    const note =
      alreadyInContext > 0
        ? `${alreadyInContext} README.md file(s) were read previously; their content is omitted below.\n\n---\n\n`
        : "---\n\n";
    const contents = readChainContents(pending, cwd);
    return {
      text: `${header}${note}${contents}`,
      readmes: pending.map((entry) => entry.readme),
    };
  }

  // ── Register the readme_chain tool ──

  pi.registerTool({
    name: "readme_chain",
    label: "Readme Chain",
    description:
      "Walk up the directory tree from a given file or directory path and collect the " +
      "nested README.md files in the chain that are not already in the conversation " +
      "context. " +
      "Use this before editing a file to understand the project conventions for that " +
      "part of the codebase. " +
      "If no path is given, the current working directory is used.",
    parameters: Type.Object({
      path: Type.Optional(
        Type.String({
          description:
            "File or directory path to find the chain for (default: current working directory)",
        }),
      ),
    }),
    promptSnippet:
      "Collect the documentation chain (nested README.md files) for a file path",
    promptGuidelines: [
      "Before editing a file in a new directory, use readme_chain to read " +
      "whatever part of the documentation chain is not already in context.",
    ],
    async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
      const cwd = ctx.cwd;
      const { text, readmes } = getChainResponse(params.path ?? cwd, cwd, ctx);
      return {
        content: [{ type: "text", text }],
        details: { readmes },
      };
    },
  });

  // ── Register /readme-chain command (for human use) ──

  pi.registerCommand("readme-chain", {
    description:
      "Walk up the directory tree from a path and display all README.md " +
      "files in the documentation chain. Usage: /readme-chain [path]",
    handler: async (args, ctx) => {
      const targetPath = args?.trim() || ctx.cwd;
      const root = findRoot(ctx.cwd);
      if (!root) {
        ctx.ui.notify("No AGENTS.md found at the project root.", "warning");
        return;
      }
      const chain = collectChain(targetPath, root, ctx.cwd);
      if (chain.length === 0) {
        ctx.ui.notify(`No README files found in the chain for \`${targetPath}\`.`, "info");
        return;
      }
      const summary = formatChainSummary(chain, ctx.cwd, readmesInContext(ctx, ctx.cwd));
      ctx.ui.notify(
        `Documentation chain for \`${targetPath}\`:\n${summary}`,
        "info",
      );
    },
  });
}
