import { lookupAnchor } from "./lookup.ts";
import { findReferenceLinks, type ReadSelection } from "./refs.ts";
import { searchSpecIds } from "./search.ts";
import { formatVerifyReport, verifyPaths } from "./verify.ts";

const DEFAULT_MAX_LINES = 400;

interface ParsedArguments {
  positional: string[];
  offset: number;
  maxLines: number;
  refresh: boolean;
  limit: number;
  read: ReadSelection | undefined;
  json: boolean;
}

function parseArguments(args: string[]): ParsedArguments {
  const parsed: ParsedArguments = {
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

function parsePositiveInteger(value: string | undefined, flag: string): number {
  if (value === undefined) {
    throw new Error(`Missing value for ${flag}`);
  }
  const parsed = Number.parseInt(value, 10);
  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error(`Invalid ${flag} value: ${value}`);
  }
  return parsed;
}

function paginate(text: string, offset: number, maxLines: number): string {
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

function requirePositional(
  positional: string[],
  count: number,
  usage: string
): void {
  if (positional.length < count) {
    throw new Error(`Usage: ${usage}`);
  }
}

async function runLookup(parsed: ParsedArguments): Promise<number> {
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

async function runRefs(parsed: ParsedArguments): Promise<number> {
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

async function runSearch(parsed: ParsedArguments): Promise<number> {
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

async function runVerify(parsed: ParsedArguments): Promise<number> {
  if (parsed.positional.length === 0) {
    throw new Error(
      "Usage: spec.mjs verify <path...> [--refresh] [--json]"
    );
  }
  const report = await verifyPaths(parsed.positional, {
    refresh: parsed.refresh,
  });
  if (parsed.json) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } else {
    process.stdout.write(
      `${formatVerifyReport(report, process.cwd())}\n`
    );
  }
  return report.issues.length === 0 ? 0 : 1;
}

function printUsage(): void {
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

async function main(): Promise<number> {
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
