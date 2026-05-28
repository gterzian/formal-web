import { disconnect, getClient, jsEval, setPort, waitForLoad } from "./cdp.js";

type Args = {
  url: string;
  port: number;
};

function parseArgs(argv: string[]): Args {
  let url = "http://localhost:3000";
  let port = 9222;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") {
      const next = argv[i + 1];
      if (!next) {
        throw new Error("Missing value for --url");
      }
      url = next;
      i += 1;
      continue;
    }
    if (arg === "--port") {
      const next = argv[i + 1];
      if (!next) {
        throw new Error("Missing value for --port");
      }
      const parsed = Number.parseInt(next, 10);
      if (Number.isNaN(parsed)) {
        throw new Error(`Invalid --port value: ${next}`);
      }
      port = parsed;
      i += 1;
    }
  }

  return { url, port };
}

async function main() {
  const { url, port } = parseArgs(process.argv.slice(2));
  setPort(port);

  const client = await getClient();
  const load = waitForLoad(client);
  await client.send("Page.navigate", { url });
  await load;

  const title = await jsEval<string>(client, "document.title");
  const href = await jsEval<string>(client, "location.href");

  console.log(JSON.stringify({ port, url: href, title }, null, 2));
}

main()
  .catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  })
  .finally(() => {
    disconnect();
  });
