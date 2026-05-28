import { disconnect, setPort } from "./cdp.js";
import { registerTools } from "./tools.js";
import { FORMALWEB_TEST_PLAN } from "./tests/formalweb.js";

export default function (pi: any) {
  registerTools(pi);

  pi.on("session_shutdown", async () => {
    disconnect();
  });

  pi.registerCommand("browser-connect", {
    description: "Connect to a running browser's CDP endpoint. Usage: /browser-connect [port]",
    handler: async (args, ctx) => {
      const port = args ? Number.parseInt(args.trim(), 10) : 9222;
      if (Number.isNaN(port)) {
        ctx.ui.notify("Invalid port number", "error");
        return;
      }
      disconnect();
      setPort(port);
      try {
        const { getClient } = await import("./cdp.js");
        await getClient();
        ctx.ui.notify(`Connected to CDP on port ${port}`, "info");
      } catch (error: any) {
        ctx.ui.notify(`Connection failed: ${error.message}`, "error");
      }
    },
  });

  pi.registerCommand("browser-disconnect", {
    description: "Disconnect from the browser CDP endpoint.",
    handler: async (_args, ctx) => {
      disconnect();
      ctx.ui.notify("Disconnected from browser", "info");
    },
  });

  pi.registerCommand("test-page", {
    description: "Run the FormalWeb page test suite using the general browser tools.",
    handler: async (_args, ctx) => {
      ctx.ui.notify("Queuing FormalWeb test suite...", "info");
      await pi.sendUserMessage(FORMALWEB_TEST_PLAN, { deliverAs: "followUp" });
    },
  });
}
