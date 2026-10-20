import { createHash } from "node:crypto";
import { mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { load } from "cheerio";

const CACHE_TTL_MS = 14 * 24 * 60 * 60 * 1000;

function cacheDirectory() {
  const xdgCacheHome = process.env.XDG_CACHE_HOME;
  const base =
    xdgCacheHome && xdgCacheHome.length > 0
      ? xdgCacheHome
      : join(homedir(), ".cache");
  return join(base, "formal-web", "specs");
}

function cachePathFor(url) {
  const digest = createHash("sha256").update(url).digest("hex");
  return join(cacheDirectory(), `${digest}.html`);
}

async function readFreshCache(path) {
  const entry = await stat(path).catch(() => null);
  if (!entry || !entry.isFile()) return null;
  if (Date.now() - entry.mtimeMs > CACHE_TTL_MS) return null;
  return readFile(path, "utf-8").catch(() => null);
}

async function writeCache(path, html) {
  await mkdir(cacheDirectory(), { recursive: true });
  const temporary = `${path}.${process.pid}.tmp`;
  await writeFile(temporary, html, "utf-8");
  await rename(temporary, path);
}

async function fetchSpecHtml(url, options) {
  const path = cachePathFor(url);
  if (!options.refresh) {
    const cached = await readFreshCache(path);
    if (cached !== null) return cached;
  }

  const response = await fetch(url, {
    headers: { "Accept-Encoding": "identity" },
  });
  if (!response.ok) {
    throw new Error(`Failed to fetch ${url}: HTTP ${response.status}`);
  }
  const html = await response.text();
  if (
    html.length > 0 &&
    html.charCodeAt(0) === 0x1f &&
    html.charCodeAt(1) === 0x8b
  ) {
    throw new Error(
      `Fetched ${url} returned gzip-compressed data but identity encoding was requested`
    );
  }

  try {
    await writeCache(path, html);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`warning: could not cache ${url}: ${message}\n`);
  }

  return html;
}

export async function getDoc(url, options = {}) {
  const html = await fetchSpecHtml(url, options);
  return load(html);
}
