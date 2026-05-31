import { createHash, randomBytes } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import type { SessionHeader } from "@earendil-works/pi-coding-agent";
import type { SessionEntry } from "@earendil-works/pi-coding-agent";

export interface CollectResult {
  /** The session display name (basename without .jsonl). */
  sessionName: string;
  /** The session file name (e.g. "2026-05-28T10-00-00_abc123.jsonl"). */
  fileName: string;
  /** Full path where the session was copied to. */
  collectedPath: string;
  /** SHA-256 hash of the collected file (sha256:xxx). */
  hash: string;
  /** File size in bytes. */
  sizeBytes: number;
}

/**
 * Collect a pi session file to a local archive directory.
 *
 * Uses in-memory session data (header + entries) to reconstruct the JSONL file.
 * This avoids the race condition of copying a file that pi may be concurrently appending to.
 *
 * @param sessionFile - Full path to the session file (used for file naming).
 * @param header      - Session header from sessionManager.getHeader().
 * @param entries     - Session entries from sessionManager.getEntries().
 * @param cwd         - Current working directory (used to locate `.pi/collected-sessions/`).
 * @returns CollectResult on success, undefined if something went wrong.
 */
export function collectSession(
  sessionFile: string,
  header: SessionHeader | null,
  entries: SessionEntry[],
  cwd: string,
): CollectResult | undefined {
  const projectRoot = findProjectRoot(cwd);
  if (!projectRoot) return undefined;

  const archiveDir = path.join(projectRoot, ".pi", "collected-sessions");
  fs.mkdirSync(archiveDir, { recursive: true });

  const originalName = path.basename(sessionFile);
  if (!originalName.endsWith(".jsonl")) return undefined;

  // Generate a unique file name by appending a timestamp and random suffix.
  // This ensures every collection creates a separate file, even when called
  // multiple times against the same pi session file.
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const rand = randomBytes(4).toString("hex");
  const stem = originalName.replace(/\.jsonl$/, "");
  const fileName = `${stem}_${timestamp}_${rand}.jsonl`;

  const destPath = path.join(archiveDir, fileName);

  // Serialize from in-memory data to avoid race conditions with concurrent file writes.
  let jsonlContent: string;
  try {
    const lines: string[] = [];
    // First line: session header
    if (header) {
      lines.push(JSON.stringify(header));
    }
    // Remaining lines: session entries
    for (const entry of entries) {
      lines.push(JSON.stringify(entry));
    }
    jsonlContent = lines.join("\n");
    if (lines.length > 0) {
      jsonlContent += "\n";
    }
  } catch {
    return undefined;
  }

  try {
    fs.writeFileSync(destPath, jsonlContent, "utf-8");
  } catch {
    return undefined;
  }

  // Compute SHA-256 hash of the collected content.
  const hash = sha256Content(jsonlContent);

  return {
    sessionName: fileName.replace(/\.jsonl$/, ""),
    fileName,
    collectedPath: destPath,
    hash,
    sizeBytes: Buffer.byteLength(jsonlContent, "utf-8"),
  };
}

function sha256Content(content: string): string {
  const hash = createHash("sha256");
  hash.update(content, "utf-8");
  return `sha256:${hash.digest("hex")}`;
}

/**
 * Walk up from `startDir` to find the first directory containing a `.pi` subdirectory.
 * This is the project root where pi stores its local extensions and where we
 * place the collected sessions archive.
 */
export function findProjectRoot(startDir: string): string | undefined {
  let current = path.resolve(startDir);
  // Limit traversal depth to avoid infinite loops.
  for (let i = 0; i < 32; i++) {
    if (fs.existsSync(path.join(current, ".pi"))) {
      return current;
    }
    const parent = path.dirname(current);
    if (parent === current) return undefined; // Reached filesystem root.
    current = parent;
  }
  return undefined;
}
