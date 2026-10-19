#!/usr/bin/env node
import { createRequire as __createRequire } from "node:module";
const require = __createRequire(import.meta.url);

var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res) => function __init() {
  return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
};
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};

// .agents/skills/browser-debug/src/cdp.ts
function resolvePort(explicitPort) {
  if (explicitPort !== void 0) return explicitPort;
  const fromEnvironment = process.env.FORMAL_WEB_CDP_PORT;
  if (fromEnvironment) {
    const parsed = Number.parseInt(fromEnvironment, 10);
    if (Number.isInteger(parsed)) return parsed;
  }
  return 9222;
}
async function listTargets(port) {
  const response = await fetch(`http://localhost:${port}/json/list`);
  if (!response.ok) {
    throw new Error(`CDP list failed: ${response.status}`);
  }
  return await response.json();
}
async function resolvePageWsUrl(port) {
  const targets = await listTargets(port);
  const page = targets.find(
    (target) => target.type === "page" && target.webSocketDebuggerUrl
  );
  if (!page?.webSocketDebuggerUrl) {
    throw new Error(`No page target found on port ${port}`);
  }
  return page.webSocketDebuggerUrl;
}
async function connect(port) {
  const wsUrl = await resolvePageWsUrl(port);
  const client = new CDPClient(wsUrl);
  await client.ready;
  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("DOM.enable").catch(() => void 0);
  await client.send("Log.enable").catch(() => void 0);
  return client;
}
function isConnectionError(error) {
  const message = error instanceof Error ? error.message : String(error);
  return message.includes("CDP socket closed") || message.includes("WebSocket") || message.includes("ECONNRESET") || message.includes("fetch failed") || message.includes("CDP list failed") || message.includes("No page target found");
}
async function withRetry(port, operation) {
  const attempt = async () => {
    const client = await connect(port);
    try {
      return await operation(client);
    } finally {
      client.close();
    }
  };
  try {
    return await attempt();
  } catch (error) {
    if (!isConnectionError(error)) throw error;
    return attempt();
  }
}
async function jsEval(client, expression) {
  const { result, exceptionDetails } = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true
  });
  if (exceptionDetails) {
    throw new Error(
      exceptionDetails.exception?.description ?? exceptionDetails.text
    );
  }
  return result.value;
}
function waitForLoad(client, timeoutMs = 8e3) {
  return new Promise((resolve) => {
    const off = client.onEvent("Page.loadEventFired", () => {
      off();
      resolve();
    });
    setTimeout(() => {
      off();
      resolve();
    }, timeoutMs);
  });
}
function waitForNavigation(client, timeoutMs = 8e3) {
  return new Promise((resolve) => {
    const off = client.onEvent("Page.frameNavigated", (params) => {
      if (params?.frame?.parentId == null) {
        off();
        resolve();
      }
    });
    setTimeout(() => {
      off();
      resolve();
    }, timeoutMs);
  });
}
async function getBoundingRect(client, selector) {
  const rect = await jsEval(
    client,
    `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return null;
      const bounds = element.getBoundingClientRect();
      return { left: bounds.left, top: bounds.top, width: bounds.width, height: bounds.height };
    })()`
  );
  if (!rect) {
    throw new Error(`Element not found: ${selector}`);
  }
  return rect;
}
async function tryCdpInput(client, method, params) {
  try {
    await client.send(method, params);
    return true;
  } catch {
    return false;
  }
}
async function jsSetInputValue(client, selector, text) {
  const applied = await jsEval(
    client,
    `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return false;
      const nativeSetter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype, 'value'
      )?.set;
      if (nativeSetter) {
        nativeSetter.call(element, ${JSON.stringify(text)});
      } else {
        element.value = ${JSON.stringify(text)};
      }
      element.dispatchEvent(new Event('input', { bubbles: true }));
      element.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    })()`
  );
  if (!applied) {
    throw new Error(`Element not found: ${selector}`);
  }
}
async function jsHoverElement(client, selector) {
  const hovered = await jsEval(
    client,
    `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return false;
      element.dispatchEvent(new MouseEvent('mouseover', { bubbles: true, relatedTarget: null }));
      element.dispatchEvent(new MouseEvent('mouseenter', { bubbles: false, relatedTarget: null }));
      return true;
    })()`
  );
  if (!hovered) {
    throw new Error(`Element not found: ${selector}`);
  }
}
async function jsUnhoverElement(client, selector) {
  await jsEval(
    client,
    `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return;
      element.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: null }));
      element.dispatchEvent(new MouseEvent('mouseleave', { bubbles: false, relatedTarget: null }));
    })()`
  );
}
var CDPClient;
var init_cdp = __esm({
  ".agents/skills/browser-debug/src/cdp.ts"() {
    CDPClient = class {
      socket = null;
      pending = /* @__PURE__ */ new Map();
      eventHandlers = /* @__PURE__ */ new Map();
      messageId = 0;
      readyPromise;
      resolveReady = null;
      rejectReady = null;
      isClosed = false;
      constructor(wsUrl) {
        this.readyPromise = new Promise((resolve, reject) => {
          this.resolveReady = resolve;
          this.rejectReady = reject;
        });
        this.connect(wsUrl);
      }
      connect(wsUrl) {
        this.isClosed = false;
        this.socket = new WebSocket(wsUrl);
        this.socket.addEventListener("open", () => {
          this.resolveReady?.();
          this.resolveReady = null;
          this.rejectReady = null;
        });
        this.socket.addEventListener("error", () => {
          const error = new Error("CDP WebSocket error");
          this.rejectReady?.(error);
          this.resolveReady = null;
          this.rejectReady = null;
        });
        this.socket.addEventListener("message", (event) => {
          const raw = typeof event.data === "string" ? event.data : "";
          if (!raw) return;
          let message;
          try {
            message = JSON.parse(raw);
          } catch {
            return;
          }
          if (message.id != null && this.pending.has(message.id)) {
            const pending = this.pending.get(message.id);
            this.pending.delete(message.id);
            if (message.error) {
              pending.reject(new Error(message.error.message));
            } else {
              pending.resolve(message.result ?? {});
            }
            return;
          }
          if (message.method) {
            for (const handler of this.eventHandlers.get(message.method) ?? []) {
              handler(message.params);
            }
          }
        });
        this.socket.addEventListener("close", () => {
          this.isClosed = true;
          const error = new Error("CDP socket closed");
          for (const pending of this.pending.values()) {
            pending.reject(error);
          }
          this.pending.clear();
        });
      }
      get ready() {
        return this.readyPromise;
      }
      get closed() {
        return this.isClosed;
      }
      async send(method, params = {}) {
        if (this.isClosed || !this.socket) {
          throw new Error("CDP socket closed");
        }
        return new Promise((resolve, reject) => {
          const id = ++this.messageId;
          this.pending.set(id, {
            resolve: (value) => resolve(value),
            reject
          });
          try {
            this.socket.send(JSON.stringify({ id, method, params }));
          } catch (error) {
            this.pending.delete(id);
            reject(error instanceof Error ? error : new Error(String(error)));
          }
        });
      }
      onEvent(method, handler) {
        if (!this.eventHandlers.has(method)) {
          this.eventHandlers.set(method, []);
        }
        this.eventHandlers.get(method).push(handler);
        return () => {
          const handlers = this.eventHandlers.get(method);
          if (!handlers) return;
          const index = handlers.indexOf(handler);
          if (index !== -1) {
            handlers.splice(index, 1);
          }
        };
      }
      close() {
        this.isClosed = true;
        if (this.socket) {
          this.socket.close();
          this.socket = null;
        }
      }
    };
  }
});

