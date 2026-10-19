import * as fs from "node:fs";
import * as path from "node:path";

interface ChainEntry {
  /** Absolute directory path. */
  dir: string;
  /** Absolute path to README.md, or null if none exists at this level. */
  readme: string | null;
}

const SKIP_DIRECTORIES = new Set([
  "node_modules",
  "vendor",
  "target",
  ".pi",
  ".agents",
  ".git",
]);

function findRoot(from: string): string | null {
  let directory = path.resolve(from);
  while (directory !== path.parse(directory).root) {
    if (fs.existsSync(path.join(directory, "AGENTS.md"))) {
      return directory;
    }
    directory = path.dirname(directory);
  }
  return null;
}

function collectChain(targetPath: string, root: string): ChainEntry[] {
  const chain: ChainEntry[] = [];
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
      readme: fs.existsSync(readmePath) ? readmePath : null,
    });
  }

  return chain;
}

function formatChainSummary(
  chain: ChainEntry[],
  root: string,
  includeRoot: boolean,
  cwd: string
): string {
  const lines: string[] = [];
  if (includeRoot) {
    lines.push(`  - \`${path.relative(cwd, path.join(root, "AGENTS.md"))}\``);
  }
  for (const entry of chain) {
    const source = entry.readme ?? entry.dir;
    lines.push(`  - \`${path.relative(cwd, source)}\``);
  }
  return lines.join("\n");
}

function readChainContents(
  chain: ChainEntry[],
  root: string,
  includeRoot: boolean,
  cwd: string
): string {
  const parts: string[] = [];
  if (includeRoot) {
    const rootAgents = path.join(root, "AGENTS.md");
    if (fs.existsSync(rootAgents)) {
      parts.push(
        `## ${path.relative(cwd, rootAgents)}\n\n${fs.readFileSync(rootAgents, "utf-8")}`
      );
    }
  }
  for (const entry of chain) {
    if (entry.readme) {
      const label = path.relative(cwd, entry.readme);
      parts.push(`## ${label}\n\n${fs.readFileSync(entry.readme, "utf-8")}`);
    }
  }
  return parts.join("\n\n---\n\n");
}

function countReadmes(chain: ChainEntry[], root: string, includeRoot: boolean): number {
  let count = chain.filter((entry) => entry.readme !== null).length;
  if (includeRoot && fs.existsSync(path.join(root, "AGENTS.md"))) {
    count += 1;
  }
  return count;
}

interface Options {
  targetPath: string;
  list: boolean;
  includeRoot: boolean;
}

function parseArguments(args: string[]): Options {
  const options: Options = {
    targetPath: process.cwd(),
    list: false,
    includeRoot: false,
  };
  const positional: string[] = [];
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

/** Reject paths that are inside a skipped directory. */
function isIgnored(absolutePath: string, root: string): boolean {
  if (!absolutePath.startsWith(root)) return false;
  const relativeParts = path.relative(root, absolutePath).split(path.sep);
  return relativeParts.some((part) => SKIP_DIRECTORIES.has(part));
}

function main(): number {
  const options = parseArguments(process.argv.slice(2));
  const cwd = process.cwd();
  const targetPath = path.resolve(cwd, options.targetPath);

  const root = findRoot(targetPath);
  if (!root) {
    process.stderr.write(
      "No AGENTS.md found — there is no documentation chain defined for this project.\n"
    );
    return 1;
  }
  if (isIgnored(targetPath, root)) {
    process.stderr.write(
      `${options.targetPath} is under a skipped directory.\n`
    );
    return 1;
  }

  const chain = collectChain(targetPath, root);

  if (options.list) {
    if (countReadmes(chain, root, options.includeRoot) === 0) {
      process.stdout.write(
        `No README.md files found in the chain for \`${options.targetPath}\`.\n`
      );
      return 0;
    }
    process.stdout.write(
      `Documentation chain for \`${options.targetPath}\`:\n` +
        `${formatChainSummary(chain, root, options.includeRoot, cwd)}\n`
    );
    return 0;
  }

  if (countReadmes(chain, root, options.includeRoot) === 0) {
    process.stdout.write(
      `No README.md files found in the chain for \`${options.targetPath}\`.\n`
    );
    return 0;
  }

  process.stdout.write(
    `## Documentation chain for \`${options.targetPath}\`\n\n` +
      `${formatChainSummary(chain, root, options.includeRoot, cwd)}\n\n---\n\n` +
      `${readChainContents(chain, root, options.includeRoot, cwd)}\n`
  );
  return 0;
}

try {
  process.exitCode = main();
} catch (error) {
  process.stderr.write(
    `${error instanceof Error ? error.message : String(error)}\n`
  );
  process.exitCode = 1;
}
