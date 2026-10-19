#!/usr/bin/env node
import { createRequire as __createRequire } from "node:module";
const require = __createRequire(import.meta.url);

var __getOwnPropNames = Object.getOwnPropertyNames;
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};

// .agents/skills/readme-chain/src/readme-chain.ts
import * as fs from "node:fs";
import * as path from "node:path";
var require_readme_chain = __commonJS({
  ".agents/skills/readme-chain/src/readme-chain.ts"() {
    var SKIP_DIRECTORIES = /* @__PURE__ */ new Set([
      "node_modules",
      "vendor",
      "target",
      ".pi",
      ".agents",
      ".git"
    ]);
    function findRoot(from) {
      let directory = path.resolve(from);
      while (directory !== path.parse(directory).root) {
        if (fs.existsSync(path.join(directory, "AGENTS.md"))) {
          return directory;
        }
        directory = path.dirname(directory);
      }
      return null;
    }
    function collectChain(targetPath, root) {
      const chain = [];
      const absoluteTarget = path.resolve(targetPath);
      const relativePath = path.relative(root, absoluteTarget);
      const directorySegments = relativePath.split(path.sep);
      if (!fs.statSync(absoluteTarget, { throwIfNoEntry: false })?.isDirectory()) {
        directorySegments.pop();
      }
      let current = root;
      for (const segment of directorySegments) {
        if (!segment || segment === "." || segment === "..") continue;
        current = path.join(current, segment);
        const readmePath = path.join(current, "README.md");
        chain.push({
          dir: current,
          readme: fs.existsSync(readmePath) ? readmePath : null
        });
      }
      return chain;
    }
    function formatChainSummary(chain, root, includeRoot, cwd) {
      const lines = [];
      if (includeRoot) {
        lines.push(`  - \`${path.relative(cwd, path.join(root, "AGENTS.md"))}\``);
      }
      for (const entry of chain) {
        const source = entry.readme ?? entry.dir;
        lines.push(`  - \`${path.relative(cwd, source)}\``);
      }
      return lines.join("\n");
    }
    function readChainContents(chain, root, includeRoot, cwd) {
      const parts = [];
      if (includeRoot) {
        const rootAgents = path.join(root, "AGENTS.md");
        if (fs.existsSync(rootAgents)) {
          parts.push(
            `## ${path.relative(cwd, rootAgents)}

${fs.readFileSync(rootAgents, "utf-8")}`
          );
        }
      }
      for (const entry of chain) {
        if (entry.readme) {
          const label = path.relative(cwd, entry.readme);
          parts.push(`## ${label}

${fs.readFileSync(entry.readme, "utf-8")}`);
        }
      }
      return parts.join("\n\n---\n\n");
    }
    function countReadmes(chain, root, includeRoot) {
      let count = chain.filter((entry) => entry.readme !== null).length;
      if (includeRoot && fs.existsSync(path.join(root, "AGENTS.md"))) {
        count += 1;
      }
      return count;
    }
    function parseArguments(args) {
      const options = {
        targetPath: process.cwd(),
        list: false,
        includeRoot: false
      };
      const positional = [];
      for (const argument of args) {
        if (argument === "--list") {
          options.list = true;
          continue;
        }
        if (argument === "--include-root") {
          options.includeRoot = true;
          continue;
        }
        positional.push(argument);
      }
      if (positional.length > 0) {
        options.targetPath = positional[0];
      }
      return options;
    }
    function isIgnored(absolutePath, root) {
      if (!absolutePath.startsWith(root)) return false;
      const relativeParts = path.relative(root, absolutePath).split(path.sep);
      return relativeParts.some((part) => SKIP_DIRECTORIES.has(part));
    }
    function main() {
      const options = parseArguments(process.argv.slice(2));
      const cwd = process.cwd();
      const targetPath = path.resolve(cwd, options.targetPath);
      const root = findRoot(targetPath);
      if (!root) {
        process.stderr.write(
          "No AGENTS.md found \u2014 there is no documentation chain defined for this project.\n"
        );
        return 1;
      }
      if (isIgnored(targetPath, root)) {
        process.stderr.write(
          `${options.targetPath} is under a skipped directory.
`
        );
        return 1;
      }
      const chain = collectChain(targetPath, root);
      if (options.list) {
        if (countReadmes(chain, root, options.includeRoot) === 0) {
          process.stdout.write(
            `No README.md files found in the chain for \`${options.targetPath}\`.
`
          );
          return 0;
        }
        process.stdout.write(
          `Documentation chain for \`${options.targetPath}\`:
${formatChainSummary(chain, root, options.includeRoot, cwd)}
`
        );
        return 0;
      }
      if (countReadmes(chain, root, options.includeRoot) === 0) {
        process.stdout.write(
          `No README.md files found in the chain for \`${options.targetPath}\`.
`
        );
        return 0;
      }
      process.stdout.write(
        `## Documentation chain for \`${options.targetPath}\`

${formatChainSummary(chain, root, options.includeRoot, cwd)}

---

${readChainContents(chain, root, options.includeRoot, cwd)}
`
      );
      return 0;
    }
    try {
      process.exitCode = main();
    } catch (error) {
      process.stderr.write(
        `${error instanceof Error ? error.message : String(error)}
`
      );
      process.exitCode = 1;
    }
  }
});
export default require_readme_chain();
