import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { collectSession } from "./collect.ts";

export default function (pi: ExtensionAPI) {
  // ─── /collect-session command ─────────────────────────────────────────
  pi.registerCommand("collect-session", {
    description:
      "Collect the current pi session to .pi/collected-sessions/ for archival. " +
      "Does not upload or share the session.",
    handler: async (_args, ctx) => {
      const sessionFile = ctx.sessionManager.getSessionFile();
      if (!sessionFile) {
        ctx.ui.notify("No session file available (ephemeral session)", "error");
        return;
      }

      const header = ctx.sessionManager.getHeader();
      const entries = ctx.sessionManager.getEntries();

      const result = collectSession(sessionFile, header, entries, ctx.cwd);
      if (!result) {
        ctx.ui.notify("Failed to collect session (file not found or unreadable)", "error");
        return;
      }

      ctx.ui.notify(
        `Collected session: ${result.sessionName} (${result.sizeBytes} bytes, ${entries.length} entries)`,
        "info",
      );
    },
  });

  // ─── collect_session tool (callable by the LLM) ───────────────────────
  pi.registerTool({
    name: "collect_session",
    label: "Collect Session",
    description:
      "Copy the current pi session file to .pi/collected-sessions/ for archival. " +
      "Use this at the end of a task to preserve the session trace. " +
      "Does not upload or share the session — it stays local.",
    parameters: Type.Object({}),
    async execute(_toolCallId, _params, _signal, _onUpdate, ctx) {
      const sessionFile = ctx.sessionManager.getSessionFile();
      if (!sessionFile) {
        return {
          content: [
            {
              type: "text" as const,
              text: "No session file available (ephemeral session). Cannot collect.",
            },
          ],
          details: { collected: false },
        };
      }

      const header = ctx.sessionManager.getHeader();
      const entries = ctx.sessionManager.getEntries();

      const result = collectSession(sessionFile, header, entries, ctx.cwd);
      if (!result) {
        return {
          content: [
            {
              type: "text" as const,
              text: "Failed to collect session: session file not found or unreadable.",
            },
          ],
          details: { collected: false },
        };
      }

      return {
        content: [
          {
            type: "text" as const,
            text:
              `Session collected successfully.\n` +
              `  Session: ${result.sessionName}\n` +
              `  File: ${result.fileName}\n` +
              `  Size: ${result.sizeBytes} bytes (${entries.length} entries)\n` +
              `  Hash: ${result.hash}\n` +
              `  Path: ${result.collectedPath}`,
          },
        ],
        details: {
          collected: true,
          sessionName: result.sessionName,
          fileName: result.fileName,
          sizeBytes: result.sizeBytes,
          hash: result.hash,
          collectedPath: result.collectedPath,
          entryCount: entries.length,
        },
      };
    },
  });

  // ─── upload_session tool (stub — not yet implemented) ────────────────
  pi.registerTool({
    name: "upload_session",
    label: "Upload Session",
    description:
      "Upload collected sessions to a remote destination (e.g. Hugging Face dataset). " +
      "Stub — not yet implemented. This will be wired up in a future update.",
    parameters: Type.Object({}),
    async execute(_toolCallId, _params, _signal, _onUpdate, _ctx) {
      return {
        content: [
          {
            type: "text" as const,
            text:
              "Upload is not yet implemented. " +
              "Sessions are already collected locally in .pi/collected-sessions/. " +
              "A future version of this extension will support uploading to remote destinations.",
          },
        ],
        details: { uploaded: false, reason: "not_implemented" },
      };
    },
  });
}
