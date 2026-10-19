import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  getBoundingRect,
  jsEval,
  jsHoverElement,
  jsSetInputValue,
  jsUnhoverElement,
  listTargets,
  resolvePort,
  tryCdpInput,
  waitForLoad,
  waitForNavigation,
  withRetry,
} from "./cdp.mjs";

const MAX_TEXT = 4000;

function parseArguments(args) {
  const positional = [];
  const options = new Map();

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument.startsWith("--")) {
      const name = argument.slice(2);
      const next = args[index + 1];
      if (next !== undefined && !next.startsWith("--")) {
        options.set(name, next);
        index += 1;
      } else {
        options.set(name, true);
      }
      continue;
    }
    positional.push(argument);
  }

  return { positional, options };
}

function optionNumber(parsed, name, fallback) {
  const value = parsed.options.get(name);
  if (value === undefined || value === true) return fallback;
  const number = Number.parseFloat(value);
  if (!Number.isFinite(number)) {
    throw new Error(`Invalid --${name} value: ${value}`);
  }
  return number;
}

function optionString(parsed, name) {
  const value = parsed.options.get(name);
  return typeof value === "string" ? value : undefined;
}

function optionFlag(parsed, name) {
  return parsed.options.get(name) === true;
}

function requirePositional(parsed, count, usage) {
  if (parsed.positional.length < count) {
    throw new Error(`Usage: ${usage}`);
  }
}

function truncate(text, max = MAX_TEXT) {
  if (text.length <= max) return text;
  return `${text.slice(0, max)}\n[truncated]`;
}

function jsonSerializable(value) {
  if (value === undefined) return null;
  try {
    return JSON.parse(JSON.stringify(value));
  } catch {
    return String(value);
  }
}

function printJson(value) {
  process.stdout.write(`${JSON.stringify(value, null, 2)}\n`);
}

function printText(value) {
  process.stdout.write(`${value}\n`);
}

async function navigate(parsed, port) {
  requirePositional(parsed, 1, "browser.mjs navigate <url>");
  const url = parsed.positional[0];
  await withRetry(port, async (client) => {
    const load = waitForLoad(client);
    await client.send("Page.navigate", { url });
    await load;
    const title = await jsEval(client, "document.title");
    printJson({ url, title });
  });
}

async function reload(parsed, port) {
  await withRetry(port, async (client) => {
    const load = waitForLoad(client);
    await client.send("Page.reload", {
      ignoreCache: optionFlag(parsed, "ignore-cache"),
    });
    await load;
    const url = await jsEval(client, "location.href");
    printJson({ url });
  });
}

async function evaluate(parsed, port) {
  requirePositional(parsed, 1, "browser.mjs eval <expression>");
  const expression = parsed.positional.join(" ");
  await withRetry(port, async (client) => {
    const value = await jsEval(client, expression);
    printJson({ value: jsonSerializable(value) });
  });
}

async function click(parsed, port) {
  requirePositional(parsed, 1, "browser.mjs click <selector>");
  const selector = parsed.positional[0];
  await withRetry(port, async (client) => {
    // Try DOM click via JS first; formal-web may not implement el.click().
    const domClicked = await jsEval(
      client,
      `(() => {
        try {
          const element = document.querySelector(${JSON.stringify(selector)});
          if (!element) return false;
          element.click();
          return true;
        } catch {
          return false;
        }
      })()`
    );
    if (domClicked) {
      await new Promise((resolve) => setTimeout(resolve, 60));
      printJson({ selector });
      return;
    }

    // Fall back to a CDP physical click.
    const rect = await getBoundingRect(client, selector);
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;
    await client.send("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x,
      y,
      button: "left",
      clickCount: 1,
    });
    await client.send("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x,
      y,
      button: "left",
      clickCount: 1,
    });
    await new Promise((resolve) => setTimeout(resolve, 60));
    printJson({ selector });
  });
}

