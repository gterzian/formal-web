/**
 * rust-analyzer Pi extension
 *
 * Spawns rust-analyzer as a child process and communicates via LSP over stdio.
 *
 * Requirements:
 *   - rust-analyzer on PATH (or set RA_PATH env var to its absolute path)
 *     Installed at /Users/Gregory/.cargo/bin/rust-analyzer
 *   - A Rust project (Cargo.toml) in or above ctx.cwd
 *
 * Tools
 *   ra_diagnostics      — compiler errors, warnings, Clippy
 *   ra_hover            — type info and docs at a position
 *   ra_definition       — go to definition
 *   ra_type_definition  — go to the type's definition (not the binding)
 *   ra_implementation   — find all impl blocks for a trait or type
 *   ra_references       — find all usages of a symbol
 *   ra_rename           — project-wide rename (returns WorkspaceEdit, doesn't write)
 *   ra_symbols          — fuzzy-search symbols across the workspace
 *   ra_file_structure   — outline of all symbols defined in a file
 *   ra_inlay_hints      — inferred types and parameter labels for a line range
 *   ra_expand_macro     — fully expand a macro at a position
 *   ra_code_actions     — list available assists/quick-fixes at a position
 *   ra_apply_action     — apply a specific code action by index
 *   ra_ssr              — structural search and replace across the workspace
 *   ra_call_hierarchy   — incoming and outgoing calls for a function
 *
 * Note: ra_apply_action and ra_ssr return WorkspaceEdits for the agent to apply
 * via the built-in write/edit tools. They do not write files themselves.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import {
  truncateHead,
  truncateTail,
  DEFAULT_MAX_BYTES,
  DEFAULT_MAX_LINES,
} from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { spawn, type ChildProcess } from "node:child_process";
import * as path from "node:path";
import { readFileSync } from "node:fs";

// ─────────────────────────────────────────────────────────────────────────────
// LSP types
// ─────────────────────────────────────────────────────────────────────────────

interface LspRequest {
  resolve: (value: unknown) => void;
  reject: (err: Error) => void;
}

interface Location {
  uri: string;
  range: Range;
}

interface Range {
  start: Position;
  end: Position;
}

interface Position {
  line: number;
  character: number;
}

interface Diagnostic {
  range: Range;
  severity?: number; // 1=Error 2=Warning 3=Info 4=Hint
  message: string;
  code?: string | number;
}

interface WorkspaceEdit {
  changes?: Record<string, Array<{ range: Range; newText: string }>>;
  documentChanges?: Array<{
    textDocument: { uri: string };
    edits: Array<{ range: Range; newText: string }>;
  }>;
}

interface CodeAction {
  title: string;
  kind?: string;
  edit?: WorkspaceEdit;
  command?: { title: string; command: string; arguments?: unknown[] };
}

/** Callback for status updates, so the extension can update ui.setStatus. */
type StatusCallback = (status: string) => void;

interface CallHierarchyItem {
  name: string;
  kind: number;
  uri: string;
  range: Range;
  selectionRange: Range;
}

// ─────────────────────────────────────────────────────────────────────────────
// LSP client — communicates with rust-analyzer over stdio
// ─────────────────────────────────────────────────────────────────────────────

const READINESS_POLL_MS = 2_000;

class RustAnalyzerClient {
  private proc: ChildProcess;
  private msgId = 1;
  private pending = new Map<number, LspRequest>();
  private initialized = false;
  private initPromise: Promise<void>;
  private buffer = "";

  private _projectLoaded = false;
  private _projectLoadingStarted = false;
  private _projectReadyPromise: Promise<void> | null = null;
  private _onStatus: StatusCallback | null = null;
  private _pollAttempts = 0;
  private _latestProgress: string | null = null;
  private _stderrBuffer = "";

  constructor(private rootUri: string, onStatus?: StatusCallback) {
    this._onStatus = onStatus ?? null;
    const binary =
      process.env.RA_PATH ??
      (() => {
        // Default well-known install locations
        const candidates = [
          "/Users/Gregory/.cargo/bin/rust-analyzer",
          "/usr/local/bin/rust-analyzer",
          "/opt/homebrew/bin/rust-analyzer",
        ];
        for (const c of candidates) {
          try {
            readFileSync(c);
            return c;
          } catch {
            continue;
          }
        }
        return "rust-analyzer"; // fallback — rely on PATH
      })();

    this.proc = spawn(binary, [], {
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, RUST_LOG: "" },
    });

