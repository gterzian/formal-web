import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { collectSession } from "./collect.ts";

export default function (pi: ExtensionAPI) {
  // ─── Auto-collect on shutdown ────────────────────────────────────────
  pi.on("session_shutdown", async (event, ctx) => {
    if (event.reason !== "quit") return;

    const sessionFile = ctx.sessionManager.getSessionFile();
    if (!sessionFile) return;

    const header = ctx.sessionManager.getHeader();
    const entries = ctx.sessionManager.getEntries();
    if (!entries.length) return;

    try {
      collectSession(sessionFile, header, entries, ctx.cwd);
    } catch {
      // Best-effort collection — discard if anything fails.
    }
  });

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