async function typeText(parsed, port) {
  requirePositional(parsed, 2, "browser.mjs type <selector> <text>");
  const selector = parsed.positional[0];
  const text = parsed.positional.slice(1).join(" ");
  await withRetry(port, async (client) => {
    const focused = await jsEval(
      client,
      `(() => {
        const element = document.querySelector(${JSON.stringify(selector)});
        if (!element) return false;
        element.focus();
        return true;
      })()`
    );
    if (!focused) {
      throw new Error(`Element not found: ${selector}`);
    }

    if (optionFlag(parsed, "clear-first")) {
      await jsSetInputValue(client, selector, "");
    }
    await jsSetInputValue(client, selector, text);
    printJson({ selector, length: text.length });
  });
}

async function hover(parsed, port) {
  const selector = parsed.positional[0];
  await withRetry(port, async (client) => {
    if (!selector) {
      await jsUnhoverElement(client, "body");
      await new Promise((resolve) => setTimeout(resolve, 200));
      printJson({ x: 0, y: 0, selector: null });
      return;
    }

    const rect = await getBoundingRect(client, selector);
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;

    // Try CDP mouseMoved first; it works in Chrome/Chromium but may be a
    // no-op in formal-web. Verify by checking :hover state after.
    await tryCdpInput(client, "Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x,
      y,
    });

    const hoverActive = await jsEval(
      client,
      `(() => {
        const element = document.querySelector(${JSON.stringify(selector)});
        if (!element) return false;
        return element.matches(':hover');
      })()`
    );

    if (!hoverActive) {
      await jsHoverElement(client, selector);
    }

    await new Promise((resolve) => setTimeout(resolve, 200));
    printJson({ x, y, selector });
  });
}

async function text(parsed, port) {
  const selector = parsed.positional[0];
  const max = optionNumber(parsed, "max", MAX_TEXT);
  await withRetry(port, async (client) => {
    const expression = selector
      ? `document.querySelector(${JSON.stringify(selector)})?.innerText ?? ""`
      : "document.body.innerText";
    const raw = await jsEval(client, expression);
    printText(truncate(raw ?? "", max));
  });
}

async function attribute(parsed, port) {
  requirePositional(parsed, 2, "browser.mjs attr <selector> <name>");
  const selector = parsed.positional[0];
  const name = parsed.positional[1];
  await withRetry(port, async (client) => {
    const value = await jsEval(
      client,
      `document.querySelector(${JSON.stringify(selector)})?.getAttribute(${JSON.stringify(name)}) ?? null`
    );
    printText(value === null ? "(attribute not found)" : value);
  });
}

async function style(parsed, port) {
  requirePositional(parsed, 2, "browser.mjs style <selector> <property>");
  const selector = parsed.positional[0];
  const property = parsed.positional[1];
  await withRetry(port, async (client) => {
    const value = await jsEval(
      client,
      `(() => {
        const element = document.querySelector(${JSON.stringify(selector)});
        if (!element) throw new Error('Element not found: ' + ${JSON.stringify(selector)});
        return getComputedStyle(element).getPropertyValue(${JSON.stringify(property)});
      })()`
    );
    printText(value);
  });
}

async function screenshot(parsed, port) {
  const outputPath =
    optionString(parsed, "out") ??
    join(tmpdir(), `formal-web-screenshot-${Date.now()}.png`);
  await withRetry(port, async (client) => {
    const { data } = await client.send("Page.captureScreenshot", {
      format: "png",
    });
    await writeFile(outputPath, Buffer.from(data, "base64"));
    printJson({ path: outputPath });
  });
}

async function captureConsole(parsed, port) {
  const durationMs = optionNumber(parsed, "ms", 1000);
  await withRetry(port, async (client) => {
    const logs = [];
    const off = client.onEvent("Runtime.consoleAPICalled", (event) => {
      const line = (event.args ?? [])
        .map((argument) => argument.value ?? argument.description ?? "")
        .join(" ");
      logs.push({ level: event.type, text: line });
    });
    await new Promise((resolve) => setTimeout(resolve, durationMs));
    off();
    printJson({ logs });
  });
}