    this.proc.stderr?.on("data", (chunk: Buffer) => {
      const text = chunk.toString();
      this._stderrBuffer += text;
      if (this._stderrBuffer.length > 4096) {
        this._stderrBuffer = this._stderrBuffer.slice(-4096);
      }
      const lines = text.split("\n").filter(l => l.trim());
      for (const line of lines) {
        if (RustAnalyzerClient._isNoise(line)) continue;

        const msgMatch = line.match(/\].*?(\w[\w\s()._\/-]+)$/);
        const message = msgMatch ? msgMatch[1].trim() : line.trim();

        if (RustAnalyzerClient._isProgressMessage(message)) {
          this._latestProgress = message;
          this._reportStatusFromProgress();
        }
      }
    });

    this.proc.stdout?.on("data", (chunk: Buffer) => {
      this.buffer += chunk.toString();
      this.pump();
    });

    this.proc.on("exit", (code) => {
      // Reject all pending requests
      for (const [, req] of this.pending) {
        req.reject(new Error(`rust-analyzer exited with code ${code}`));
      }
      this.pending.clear();
    });

    this.initPromise = this.initialize().then(() => {
      this._startProjectLoadingDetection();
    });
  }

  // ── LSP framing parser (Content-Length based) ──────────────────────────────

  private pump(): void {
    while (true) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) break;

      const header = this.buffer.slice(0, headerEnd);
      const lengthMatch = header.match(/Content-Length: (\d+)/i);
      if (!lengthMatch) {
        this.buffer = this.buffer.slice(headerEnd + 4);
        continue;
      }

      const length = parseInt(lengthMatch[1], 10);
      const bodyStart = headerEnd + 4;
      if (this.buffer.length < bodyStart + length) break;

      const body = this.buffer.slice(bodyStart, bodyStart + length);
      this.buffer = this.buffer.slice(bodyStart + length);

      try {
        const msg = JSON.parse(body);
        if (msg.id !== undefined && this.pending.has(msg.id)) {
          const { resolve, reject } = this.pending.get(msg.id)!;
          this.pending.delete(msg.id);
          if (msg.error) {
            reject(new Error(msg.error.message ?? JSON.stringify(msg.error)));
          } else {
            resolve(msg.result);
          }
        } else if (msg.method === "window/progress" || msg.method === "window/workDoneProgress/create") {
          const params = msg.params as any;
          // Progress end: token work is done, which may mean the project is loaded
          if (params?.value?.kind === "end") {
            if (!this._projectLoaded) {
              this._projectLoaded = true;
              this._reportStatus("ra: ready");
            }
            return;
          }
          const raw = params?.value?.title || params?.title || "";
          if (!RustAnalyzerClient._isNoise(raw)) {
            this._latestProgress = raw;
            if (params?.value?.message) {
              this._latestProgress += `: ${params.value.message}`;
            }
            this._reportStatusFromProgress();
          }
        }
        // Other notifications are ignored — we pull on demand.
      } catch {
        // malformed JSON — skip
      }
    }
  }

  // ── Wire helpers ──────────────────────────────────────────────────────────

  private send(method: string, params: unknown, id?: number): void {
    const msg: Record<string, unknown> = { jsonrpc: "2.0", method, params };
    if (id !== undefined) msg.id = id;
    const body = JSON.stringify(msg);
    const frame = `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`;
    this.proc.stdin!.write(frame);
  }

  private request<T>(
    method: string,
    params: unknown,
    signal?: AbortSignal,
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      if (signal?.aborted) {
        reject(new Error("Aborted"));
        return;
      }
      const id = this.msgId++;
      const cleanup = () => {
        this.pending.delete(id);
        reject(new Error("Aborted"));
      };
      signal?.addEventListener("abort", cleanup, { once: true });
      this.pending.set(id, {
        resolve: (value) => {
          signal?.removeEventListener("abort", cleanup);
          resolve(value as T);
        },
        reject: (error) => {
          signal?.removeEventListener("abort", cleanup);
          reject(error);
        },
      });
      this.send(method, params, id);
    });
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────────

  private async initialize(): Promise<void> {
    await this.request("initialize", {
      processId: process.pid,
      rootUri: this.rootUri,
      capabilities: {
        textDocument: {
          hover: { contentFormat: ["plaintext"] },
          definition: {},
          typeDefinition: {},
          implementation: {},
          references: {},
          rename: {},
          publishDiagnostics: {},
          codeAction: {
            codeActionLiteralSupport: {
              codeActionKind: {
                valueSet: [
                  "",
                  "quickfix",
                  "refactor",
                  "refactor.extract",
                  "refactor.inline",
                  "source",
                ],
              },
            },
            resolveSupport: { properties: ["edit"] },
          },
          inlayHint: {},
          callHierarchy: {},
        },
        workspace: {
          symbol: {},
          executeCommand: { dynamicRegistration: false },
        },
      },
      initializationOptions: {
        checkOnSave: true,
      },
    });
    this.send("initialized", {});
    this.initialized = true;
  }

  async ready(): Promise<void> {
    if (!this.initialized) await this.initPromise;
  }

  private _startProjectLoadingDetection(): void {
    if (this._projectLoadingStarted) return;
    this._projectLoadingStarted = true;
    this._reportStatus("ra: loading...");

    this._projectReadyPromise = new Promise<void>((resolve) => {
      // Start checking readiness after a short delay
      setTimeout(() => this._checkReadiness(resolve, 0), READINESS_POLL_MS);
    });
  }

  private _checkReadiness(resolve: () => void, attempt = 0): void {
    if (this._projectLoaded) { resolve(); return; }

    this._pollAttempts++;
    if (this._pollAttempts % 5 === 0 || this._latestProgress) {
      this._reportStatusFromProgress();
    }

    // Phase 1 (first 30s): workspace/symbol — fast if project is small
    // Phase 2 (30s-5min): file-based check — open src/main.rs and try hover
    // Phase 3 (5min+): force ready regardless
    if (attempt < 15) {
      this._checkViaWorkspaceSymbol(resolve, attempt);
    } else if (attempt < 150) {
      this._checkViaFileAccess(resolve, attempt);
    } else {
      this._projectLoaded = true;
      this._reportStatus("ra: ready");
      resolve();
    }
  }

  private _checkViaWorkspaceSymbol(resolve: () => void, attempt: number): void {
    this.request<Array<{ name: string }> | null>("workspace/symbol", { query: "main" })
      .then((result) => {
        if (result && result.length > 0) {
          this._projectLoaded = true;
          this._reportStatus("ra: ready");
          resolve();
        } else {
          setTimeout(() => this._checkReadiness(resolve, attempt + 1), READINESS_POLL_MS);
        }
      })
      .catch(() => setTimeout(() => this._checkReadiness(resolve, attempt + 1), READINESS_POLL_MS));
  }

  private _checkViaFileAccess(resolve: () => void, attempt: number): void {
    // Open the root main.rs and check if semanticTokens works.
    // This only needs file-level parsing, not full project index.
    const testUri = `${this.rootUri}/src/main.rs`;
    try {
      const { readFileSync } = require("node:fs");
      const text = readFileSync(this.rootUri.replace("file://", "") + "/src/main.rs", "utf8");
      // Send didOpen to ensure RA processes the file
      this.send("textDocument/didOpen", {
        textDocument: { uri: testUri, languageId: "rust", version: 1, text },
      });
    } catch {}

    // Wait a moment then check with a simple request
    setTimeout(() => {
      this.request<any>("textDocument/semanticTokens/full", {
        textDocument: { uri: testUri },
      })
        .then((result) => {
          if (result && result.data && result.data.length > 0) {
            this._projectLoaded = true;
            this._reportStatus("ra: ready");
            resolve();
          } else {
            // No tokens yet — file might not be processed. Try hover instead.
            this.request<any>("textDocument/hover", {
              textDocument: { uri: testUri },
              position: { line: 0, character: 4 },
            }).then((hoverResult) => {
              // Hover returning non-null (even error msg) means file is parsed
              if (hoverResult !== null) {
                this._projectLoaded = true;
                this._reportStatus("ra: ready");
                resolve();
                return;
              }
              setTimeout(() => this._checkReadiness(resolve, attempt + 1), READINESS_POLL_MS);
            }).catch(() => {
              setTimeout(() => this._checkReadiness(resolve, attempt + 1), READINESS_POLL_MS);
            });
          }
        })
        .catch(() => setTimeout(() => this._checkReadiness(resolve, attempt + 1), READINESS_POLL_MS));
    }, 1000);
  }

  /** Known noise patterns to filter out of progress display. */
  private static _noisePatterns = [
    /notify error/i, /No path was found/i, /rust-analyzer\.toml/i,
    /rust-analyzer config file/i, /Failed to deserialize/i,
    /WARN/i, /checkOnSave/i, /invalid type/i,
  ];

  private static _isNoise(text: string): boolean {
    return RustAnalyzerClient._noisePatterns.some(p => p.test(text));
  }

  /** Activity keywords that indicate real progress. */
  private static _isProgressMessage(text: string): boolean {
    const lower = text.toLowerCase();
    return /fetch|download|build|compil|analyz|collect|check|resolv|crate|proc.macro|expand|index|load|pars|generat|resolv|metadata/i.test(lower);
  }

  private _reportStatusFromProgress(): void {
    if (this._projectLoaded) {
      this._reportStatus("ra: ready");
      return;
    }
    // Sanitize _latestProgress to remove noise
    let cleanProgress: string | null = null;
    if (this._latestProgress && !RustAnalyzerClient._isNoise(this._latestProgress)) {
      cleanProgress = this._latestProgress;
    }
    const progress = cleanProgress ? ` (${cleanProgress})` : "";
    const elapsedSec = Math.round((this._pollAttempts * READINESS_POLL_MS) / 1000);
    this._reportStatus(`ra: loading${progress} — ${elapsedSec}s`);
  }

  private _reportStatus(status: string): void {
    this._onStatus?.(status);
  }

  async waitForProject(): Promise<boolean> {
    if (this._projectLoaded) return true;
    if (this._projectReadyPromise) {
      await this._projectReadyPromise;
      return this._projectLoaded;
    }
    return false;
  }

  get projectLoaded(): boolean {
    return this._projectLoaded;
  }

  /** Check if the underlying RA process is still alive. */
  get alive(): boolean {
    return this.proc.exitCode === null && this.proc.killed === false;
  }

  setStatusCallback(cb: StatusCallback): void {
    this._onStatus = cb;
    // Immediately report current state
    if (this._projectLoaded) {
      cb("ra: ready");
    } else if (this._projectLoadingStarted) {
      this._reportStatusFromProgress();
    } else {
      cb("ra: starting...");
    }
  }

  async ensureProjectReady(): Promise<void> {
    if (!this._projectLoaded) {
      this._reportStatusFromProgress();
      // Don't block — let the tool proceed. The LSP request may return
      // null/empty if the project is still loading, and the tool handler
      // will report that appropriately.
    }
  }

  shutdown(): void {
    try {
      this.request("shutdown", null).then(() => this.send("exit", null));
    } catch {
      // best-effort
    }
  }

  // ── File helpers ──────────────────────────────────────────────────────────

  async openFile(uri: string, text: string, languageId = "rust"): Promise<void> {
    this.send("textDocument/didOpen", {
      textDocument: { uri, languageId, version: 1, text },
    });
    // Give rust-analyzer a moment to process the open notification before
    // we send a request that depends on it.
    await new Promise((r) => setTimeout(r, 300));
  }

  // ── LSP methods ───────────────────────────────────────────────────────────

  hover(
    uri: string,
    line: number,
    character: number,
    signal?: AbortSignal,
  ): Promise<{ contents: { value: string } | string } | null> {
    return this.request("textDocument/hover", {
      textDocument: { uri },
      position: { line, character },
    }, signal);
  }

  definition(
    uri: string,
    line: number,
    character: number,
    signal?: AbortSignal,
  ): Promise<Location[]> {
    return this.request("textDocument/definition", {
      textDocument: { uri },
      position: { line, character },
    }, signal);
  }

  typeDefinition(
    uri: string,
    line: number,
    character: number,
    signal?: AbortSignal,
  ): Promise<Location[]> {
    return this.request("textDocument/typeDefinition", {
      textDocument: { uri },
      position: { line, character },
    }, signal);
  }

  implementation(
    uri: string,
    line: number,
    character: number,
    signal?: AbortSignal,
  ): Promise<Location[]> {
    return this.request("textDocument/implementation", {
      textDocument: { uri },
      position: { line, character },
    }, signal);
  }

  references(
    uri: string,
    line: number,
    character: number,
    includeDeclaration: boolean,
    signal?: AbortSignal,
  ): Promise<Location[]> {
    return this.request("textDocument/references", {
      textDocument: { uri },
      position: { line, character },
      context: { includeDeclaration },
    }, signal);
  }

  diagnostics(uri: string, signal?: AbortSignal): Promise<{ items: Diagnostic[] }> {
    return this.request("textDocument/diagnostic", {
      textDocument: { uri },
    }, signal).catch(() => ({ items: [] as Diagnostic[] }));
  }

  rename(
    uri: string,
    line: number,
    character: number,
    newName: string,
    signal?: AbortSignal,
  ): Promise<WorkspaceEdit | null> {
    return this.request("textDocument/rename", {
      textDocument: { uri },
      position: { line, character },
      newName,
    }, signal);
  }

  workspaceSymbols(
    query: string,
    signal?: AbortSignal,
  ): Promise<Array<{ name: string; kind: number; location: Location }>> {
    return this.request("workspace/symbol", { query }, signal);
  }

  documentSymbols(
    uri: string,
    signal?: AbortSignal,
  ): Promise<
    Array<{
      name: string;
      kind: number;
      range: Range;
      selectionRange: Range;
      children?: unknown[];
    }>
  > {
    return this.request("textDocument/documentSymbol", {
      textDocument: { uri },
    }, signal);
  }

  inlayHints(
    uri: string,
    start: Position,
    end: Position,
    signal?: AbortSignal,
  ): Promise<
    Array<{
      position: Position;
      label: string | Array<{ value: string }>;
    }>
  > {
    return this.request("textDocument/inlayHint", {
      textDocument: { uri },
      range: { start, end },
    }, signal).catch(() => []);
  }

  expandMacro(
    uri: string,
    line: number,
    character: number,
    signal?: AbortSignal,
  ): Promise<{ name: string; expansion: string } | null> {
    return this.request("rust-analyzer/expandMacro", {
      textDocument: { uri },
      position: { line, character },
    }, signal).catch(() => null);
  }

  codeActions(
    uri: string,
    range: Range,
    signal?: AbortSignal,
  ): Promise<CodeAction[]> {
    return this.request("textDocument/codeAction", {
      textDocument: { uri },
      range,
      context: { diagnostics: [], triggerKind: 1 },
    }, signal).catch(() => [] as CodeAction[]);
  }

  resolveCodeAction(
    action: CodeAction,
    signal?: AbortSignal,
  ): Promise<CodeAction> {
    return this.request("codeAction/resolve", action, signal).catch(
      () => action,
    );
  }

  ssr(
    query: string,
    parseOnly: boolean,
    uri: string,
    position: Position,
    selections: Range[],
    signal?: AbortSignal,
  ): Promise<WorkspaceEdit> {
    return this.request("rust-analyzer/ssr", {
      query,
      parseOnly,
      textDocument: { uri },
      position,
      selections,
    }, signal);
  }

  prepareCallHierarchy(
    uri: string,
    line: number,
    character: number,
    signal?: AbortSignal,
  ): Promise<CallHierarchyItem[] | null> {
    return this.request("textDocument/prepareCallHierarchy", {
      textDocument: { uri },
      position: { line, character },
    }, signal).catch(() => null);
  }

  incomingCalls(
    item: CallHierarchyItem,
    signal?: AbortSignal,
  ): Promise<Array<{ from: CallHierarchyItem; fromRanges: Range[] }>> {
    return this.request("callHierarchy/incomingCalls", { item }, signal).catch(
      () => [],
    );
  }

  outgoingCalls(
    item: CallHierarchyItem,
    signal?: AbortSignal,
  ): Promise<Array<{ to: CallHierarchyItem; fromRanges: Range[] }>> {
    return this.request("callHierarchy/outgoingCalls", { item }, signal).catch(
      () => [],
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared state — persists across extension reloads via globalThis
// ─────────────────────────────────────────────────────────────────────────────

const GLOBAL_RA_KEY = "__ra_client";

function getRaClient(): RustAnalyzerClient | null {
  return ((globalThis as any)[GLOBAL_RA_KEY] ?? null) as RustAnalyzerClient | null;
}

function setRaClient(c: RustAnalyzerClient | null): void {
  (globalThis as any)[GLOBAL_RA_KEY] = c;
}

function clearRaClient(): void {
  const existing = getRaClient();
  if (existing) {
    existing.shutdown();
  }
  setRaClient(null);
}

function getOrCreateClient(cwd: string, onStatus?: StatusCallback): RustAnalyzerClient {
  const existing = getRaClient();
  if (existing && existing.alive) {
    if (onStatus) existing.setStatusCallback(onStatus);
    return existing;
  }
  // Stale client — discard it
  setRaClient(null);

  // Kill orphaned rust-analyzer processes before starting a new one
  try {
    const { execSync } = require("node:child_process");
    execSync("pkill -f \"rust-analyzer\" 2>/dev/null; true");
  } catch {}

  const ra = new RustAnalyzerClient(`file://${cwd}`, onStatus);
  setRaClient(ra);
  return ra;
}

// ─────────────────────────────────────────────────────────────────────────────
// Formatting and display helpers
// ─────────────────────────────────────────────────────────────────────────────

/** Strip leading @ that some models incorrectly prepend to path arguments. */
function normalizePath(filePath: string): string {
  return filePath.startsWith("@") ? filePath.slice(1) : filePath;
}

function severityLabel(severity?: number): string {
  return severity === 1
    ? "error"
    : severity === 2
      ? "warning"
      : severity === 3
        ? "info"
        : "hint";
}

function uriToRelative(uri: string, cwd: string): string {
  return path.relative(cwd, uri.replace(/^file:\/\//, "")) || uri;
}

function formatLocation(loc: Location, cwd: string): string {
  return `${uriToRelative(loc.uri, cwd)}:${loc.range.start.line + 1}:${loc.range.start.character + 1}`;
}

/** Resolve a file path, open it in rust-analyzer, and return uri + text. */
async function openFile(
  ra: RustAnalyzerClient,
  filePath: string,
  cwd: string,
): Promise<{ abs: string; uri: string; text: string }> {
  const absolutePath = path.resolve(cwd, normalizePath(filePath));
  const uri = `file://${absolutePath}`;
  const text = readFileSync(absolutePath, "utf8");
  await ra.openFile(uri, text);
  return { abs: absolutePath, uri, text };
}

function summarizeWorkspaceEdit(edit: WorkspaceEdit, cwd: string): string[] {
  const summary: string[] = [];
  for (const [fileUri, edits] of Object.entries(edit.changes ?? {})) {
    summary.push(`${uriToRelative(fileUri, cwd)}: ${edits.length} edit(s)`);
  }
  for (const dc of edit.documentChanges ?? []) {
    summary.push(
      `${uriToRelative(dc.textDocument.uri, cwd)}: ${dc.edits.length} edit(s)`,
    );
  }
  return summary;
}

const SYMBOL_KIND: Record<number, string> = {
  1: "File",
  2: "Module",
  3: "Namespace",
  4: "Package",
  5: "Class",
  6: "Method",
  7: "Property",
  8: "Field",
  9: "Constructor",
  10: "Enum",
  11: "Interface",
  12: "Function",
  13: "Variable",
  14: "Constant",
  15: "String",
  23: "Struct",
  25: "EnumMember",
  26: "Event",
};

/** Check if rust-analyzer binary is available. */
function checkRaBinary(): string | null {
  const envPath = process.env.RA_PATH;
  if (envPath) return envPath;

  const candidates = [
    "/Users/Gregory/.cargo/bin/rust-analyzer",
    "/usr/local/bin/rust-analyzer",
    "/opt/homebrew/bin/rust-analyzer",
  ];
  for (const c of candidates) {
    try {
      readFileSync(c);
      return c;
    } catch {
      continue;
    }
  }

  // Fall back to PATH
  try {
    const which = spawn("which", ["rust-analyzer"], { stdio: "pipe" });
    return new Promise((resolve) => {
      which.stdout?.on("data", (data: Buffer) => {
        const result = data.toString().trim();
        resolve(result || null);
      });
      which.on("exit", () => resolve(null));
    });
  } catch {
    return null;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Extension entry point
// ─────────────────────────────────────────────────────────────────────────────

export default function (pi: ExtensionAPI) {
  // ── Lifecycle ────────────────────────────────────────────────────────────

  pi.on("session_start", async (_event, ctx) => {
    const binary = checkRaBinary();
    if (!binary) {
      const msg =
        "rust-analyzer binary not found on PATH. Set RA_PATH env var or install it:\n" +
        "  rustup component add rust-analyzer\n" +
        "  # or: https://rust-analyzer.github.io/";
      if (ctx.hasUI) {
        ctx.ui.notify(msg, "error");
      }
      return;
    }

    try {
      const onStatus = ctx.hasUI ? (s: string) => ctx.ui.setStatus("rust-analyzer", s) : undefined;
      const ra = getOrCreateClient(ctx.cwd, onStatus);
      await ra.ready();
    } catch (error: unknown) {
      const msg = error instanceof Error ? error.message : String(error);
      if (ctx.hasUI) {
        ctx.ui.notify(`rust-analyzer: failed to initialize: ${msg}`, "error");
      }
    }
  });

  pi.on("session_shutdown", async (event) => {
    if (event.reason !== "reload") {
      // On reload, keep RA running (globalThis reference survives).
      // On quit/new/resume/fork, kill it cleanly.
      clearRaClient();
    }
  });

  // ── Commands ─────────────────────────────────────────────────────────────

  pi.registerCommand("ra-status", {
    description: "Show rust-analyzer connection status",
    handler: async (_args, ctx) => {
      const ra = getRaClient(); if (!ra) {
        ctx.ui.notify("rust-analyzer: not running", "warning");
        return;
      }
      ctx.ui.notify(ra.projectLoaded ? "rust-analyzer: connected, project loaded" : "rust-analyzer: connected, still loading", "info");
    },
  });

  pi.registerCommand("ra-restart", {
    description: "Restart the rust-analyzer LSP server",
    handler: async (_args, ctx) => {
      clearRaClient();
      await new Promise((r) => setTimeout(r, 500));
      const onStatus = ctx.hasUI ? (s: string) => ctx.ui.setStatus("rust-analyzer", s) : undefined;
      const ra = getOrCreateClient(ctx.cwd, onStatus);
      try {
        await ra.ready();
        if (ctx.hasUI) {
          ctx.ui.notify("rust-analyzer: restarted", "info");
        }
      } catch (error: unknown) {
        const msg = error instanceof Error ? error.message : String(error);
        ctx.ui.notify(`rust-analyzer: restart failed: ${msg}`, "error");
      }
    },
  });

  pi.registerCommand("ra-wait", {
    description: "Wait for rust-analyzer to finish loading the project",
    handler: async (_args, ctx) => {
      const ra = getRaClient(); if (!ra) {
        ctx.ui.notify("rust-analyzer: not running", "warning");
        return;
      }
      ctx.ui.notify("rust-analyzer: waiting for project to load...", "info");
      const ready = await ra.waitForProject();
      ctx.ui.notify(ready ? "rust-analyzer: project loaded" : "rust-analyzer: project still loading", ready ? "info" : "warning");
    },
  });

  pi.registerCommand("ra-loading-state", {
    description: "Check rust-analyzer project loading state",
    handler: async (_args, ctx) => {
      const ra = getRaClient(); if (!ra) {
        ctx.ui.notify("rust-analyzer: not running", "warning");
        return;
      }
      const state = ra.projectLoaded ? "loaded" : "loading...";
      ctx.ui.notify(`rust-analyzer: ${state}`, ra.projectLoaded ? "info" : "warning");
    },
  });

  // ── Tool: diagnostics ────────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_diagnostics",
    label: "Diagnostics",
    description:
      "Get compiler errors, warnings, and Clippy suggestions for a Rust file via rust-analyzer.",
    promptSnippet: "Fetch diagnostics (errors/warnings/clippy) for a Rust file",
    promptGuidelines: [
      "Use ra_diagnostics to check a Rust file for errors and warnings before and after editing.",
    ],
    parameters: Type.Object({
      file: Type.String({
        description: "Path to the .rs file, relative to project root",
      }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const result = await ra.diagnostics(uri, signal ?? undefined);

      if (!result.items.length) {
        return {
          content: [
            { type: "text", text: `No diagnostics for ${params.file}` },
          ],
          details: {},
        };
      }

      const raw = result.items
        .map((d) => {
          const loc = `${params.file}:${d.range.start.line + 1}:${d.range.start.character + 1}`;
          return `[${severityLabel(d.severity).toUpperCase()}] ${loc}\n  ${d.message}${d.code ? ` (${d.code})` : ""}`;
        })
        .join("\n\n");

      const truncation = truncateHead(raw, {
        maxLines: DEFAULT_MAX_LINES,
        maxBytes: DEFAULT_MAX_BYTES,
      });
      const out = truncation.truncated
        ? `${truncation.content}\n\n[Truncated — ${result.items.length} total diagnostics]`
        : truncation.content;

      return {
        content: [{ type: "text", text: out }],
        details: { file: params.file, count: result.items.length },
      };
    },
  });

  // ── Tool: hover ──────────────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_hover",
    label: "Hover Info",
    description:
      "Get type information, documentation, and trait implementations for a symbol at a given position.",
    promptSnippet: "Get hover/type info for a symbol in a Rust file",
    promptGuidelines: [
      "Use ra_hover to look up the type, docs, or trait impls of any Rust symbol before reasoning about it.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const result = await ra.hover(
        uri,
        params.line - 1,
        params.character - 1,
        signal ?? undefined,
      );
      if (!result) throw new Error("No hover info at that position.");
      const hoverText =
        typeof result.contents === "string"
          ? result.contents
          : result.contents.value;
      return { content: [{ type: "text", text: hoverText }], details: {} };
    },
  });

  // ── Tool: go to definition ───────────────────────────────────────────────

  pi.registerTool({
    name: "ra_definition",
    label: "Go to Definition",
    description: "Find where a symbol is defined in the Rust project.",
    promptSnippet: "Find the definition of a Rust symbol",
    promptGuidelines: [
      "Use ra_definition to locate where a function, type, or trait is defined before reading or editing it.",
    ],
    parameters: Type.Object({
      file: Type.String({
        description: "Path to the .rs file containing the symbol",
      }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const locations = await ra.definition(
        uri,
        params.line - 1,
        params.character - 1,
        signal ?? undefined,
      );
      if (!locations?.length) throw new Error("Definition not found.");
      return {
        content: [
          {
            type: "text",
            text: locations
              .map((location) => formatLocation(location, ctx.cwd))
              .join("\n"),
          },
        ],
        details: {
          locations: locations.map((location) =>
            formatLocation(location, ctx.cwd),
          ),
        },
      };
    },
  });

  // ── Tool: go to type definition ──────────────────────────────────────────

  pi.registerTool({
    name: "ra_type_definition",
    label: "Go to Type Definition",
    description:
      "Navigate to the definition of the *type* of an expression — different from go-to-definition. For `let foo: MyStruct = ...`, goes to where `MyStruct` is declared, not where `foo` is bound.",
    promptSnippet: "Find the type definition of an expression in a Rust file",
    promptGuidelines: [
      "Use ra_type_definition when you want to understand what type a variable holds, not just where it was bound.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const locations = await ra.typeDefinition(
        uri,
        params.line - 1,
        params.character - 1,
        signal ?? undefined,
      );
      if (!locations?.length) throw new Error("Type definition not found.");
      return {
        content: [
          {
            type: "text",
            text: locations
              .map((location) => formatLocation(location, ctx.cwd))
              .join("\n"),
          },
        ],
        details: {
          locations: locations.map((location) =>
            formatLocation(location, ctx.cwd),
          ),
        },
      };
    },
  });

  // ── Tool: go to implementation ───────────────────────────────────────────

  pi.registerTool({
    name: "ra_implementation",
    label: "Go to Implementation",
    description:
      "Find all impl blocks for a trait or type — every `impl MyTrait for ...` or `impl MyStruct` in the project.",
    promptSnippet: "Find all impl blocks for a Rust trait or type",
    promptGuidelines: [
      "Use ra_implementation to find every concrete implementation of a trait or all inherent impl blocks for a type.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const locations = await ra.implementation(
        uri,
        params.line - 1,
        params.character - 1,
        signal ?? undefined,
      );
      if (!locations?.length) throw new Error("No implementations found.");
      const lines = locations.map((location) =>
        formatLocation(location, ctx.cwd),
      );
      return {
        content: [
          {
            type: "text",
            text: `${lines.length} implementation(s):\n${lines.join("\n")}`,
          },
        ],
        details: { count: lines.length, locations: lines },
      };
    },
  });

  // ── Tool: find references ────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_references",
    label: "Find References",
    description:
      "Find all usages of a symbol across the Rust project.",
    promptSnippet: "Find all references to a Rust symbol",
    promptGuidelines: [
      "Use ra_references before renaming or deleting a Rust symbol to understand its full impact.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
      include_declaration: Type.Optional(
        Type.Boolean({
          description:
            "Include the definition site (default true)",
        }),
      ),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const refs = await ra.references(
        uri,
        params.line - 1,
        params.character - 1,
        params.include_declaration ?? true,
        signal ?? undefined,
      );
      if (!refs?.length) throw new Error("No references found.");

      const lines = refs.map((r) => {
        const rel = uriToRelative(r.uri, ctx.cwd);
        return `${rel}:${r.range.start.line + 1}-${r.range.end.line + 1}`;
      });

      const raw = lines.join("\n");
      const truncation = truncateHead(raw, {
        maxLines: DEFAULT_MAX_LINES,
        maxBytes: DEFAULT_MAX_BYTES,
      });
      const out = truncation.truncated
        ? `${truncation.content}\n\n[Truncated — ${refs.length} total references]`
        : `${refs.length} reference(s):\n${truncation.content}`;

      return {
        content: [{ type: "text", text: out }],
        details: { count: refs.length },
      };
    },
  });

  // ── Tool: rename ─────────────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_rename",
    label: "Rename Symbol",
    description:
      "Rename a symbol across the entire Rust project. Returns the WorkspaceEdit — does NOT write files. Apply edits with the write/edit tool after reviewing.",
    promptSnippet: "Rename a Rust symbol project-wide (returns edits to apply)",
    promptGuidelines: [
      "Use ra_rename to get the full edit set for renaming a Rust symbol, then apply with the write or edit tool.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
      new_name: Type.String({
        description: "The new name for the symbol",
      }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const edit = await ra.rename(
        uri,
        params.line - 1,
        params.character - 1,
        params.new_name,
        signal ?? undefined,
      );
      if (!edit) throw new Error("Rename not applicable at that position.");

      const summary = summarizeWorkspaceEdit(edit, ctx.cwd);
      const full = JSON.stringify(edit, null, 2);
      const out = summary.length
        ? `Rename edits (apply with write/edit tool):\n${summary.join("\n")}\n\nFull WorkspaceEdit:\n${full}`
        : `WorkspaceEdit:\n${full}`;

      return {
        content: [{ type: "text", text: out }],
        details: { edit },
      };
    },
  });

  // ── Tool: workspace symbols ──────────────────────────────────────────────

  pi.registerTool({
    name: "ra_symbols",
    label: "Workspace Symbols",
    description:
      "Fuzzy-search for functions, structs, enums, traits, and other symbols across the whole Rust workspace. Append # to search all symbol kinds; append * to include dependencies (e.g. 'HashMap*', 'process#').",
    promptSnippet:
      "Search for Rust symbols by name across the workspace",
    promptGuidelines: [
      "Use ra_symbols to find where a function, struct, or trait is defined when you only know its name.",
    ],
    parameters: Type.Object({
      query: Type.String({
        description:
          "Symbol name or partial name. Append # for all kinds, * for deps.",
      }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const syms = await ra.workspaceSymbols(
        params.query,
        signal ?? undefined,
      );
      if (!syms?.length)
        throw new Error(`No symbols matching "${params.query}".`);

      const lines = syms.map((s) => {
        const rel = uriToRelative(s.location.uri, ctx.cwd);
        const kind = (SYMBOL_KIND[s.kind] ?? `kind(${s.kind})`).padEnd(12);
        return `${kind} ${s.name.padEnd(30)} ${rel}:${s.location.range.start.line + 1}`;
      });

      const raw = lines.join("\n");
      const truncation = truncateHead(raw, {
        maxLines: DEFAULT_MAX_LINES,
        maxBytes: DEFAULT_MAX_BYTES,
      });
      const out = truncation.truncated
        ? `${truncation.content}\n\n[Truncated — ${syms.length} total results. Refine your query.]`
        : truncation.content;

      return {
        content: [{ type: "text", text: out }],
        details: { count: syms.length },
      };
    },
  });

  // ── Tool: file structure ─────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_file_structure",
    label: "File Structure",
    description:
      "Get an outline of all symbols defined in a Rust file — functions, structs, enums, impl blocks, traits, constants, etc. Cheaper than workspace symbol search for understanding a single file.",
    promptSnippet:
      "Get the symbol outline/structure of a Rust file",
    promptGuidelines: [
      "Use ra_file_structure to understand what a Rust file defines before reading it in full.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const syms = await ra.documentSymbols(uri, signal ?? undefined);
      if (!syms?.length)
        throw new Error(`No symbols found in ${params.file}.`);

      type Sym = (typeof syms)[number];
      function formatSymbol(s: Sym, indent = ""): string {
        const kind = (SYMBOL_KIND[s.kind] ?? `kind(${s.kind})`).padEnd(12);
        const anyS = s as any;
        const r = anyS.range || anyS.location?.range || anyS.selectionRange || {};
        const lineNum = r.start?.line != null ? r.start.line + 1 : "?";
        const line = `${indent}${kind} ${s.name}  (line ${lineNum})`;
        const children = (s.children ?? []) as Sym[];
        return children.length
          ? line + "\n" + children.map((c) => formatSymbol(c, indent + "  ")).join("\n")
          : line;
      }

      const raw = syms.map((s) => formatSymbol(s)).join("\n");
      const truncation = truncateHead(raw, {
        maxLines: DEFAULT_MAX_LINES,
        maxBytes: DEFAULT_MAX_BYTES,
      });
      // Include raw first-symbol keys in details for debugging
      const debugInfo = syms.length > 0 ? Object.keys(syms[0] as any).join(",") : "none";
      return {
        content: [{ type: "text", text: truncation.content }],
        details: { count: syms.length, keys: debugInfo },
      };
    },
  });

  // ── Tool: inlay hints ────────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_inlay_hints",
    label: "Inlay Hints",
    description:
      "Get inlay type hints (inferred types, parameter names, return types) for a range of lines in a Rust file.",
    promptSnippet:
      "Get inlay type hints for a range of lines in a Rust file",
    promptGuidelines: [
      "Use ra_inlay_hints to see inferred types and parameter labels for a block of Rust code.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      start_line: Type.Number({ description: "1-based start line" }),
      end_line: Type.Number({ description: "1-based end line" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const hints = await ra.inlayHints(
        uri,
        { line: params.start_line - 1, character: 0 },
        { line: params.end_line, character: 0 },
        signal ?? undefined,
      );
      if (!hints.length)
        throw new Error("No inlay hints in that range.");

      const lines = hints.map((h) => {
        const label = Array.isArray(h.label)
          ? h.label.map((p) => p.value).join("")
          : h.label;
        return `  line ${h.position.line + 1}:${h.position.character + 1}  ${label}`;
      });

      return {
        content: [{ type: "text", text: lines.join("\n") }],
        details: { count: hints.length },
      };
    },
  });

  // ── Tool: expand macro ───────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_expand_macro",
    label: "Expand Macro",
    description:
      "Fully expand a macro invocation at a position, showing the generated code. Essential for understanding what derive macros, proc macros, and function-like macros actually produce.",
    promptSnippet:
      "Expand a Rust macro at a position and show the generated code",
    promptGuidelines: [
      "Use ra_expand_macro when reasoning about code that uses macros — see the generated code before editing around them.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({
        description:
          "1-based line number of the macro invocation or derive attribute",
      }),
      character: Type.Number({ description: "1-based column number" }),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);
      const result = await ra.expandMacro(
        uri,
        params.line - 1,
        params.character - 1,
        signal ?? undefined,
      );
      if (!result)
        throw new Error(
          "No macro found at that position, or expansion failed.",
        );

      const raw = `// Expansion of: ${result.name}\n${result.expansion}`;
      const truncation = truncateHead(raw, {
        maxLines: DEFAULT_MAX_LINES,
        maxBytes: DEFAULT_MAX_BYTES,
      });
      const out = truncation.truncated
        ? `${truncation.content}\n\n[Expansion truncated — macro output was very large]`
        : truncation.content;

      return {
        content: [{ type: "text", text: out }],
        details: { name: result.name },
      };
    },
  });

  // ── Tool: code actions ───────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_code_actions",
    label: "Code Actions",
    description:
      "List available code actions (quick-fixes, refactors, assists) at a position or range. Includes: add missing match arms, auto-import, fill struct fields, extract function, inline variable, add derives, and more.",
    promptSnippet:
      "List available code actions/assists at a position in a Rust file",
    promptGuidelines: [
      "Use ra_code_actions to discover structured refactoring options at a position, then apply one with ra_apply_action.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({ description: "1-based line number" }),
      character: Type.Number({ description: "1-based column number" }),
      end_line: Type.Optional(
        Type.Number({
          description: "1-based end line for a range (defaults to line)",
        }),
      ),
      end_character: Type.Optional(
        Type.Number({
          description:
            "1-based end column (defaults to character)",
        }),
      ),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);

      const start: Position = {
        line: params.line - 1,
        character: params.character - 1,
      };
      const end: Position = {
        line: (params.end_line ?? params.line) - 1,
        character: (params.end_character ?? params.character) - 1,
      };

      const actions = await ra.codeActions(
        uri,
        { start, end },
        signal ?? undefined,
      );
      if (!actions.length)
        throw new Error("No code actions available at that position.");

      const lines = actions.map(
        (a, i) =>
          `[${i}] ${a.title}${a.kind ? `  (${a.kind})` : ""}`,
      );
      return {
        content: [
          {
            type: "text",
            text: `${actions.length} action(s):\n${lines.join("\n")}\n\nUse ra_apply_action with the index to apply one.`,
          },
        ],
        details: {
          actions: actions.map((a) => ({
            title: a.title,
            kind: a.kind,
          })),
        },
      };
    },
  });

  // ── Tool: apply code action ──────────────────────────────────────────────

  pi.registerTool({
    name: "ra_apply_action",
    label: "Apply Code Action",
    description:
      "Apply a specific code action from ra_code_actions by index. Returns the WorkspaceEdit to review and apply with the write/edit tool.",
    promptSnippet:
      "Apply a code action/assist by index (from ra_code_actions)",
    promptGuidelines: [
      "Use ra_apply_action after ra_code_actions to get the edits for a specific assist, then apply with write or edit tool.",
    ],
    parameters: Type.Object({
      file: Type.String({
        description:
          "Path to the .rs file (same as used in ra_code_actions)",
      }),
      line: Type.Number({
        description:
          "1-based line number (same as in ra_code_actions)",
      }),
      character: Type.Number({
        description:
          "1-based column number (same as in ra_code_actions)",
      }),
      action_index: Type.Number({
        description:
          "Index of the action from ra_code_actions output",
      }),
      end_line: Type.Optional(
        Type.Number({
          description:
            "1-based end line (same as in ra_code_actions)",
        }),
      ),
      end_character: Type.Optional(
        Type.Number({
          description:
            "1-based end column (same as in ra_code_actions)",
        }),
      ),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);

      const start: Position = {
        line: params.line - 1,
        character: params.character - 1,
      };
      const end: Position = {
        line: (params.end_line ?? params.line) - 1,
        character: (params.end_character ?? params.character) - 1,
      };

      const actions = await ra.codeActions(
        uri,
        { start, end },
        signal ?? undefined,
      );
      const action = actions[params.action_index];
      if (!action)
        throw new Error(
          `No action at index ${params.action_index}. Run ra_code_actions first.`,
        );

      const resolved = action.edit
        ? action
        : await ra.resolveCodeAction(action, signal ?? undefined);
      const edit = resolved.edit;

      if (!edit) {
        // Command-based action — report but don't execute blindly
        return {
          content: [
            {
              type: "text",
              text: `Action "${action.title}" is a command: ${action.command?.command ?? "unknown"}\nArguments: ${JSON.stringify(action.command?.arguments ?? [])}`,
            },
          ],
          details: {},
        };
      }

      const summary = summarizeWorkspaceEdit(edit, ctx.cwd);
      const full = JSON.stringify(edit, null, 2);
      return {
        content: [
          {
            type: "text",
            text: `Action: "${action.title}"\n\nAffected files:\n${summary.join("\n")}\n\nWorkspaceEdit (apply with write/edit tool):\n${full}`,
          },
        ],
        details: { edit },
      };
    },
  });

  // ── Tool: structural search and replace ──────────────────────────────────

  pi.registerTool({
    name: "ra_ssr",
    label: "Structural Search & Replace",
    description: [
      "Structural Search and Replace across the workspace. Pattern: `before ==>> after`, `$name` wildcards match any expression/type/path.",
      "Returns a WorkspaceEdit — does NOT write files. Apply with write/edit tool.",
      "Examples:",
      "  `foo($a, $b) ==>> ($a).foo($b)`",
      "  `Arc::new($x) ==>> Rc::new($x)`",
      "  `$x.unwrap() ==>> $x.expect(\"TODO\")`",
    ].join("\n"),
    promptSnippet:
      "Structural search and replace across the Rust workspace",
    promptGuidelines: [
      "Use ra_ssr for large-scale pattern-based refactoring — more reliable than text search/replace for Rust code.",
    ],
    parameters: Type.Object({
      query: Type.String({
        description:
          "SSR pattern: `<search> ==>> <replacement>` with $name wildcards",
      }),
      file: Type.String({
        description:
          "Anchor file for resolving paths in the pattern",
      }),
      line: Type.Optional(
        Type.Number({
          description: "1-based anchor line (defaults to 1)",
        }),
      ),
      parse_only: Type.Optional(
        Type.Boolean({
          description:
            "Validate the pattern without running (default false)",
        }),
      ),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);

      const position: Position = {
        line: (params.line ?? 1) - 1,
        character: 0,
      };
      const edit = await ra.ssr(
        params.query,
        params.parse_only ?? false,
        uri,
        position,
        [],
        signal ?? undefined,
      );

      const summary = summarizeWorkspaceEdit(edit, ctx.cwd);
      if (!summary.length)
        return {
          content: [
            { type: "text", text: "SSR: no matches found." },
          ],
          details: {},
        };

      const full = JSON.stringify(edit, null, 2);
      return {
        content: [
          {
            type: "text",
            text: `SSR matches:\n${summary.join("\n")}\n\nWorkspaceEdit (apply with write/edit tool):\n${full}`,
          },
        ],
        details: { edit },
      };
    },
  });

  // ── Tool: call hierarchy ─────────────────────────────────────────────────

  pi.registerTool({
    name: "ra_call_hierarchy",
    label: "Call Hierarchy",
    description:
      "Show incoming calls (who calls this function) and outgoing calls (what it calls). Better than grepping references for understanding control flow.",
    promptSnippet:
      "Show call hierarchy (callers and callees) for a Rust function",
    promptGuidelines: [
      "Use ra_call_hierarchy to understand control flow before refactoring — who calls a function and what it calls.",
    ],
    parameters: Type.Object({
      file: Type.String({ description: "Path to the .rs file" }),
      line: Type.Number({
        description:
          "1-based line number of the function name",
      }),
      character: Type.Number({ description: "1-based column number" }),
      direction: Type.Optional(
        Type.String({
          description:
            "'incoming', 'outgoing', or 'both' (default)",
        }),
      ),
    }),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const ra = getOrCreateClient(ctx.cwd);
      await ra.ready();
      await ra.ensureProjectReady();
      const { uri } = await openFile(ra, params.file, ctx.cwd);

      const items = await ra.prepareCallHierarchy(
        uri,
        params.line - 1,
        params.character - 1,
        signal ?? undefined,
      );
      if (!items?.length)
        throw new Error(
          "No call hierarchy item at that position.",
        );

      const item = items[0];
      const dir = params.direction ?? "both";
      const parts: string[] = [
        `Function: ${item.name}  (${formatLocation({ uri: item.uri, range: item.range }, ctx.cwd)})`,
      ];

      if (dir === "incoming" || dir === "both") {
        const incoming = await ra.incomingCalls(
          item,
          signal ?? undefined,
        );
        if (incoming.length) {
          parts.push(
            `\nIncoming calls (${incoming.length} caller(s)):`,
          );
          for (const c of incoming) {
            parts.push(
              `  ${c.from.name.padEnd(30)} ${formatLocation({ uri: c.from.uri, range: c.from.range }, ctx.cwd)}  [${c.fromRanges.length} call site(s)]`,
            );
          }
        } else {
          parts.push("\nIncoming calls: none found");
        }
      }

      if (dir === "outgoing" || dir === "both") {
        const outgoing = await ra.outgoingCalls(
          item,
          signal ?? undefined,
        );
        if (outgoing.length) {
          parts.push(
            `\nOutgoing calls (${outgoing.length} callee(s)):`,
          );
          for (const c of outgoing) {
            parts.push(
              `  ${c.to.name.padEnd(30)} ${formatLocation({ uri: c.to.uri, range: c.to.range }, ctx.cwd)}`,
            );
          }
        } else {
          parts.push("\nOutgoing calls: none found");
        }
      }

      return {
        content: [{ type: "text", text: parts.join("\n") }],
        details: {},
      };
    },
  });
}
