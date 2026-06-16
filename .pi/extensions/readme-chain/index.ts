/**
 * readme-chain — Documentation chain reminder for pi
 *
 * Tracks which parts of the project's README/AGENTS documentation chain
 * have been consulted, reminds the agent to check the chain before editing
 * files, and provides a tool + command to display the chain on demand.
 *
 * See README.md for full documentation.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import * as fs from "node:fs";
import * as path from "node:path";

export default function (pi: ExtensionAPI) {
  // ── State ──
  // Directories whose README chain has been consulted this session.
  // Keyed by absolute directory path.  Populated when the agent reads a
  // README.md or calls readme_chain for a given path.
  const consulted = new Set<string>();
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
  function collectChain(targetPath: string, root: string): ChainEntry[] {
    const chain: ChainEntry[] = [];
    const absTarget = path.resolve(targetPath);

    // Root AGENTS.md always comes first.
    const agentsPath = path.join(root, "AGENTS.md");
    if (fs.existsSync(agentsPath)) {
      chain.push({ dir: root, readme: agentsPath });
    }

    // Determine the relative directory path from root to target.
    const relPath = path.relative(root, absTarget);
    const dirSegments = relPath.split(path.sep);
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

  /** Format the chain as a bullet list of relative paths. */
  function formatChainSummary(chain: ChainEntry[], cwd: string): string {
    return chain
      .map((entry) => {
        const source = entry.readme ?? entry.dir;
        return `  - \`${path.relative(cwd, source)}\``;
      })
      .join("\n");
  }

  /** Read and concatenate the contents of all files in the chain. */
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

  /** Mark a file or directory as having had its chain consulted. */
  function markConsulted(targetPath: string) {
    const abs = path.resolve(targetPath);
    const stat = fs.statSync(abs, { throwIfNoEntry: false });
    const dir = stat?.isDirectory() ? abs : path.dirname(abs);
    consulted.add(dir);
  }

  /** Should a given path be ignored (vendor, target, .pi, etc.)? */
  function isIgnored(filePath: string, cwd: string): boolean {
    const root = findRoot(cwd);
    if (!root) return true;
    const abs = path.resolve(cwd, filePath);
    if (!abs.startsWith(root)) return true;
    const skipDirs = new Set(["node_modules", "vendor", "target", ".pi", ".git"]);
    const relParts = path.relative(root, abs).split(path.sep);
    return relParts.some((part) => skipDirs.has(part));
  }

  /** Produce the readme_chain response for a given path. */
  function getChainResponse(targetPath: string, cwd: string): string {
    const root = findRoot(cwd);
    if (!root) {
      return "No AGENTS.md found — there is no documentation chain defined for this project.";
    }
    const chain = collectChain(targetPath, root);
    if (chain.length === 0) {
      return `No README.md files found in the chain for \`${targetPath}\`.`;
    }
    const summary = formatChainSummary(chain, cwd);
    const contents = readChainContents(chain, cwd);
    return (
      `## Documentation chain for \`${targetPath}\`\n\n` +
      `${summary}\n\n---\n\n${contents}`
    );
  }

  // ── Register the readme_chain tool ──

  pi.registerTool({
    name: "readme_chain",
    label: "Readme Chain",
    description:
      "Walk up the directory tree from a given file or directory path and collect all " +
      "AGENTS.md and README.md files in the chain. " +
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
      "Collect the documentation chain (AGENTS.md + nested README.md files) for a file path",
    promptGuidelines: [
      "Before editing a file in a new directory, use readme_chain to read the " +
      "documentation chain (AGENTS.md + all README.md files) for that file's path.",
    ],
    async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
      const cwd = ctx.cwd;
      markConsulted(params.path ?? cwd);
      const content = getChainResponse(params.path ?? cwd, cwd);
      return {
        content: [{ type: "text", text: content }],
        details: {},
      };
    },
  });

  // ── Auto-consult when reading README/AGENTS files ──

  pi.on("tool_call", async (event, ctx) => {
    if (event.toolName !== "read") return;

    const input = event.input as Record<string, unknown>;
    const filePath = typeof input.path === "string" ? input.path : undefined;
    if (!filePath) return;

    if (isIgnored(filePath, ctx.cwd)) return;

    const fileName = path.basename(filePath);
    if (fileName === "README.md" || fileName === "AGENTS.md") {
      const dir = path.resolve(ctx.cwd, path.dirname(filePath));
      consulted.add(dir);
    }
  });

  // ── Register /readme-chain command (for human use) ──

  pi.registerCommand("readme-chain", {
    description:
      "Walk up the directory tree from a path and display all AGENTS.md and README.md " +
      "files in the documentation chain. Usage: /readme-chain [path]",
    handler: async (args, ctx) => {
      const targetPath = args?.trim() || ctx.cwd;
      const root = findRoot(ctx.cwd);
      if (!root) {
        ctx.ui.notify("No AGENTS.md found at the project root.", "warning");
        return;
      }
      const chain = collectChain(targetPath, root);
      if (chain.length === 0) {
        ctx.ui.notify(`No README files found in the chain for \`${targetPath}\`.`, "info");
        return;
      }
      markConsulted(targetPath);
      const summary = formatChainSummary(chain, ctx.cwd);
      ctx.ui.notify(
        `Documentation chain for \`${targetPath}\`:\n${summary}`,
        "info",
      );
    },
  });

}