// .agents/skills/browser-debug/src/browser.ts
import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
var require_browser = __commonJS({
  ".agents/skills/browser-debug/src/browser.ts"() {
    init_cdp();
    var MAX_TEXT = 4e3;
    function parseArguments(args) {
      const positional = [];
      const options = /* @__PURE__ */ new Map();
      for (let index = 0; index < args.length; index += 1) {
        const argument = args[index];
        if (argument.startsWith("--")) {
          const name = argument.slice(2);
          const next = args[index + 1];
          if (next !== void 0 && !next.startsWith("--")) {
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
      if (value === void 0 || value === true) return fallback;
      const number = Number.parseFloat(value);
      if (!Number.isFinite(number)) {
        throw new Error(`Invalid --${name} value: ${value}`);
      }
      return number;
    }
    function optionString(parsed, name) {
      const value = parsed.options.get(name);
      return typeof value === "string" ? value : void 0;
    }
    function optionFlag(parsed, name) {
      return parsed.options.get(name) === true;
    }
    function requirePositional(parsed, count, usage) {
      if (parsed.positional.length < count) {
        throw new Error(`Usage: ${usage}`);
      }
    }
    function truncate(text2, max = MAX_TEXT) {
      if (text2.length <= max) return text2;
      return `${text2.slice(0, max)}
[truncated]`;
    }
    function jsonSerializable(value) {
      if (value === void 0) return null;
      try {
        return JSON.parse(JSON.stringify(value));
      } catch {
        return String(value);
      }
    }
    function printJson(value) {
      process.stdout.write(`${JSON.stringify(value, null, 2)}
`);
    }
    function printText(value) {
      process.stdout.write(`${value}
`);
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
          ignoreCache: optionFlag(parsed, "ignore-cache")
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
        const rect = await getBoundingRect(client, selector);
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        await client.send("Input.dispatchMouseEvent", {
          type: "mousePressed",
          x,
          y,
          button: "left",
          clickCount: 1
        });
        await client.send("Input.dispatchMouseEvent", {
          type: "mouseReleased",
          x,
          y,
          button: "left",
          clickCount: 1
        });
        await new Promise((resolve) => setTimeout(resolve, 60));
        printJson({ selector });
      });
    }
    async function typeText(parsed, port) {
      requirePositional(parsed, 2, "browser.mjs type <selector> <text>");
      const selector = parsed.positional[0];
      const text2 = parsed.positional.slice(1).join(" ");
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
        await jsSetInputValue(client, selector, text2);
        printJson({ selector, length: text2.length });
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
        await tryCdpInput(client, "Input.dispatchMouseEvent", {
          type: "mouseMoved",
          x,
          y
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
        const expression = selector ? `document.querySelector(${JSON.stringify(selector)})?.innerText ?? ""` : "document.body.innerText";
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
      const outputPath = optionString(parsed, "out") ?? join(tmpdir(), `formal-web-screenshot-${Date.now()}.png`);
      await withRetry(port, async (client) => {
        const { data } = await client.send(
          "Page.captureScreenshot",
          { format: "png" }
        );
        await writeFile(outputPath, Buffer.from(data, "base64"));
        printJson({ path: outputPath });
      });
    }
    async function captureConsole(parsed, port) {
      const durationMs = optionNumber(parsed, "ms", 1e3);
      await withRetry(port, async (client) => {
        const logs = [];
        const off = client.onEvent("Runtime.consoleAPICalled", (event) => {
          const line = (event.args ?? []).map((argument) => argument.value ?? argument.description ?? "").join(" ");
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
            url: target.url
          }))
        });
        return true;
      } catch (error) {
        printJson({
          port,
          reachable: false,
          error: error instanceof Error ? error.message : String(error),
          targets: []
        });
        return false;
      }
    }
    async function scroll(parsed, port) {
      const positionalDelta = parsed.positional[0];
      const resolvedDeltaY = positionalDelta !== void 0 ? Number.parseFloat(positionalDelta) : NaN;
      if (!Number.isFinite(resolvedDeltaY)) {
        throw new Error("Usage: browser.mjs scroll <deltaY> [x] [y]");
      }
      await withRetry(port, async (client) => {
        const viewport = await jsEval(
          client,
          "({ w: window.innerWidth, h: window.innerHeight })"
        );
        const x = parsed.positional[1] !== void 0 ? Number.parseFloat(parsed.positional[1]) : (viewport?.w ?? 0) / 2;
        const y = parsed.positional[2] !== void 0 ? Number.parseFloat(parsed.positional[2]) : (viewport?.h ?? 0) / 2;
        const deltaX = optionNumber(parsed, "delta-x", 0);
        await client.send("Input.dispatchMouseEvent", {
          type: "mouseWheel",
          x,
          y,
          deltaX,
          deltaY: resolvedDeltaY
        });
        await new Promise((resolve) => setTimeout(resolve, 120));
        printJson({ x, y, deltaX, deltaY: resolvedDeltaY });
      });
    }
    var USAGE = [
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
      ""
    ].join("\n");
    async function main() {
      const [command, ...rest] = process.argv.slice(2);
      if (!command) {
        process.stderr.write(USAGE);
        return 1;
      }
      const parsed = parseArguments(rest);
      const port = resolvePort(
        parsed.options.has("port") ? Number.parseInt(String(parsed.options.get("port")), 10) : void 0
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
          return await status(port) ? 0 : 1;
        case "scroll":
          await scroll(parsed, port);
          return 0;
        default:
          process.stderr.write(USAGE);
          return 1;
      }
    }
    main().then((exitCode) => {
      process.exitCode = exitCode;
    }).catch((error) => {
      process.stderr.write(
        `${error instanceof Error ? error.message : String(error)}
`
      );
      process.exitCode = 1;
    });
  }
});
export default require_browser();
