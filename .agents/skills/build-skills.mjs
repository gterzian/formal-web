import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const bundleDirectory = new URL(".", import.meta.url);
const banner = await readFile(
  new URL("esm-banner.txt", bundleDirectory),
  "utf-8"
);

const entries = [
  ["web-standards/src/spec.ts", "web-standards/scripts/spec.mjs"],
  ["readme-chain/src/readme-chain.ts", "readme-chain/scripts/readme-chain.mjs"],
  ["browser-debug/src/browser.ts", "browser-debug/scripts/browser.mjs"],
];

for (const [entry, output] of entries) {
  await build({
    entryPoints: [fileURLToPath(new URL(entry, bundleDirectory))],
    outfile: fileURLToPath(new URL(output, bundleDirectory)),
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node22",
    banner: { js: banner },
    logLevel: "info",
  });
}
