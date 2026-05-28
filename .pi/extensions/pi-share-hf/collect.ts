import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

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
 * Collect a pi session file by copying it to a local archive directory.
 *
 * @param sessionFile - Full path to the session file (from sessionManager.getSessionFile()).
 * @param cwd         - Current working directory (used to locate `.pi/collected-sessions/`).
 * @returns CollectResult on success, undefined if the session file doesn't exist or is unreadable.
 */
export function collectSession(
  sessionFile: string,
  cwd: string,
): CollectResult | undefined {
  // Resolve the archive directory relative to the project root.
  // Use .pi/collected-sessions/ inside the project root.
  const projectRoot = findProjectRoot(cwd);
  if (!projectRoot) return undefined;

  const archiveDir = path.join(projectRoot, ".pi", "collected-sessions");
  fs.mkdirSync(archiveDir, { recursive: true });

  // Determine the session file name.
  const fileName = path.basename(sessionFile);
  if (!fileName.endsWith(".jsonl")) return undefined;

  const destPath = path.join(archiveDir, fileName);

  // Copy the session file.
  if (!fs.existsSync(sessionFile)) {
    return undefined;
  }

  try {
    fs.copyFileSync(sessionFile, destPath);
  } catch {
    return undefined;
  }

  // Compute SHA-256 hash of the collected copy.
  const hash = sha256File(destPath);
  const stat = fs.statSync(destPath);

  return {
    sessionName: fileName.replace(/\.jsonl$/, ""),
    fileName,
    collectedPath: destPath,
    hash,
    sizeBytes: stat.size,
  };
}

function sha256File(filePath: string): string {
  const hash = createHash("sha256");
  const data = fs.readFileSync(filePath);
  hash.update(data);
  return `sha256:${hash.digest("hex")}`;
}

/**
 * Walk up from `startDir` to find the first directory containing a `.pi` subdirectory.
 * This is the project root where pi stores its local extensions and where we
 * place the collected sessions archive.
 */
function findProjectRoot(startDir: string): string | undefined {
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