async function historyBack(_parsed, port) {
  await withRetry(port, async (client) => {
    const navigation = waitForNavigation(client);
    await jsEval(client, "history.back()");
    await navigation;
    const url = await jsEval(client, "location.href");
    printJson({ url });
  });
}

async function status(port) {
  try {
    const targets = await listTargets(port);
    printJson({
      port,
      reachable: true,
      targets: targets.map((target) => ({
        type: target.type,
        url: target.url,
      })),
    });
    return true;
  } catch (error) {
    printJson({
      port,
      reachable: false,
      error: error instanceof Error ? error.message : String(error),
      targets: [],
    });
    return false;
  }
}

async function scroll(parsed, port) {
  const positionalDelta = parsed.positional[0];
  const resolvedDeltaY =
    positionalDelta !== undefined ? Number.parseFloat(positionalDelta) : NaN;
  if (!Number.isFinite(resolvedDeltaY)) {
    throw new Error("Usage: browser.mjs scroll <deltaY> [x] [y]");
  }

  await withRetry(port, async (client) => {
    // Default to the viewport centre so the page (not chrome) receives the
    // wheel event; formal-web routes ScrollWheel to the web content there.
    const viewport = await jsEval(
      client,
      "({ w: window.innerWidth, h: window.innerHeight })"
    );
    const x =
      parsed.positional[1] !== undefined
        ? Number.parseFloat(parsed.positional[1])
        : (viewport?.w ?? 0) / 2;
    const y =
      parsed.positional[2] !== undefined
        ? Number.parseFloat(parsed.positional[2])
        : (viewport?.h ?? 0) / 2;
    const deltaX = optionNumber(parsed, "delta-x", 0);
    await client.send("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x,
      y,
      deltaX,
      deltaY: resolvedDeltaY,
    });
    await new Promise((resolve) => setTimeout(resolve, 120));
    printJson({ x, y, deltaX, deltaY: resolvedDeltaY });
  });
}

const USAGE = [
  "Usage: browser.mjs <command> [args] [--port <n>]",
  "",
  "Commands:",
  "  navigate <url>",
  "  reload [--ignore-cache]",
  "  back",
  "  eval <expression>",
  "  click <selector>",
  "  type <selector> <text> [--clear-first]",
  "  hover [selector]",
  "  text [selector] [--max <n>]",
  "  attr <selector> <name>",
  "  style <selector> <property>",
  "  screenshot [--out <path>]",
  "  console --ms <n>",
  "  status",
  "  scroll <deltaY> [x] [y]",
  "",
].join("\n");

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  if (!command) {
    process.stderr.write(USAGE);
    return 1;
  }

  const parsed = parseArguments(rest);
  const port = resolvePort(
    parsed.options.has("port")
      ? Number.parseInt(String(parsed.options.get("port")), 10)
      : undefined
  );

  switch (command) {
    case "navigate":
      await navigate(parsed, port);
      return 0;
    case "reload":
      await reload(parsed, port);
      return 0;
    case "back":
      await historyBack(parsed, port);
      return 0;
    case "eval":
      await evaluate(parsed, port);
      return 0;
    case "click":
      await click(parsed, port);
      return 0;
    case "type":
      await typeText(parsed, port);
      return 0;
    case "hover":
      await hover(parsed, port);
      return 0;
    case "text":
      await text(parsed, port);
      return 0;
    case "attr":
      await attribute(parsed, port);
      return 0;
    case "style":
      await style(parsed, port);
      return 0;
    case "screenshot":
      await screenshot(parsed, port);
      return 0;
    case "console":
      await captureConsole(parsed, port);
      return 0;
    case "status":
      return (await status(port)) ? 0 : 1;
    case "scroll":
      await scroll(parsed, port);
      return 0;
    default:
      process.stderr.write(USAGE);
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
