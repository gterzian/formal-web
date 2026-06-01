import { writeFile } from "node:fs/promises";
import {
  getBoundingRect,
  getClient,
  jsEval,
  jsHoverElement,
  jsSetInputValue,
  jsUnhoverElement,
  noteSendFailure,
  tryCdpInput,
  waitForLoad,
  waitForNavigation,
} from "./cdp.js";

const MAX_TEXT = 4000;

function truncate(text: string): string {
  if (text.length <= MAX_TEXT) {
    return text;
  }
  return `${text.slice(0, MAX_TEXT)}\n[truncated]`;
}

function jsonSerializable(value: unknown): unknown {
  if (value === undefined) {
    return null;
  }
  try {
    return JSON.parse(JSON.stringify(value));
  } catch {
    return String(value);
  }
}

function toolSchema(properties: Record<string, unknown>, required: string[] = []) {
  return {
    type: "object",
    additionalProperties: false,
    properties,
    required,
  };
}

/**
 * Wrap a tool execute handler with automatic reconnection logic.
 * If the underlying CDP socket fails, this clears the cached client
 * and retries once before giving up.
 */
function withReconnect<T>(
  fn: () => Promise<T>
): () => Promise<T> {
  return async () => {
    try {
      return await fn();
    } catch (error: any) {
      if (
        error?.message?.includes("CDP socket closed") ||
        error?.message?.includes("WebSocket") ||
        error?.code === "ECONNRESET"
      ) {
        // Force reconnection and retry once
        noteSendFailure();
        await getClient(true);
        // Re-run the function; it will getClient() internally and pick up the fresh connection
        return await fn();
      }
      throw error;
    }
  };
}

export function registerTools(pi: any) {
  pi.registerTool({
    name: "browser_navigate",
    label: "Browser: navigate",
    description: "Navigate to a URL and wait for the page load event.",
    promptSnippet: "Navigate browser to a URL and wait for load.",
    promptGuidelines: ["Use browser_navigate when the user asks to open or go to a URL."],
    parameters: toolSchema({
      url: { type: "string", description: "Fully-qualified URL to navigate to" },
    }, ["url"]),
    async execute(_id: string, params: { url: string }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const nav = waitForLoad(client);
        await client.send("Page.navigate", { url: params.url });
        await nav;
        const title = await jsEval<string>(client, "document.title");
        return {
          content: [{ type: "text", text: truncate(`Navigated to ${params.url}\nTitle: ${title}`) }],
          details: { url: params.url, title },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_reload",
    label: "Browser: reload",
    description: "Reload the current page and wait for it to finish loading.",
    promptSnippet: "Reload the current browser page.",
    promptGuidelines: ["Use browser_reload to reset page state before a test run."],
    parameters: toolSchema({
      ignoreCache: { type: "boolean", description: "Hard reload ignoring cache (default false)" },
    }),
    async execute(_id: string, params: { ignoreCache?: boolean }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const nav = waitForLoad(client);
        await client.send("Page.reload", { ignoreCache: params.ignoreCache ?? false });
        await nav;
        const url = await jsEval<string>(client, "location.href");
        return {
          content: [{ type: "text", text: truncate(`Reloaded: ${url}`) }],
          details: { url },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_evaluate",
    label: "Browser: evaluate JS",
    description: "Evaluate a JavaScript expression in the page context and return the result as JSON.",
    promptSnippet: "Run a JavaScript expression in the browser and return the result.",
    promptGuidelines: ["Use browser_evaluate to read or mutate page state when no dedicated tool covers the case."],
    parameters: toolSchema({
      expression: { type: "string", description: "JS expression to evaluate" },
    }, ["expression"]),
    async execute(_id: string, params: { expression: string }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const value = await jsEval(client, params.expression);
        const text = truncate(JSON.stringify(jsonSerializable(value), null, 2) ?? "undefined");
        return { content: [{ type: "text", text }], details: { value: jsonSerializable(value) } };
      })();
    },
  });

  pi.registerTool({
    name: "browser_click",
    label: "Browser: click element",
    description: "Click a DOM element matched by a CSS selector using el.click().",
    promptSnippet: "Click a page element by CSS selector.",
    promptGuidelines: ["Use browser_click to interact with buttons, links, and checkboxes by CSS selector."],
    parameters: toolSchema({
      selector: { type: "string", description: "CSS selector of the element to click" },
    }, ["selector"]),
    async execute(_id: string, params: { selector: string }) {
      return await withReconnect(async () => {
        const client = await getClient();

        // First try DOM click via JS (works in Chrome; may throw in formal-web
        // if el.click is not implemented). Catch errors gracefully.
        const domClicked = await jsEval<boolean>(
          client,
          `(() => { try { const el = document.querySelector(${JSON.stringify(params.selector)}); if (!el) return false; el.click(); return true; } catch { return false; } })()`
        );
        if (domClicked) {
          await new Promise((resolve) => setTimeout(resolve, 60));
          return {
            content: [{ type: "text", text: truncate(`Clicked: ${params.selector}`) }],
            details: { selector: params.selector },
          };
        }

        // Fall back to CDP physical click.
        const rect = await getBoundingRect(client, params.selector);
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        await client.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
        await client.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
        await new Promise((resolve) => setTimeout(resolve, 60));
        return {
          content: [{ type: "text", text: truncate(`Clicked: ${params.selector}`) }],
          details: { selector: params.selector },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_type",
    label: "Browser: type text",
    description: "Focus a selector and type text into an input. Uses JS-based input setting with input/change events for framework compatibility.",
    promptSnippet: "Type text into a browser input or textarea.",
    promptGuidelines: ["Use browser_type to fill text inputs by selector before submitting a form."],
    parameters: toolSchema({
      selector: { type: "string", description: "CSS selector of the input to type into" },
      text: { type: "string", description: "Text to type" },
      clearFirst: { type: "boolean", description: "Select all and delete before typing (default false)" },
    }, ["selector", "text"]),
    async execute(_id: string, params: { selector: string; text: string; clearFirst?: boolean }) {
      return await withReconnect(async () => {
        const client = await getClient();

        // Focus the element
        const focused = await jsEval<boolean>(
          client,
          `(() => { const el = document.querySelector(${JSON.stringify(params.selector)}); if (!el) return false; el.focus(); return true; })()`
        );
        if (!focused) {
          throw new Error(`Element not found: ${params.selector}`);
        }

        // Use JS-based input value setting. This works reliably with both
        // Chrome/Chromium (full CDP) and formal-web (whose Input.dispatchKeyEvent
        // is a no-op stub). Sets the value directly and dispatches input/change
        // events for reactive framework compatibility.
        if (params.clearFirst) {
          await jsSetInputValue(client, params.selector, "");
        }
        await jsSetInputValue(client, params.selector, params.text);

        return {
          content: [{ type: "text", text: truncate(`Typed ${params.text.length} chars into ${params.selector}`) }],
          details: { selector: params.selector, length: params.text.length },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_hover",
    label: "Browser: hover over element",
    description: "Move the mouse to the centre of an element, triggering CSS :hover and mouseenter events.",
    promptSnippet: "Move the mouse over an element to trigger hover states.",
    promptGuidelines: ["Use browser_hover before browser_get_computed_style when testing CSS :hover transitions."],
    parameters: toolSchema({
      selector: { type: ["string", "null"], description: "CSS selector to hover over, or null to move mouse away" },
    }),
    async execute(_id: string, params: { selector?: string | null }) {
      return await withReconnect(async () => {
        const client = await getClient();

        if (!params.selector) {
          // Move mouse away — unhover via JS (works everywhere).
          await jsUnhoverElement(client, "body");
          await new Promise((resolve) => setTimeout(resolve, 200));
          return {
            content: [{ type: "text", text: "Moved mouse to (0, 0)" }],
            details: { x: 0, y: 0, selector: null },
          };
        }

        const rect = await getBoundingRect(client, params.selector);
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;

        // Try CDP mouseMoved first; it works in Chrome/Chromium but may be
        // a no-op in formal-web. Verify by checking :hover state after.
        await tryCdpInput(client, "Input.dispatchMouseEvent", {
          type: "mouseMoved",
          x,
          y,
        });

        // Check if the CDP command actually triggered :hover on the element.
        const hoverActive = await jsEval<boolean>(
          client,
          `(() => { const el = document.querySelector(${JSON.stringify(params.selector)}); if (!el) return false; return el.matches(':hover'); })()`
        );

        if (!hoverActive) {
          // CDP mouseMoved didn't trigger hover (formal-web) — use JS fallback.
          await jsHoverElement(client, params.selector);
        }

        await new Promise((resolve) => setTimeout(resolve, 200));
        return {
          content: [{ type: "text", text: `Hovered ${params.selector} at (${x.toFixed(0)}, ${y.toFixed(0)})` }],
          details: { x, y, selector: params.selector },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_get_text",
    label: "Browser: get text",
    description: "Return the innerText of a CSS selector, or document.body.innerText if no selector given.",
    promptSnippet: "Read visible text from the page or a specific element.",
    promptGuidelines: ["Use browser_get_text to read counters, labels, or page content by CSS selector."],
    parameters: toolSchema({
      selector: { type: "string", description: "CSS selector; omit for full page text" },
    }),
    async execute(_id: string, params: { selector?: string }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const expr = params.selector
          ? `document.querySelector(${JSON.stringify(params.selector)})?.innerText ?? ""`
          : "document.body.innerText";
        const raw = await jsEval<string>(client, expr);
        const text = truncate(raw ?? "");
        return {
          content: [{ type: "text", text }],
          details: { selector: params.selector ?? null, length: raw?.length ?? 0 },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_get_attribute",
    label: "Browser: get attribute",
    description: "Return the value of a DOM attribute on the first element matching a CSS selector.",
    promptSnippet: "Read a DOM attribute from an element.",
    promptGuidelines: ["Use browser_get_attribute to read data-*, aria-*, src, href, or any DOM attribute."],
    parameters: toolSchema({
      selector: { type: "string", description: "CSS selector" },
      attribute: { type: "string", description: "Attribute name, e.g. data-active or aria-pressed" },
    }, ["selector", "attribute"]),
    async execute(_id: string, params: { selector: string; attribute: string }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const value = await jsEval<string | null>(
          client,
          `document.querySelector(${JSON.stringify(params.selector)})?.getAttribute(${JSON.stringify(params.attribute)}) ?? null`
        );
        return {
          content: [{ type: "text", text: truncate(value === null ? "(attribute not found)" : value) }],
          details: { selector: params.selector, attribute: params.attribute, value },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_get_computed_style",
    label: "Browser: get computed style",
    description: "Return the computed CSS property value for an element.",
    promptSnippet: "Read a computed CSS property from an element.",
    promptGuidelines: ["Use browser_get_computed_style to verify CSS hover states, transitions, or theme values."],
    parameters: toolSchema({
      selector: { type: "string", description: "CSS selector" },
      property: { type: "string", description: "CSS property name, e.g. background-color or opacity" },
    }, ["selector", "property"]),
    async execute(_id: string, params: { selector: string; property: string }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const value = await jsEval<string>(
          client,
          `(() => { const el = document.querySelector(${JSON.stringify(params.selector)}); if (!el) throw new Error('Element not found: ' + ${JSON.stringify(params.selector)}); return getComputedStyle(el).getPropertyValue(${JSON.stringify(params.property)}); })()`
        );
        return {
          content: [{ type: "text", text: truncate(value) }],
          details: { selector: params.selector, property: params.property, value },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_screenshot",
    label: "Browser: screenshot",
    description: "Capture a PNG screenshot of the current page and save it to disk.",
    promptSnippet: "Take a screenshot of the current browser page.",
    promptGuidelines: ["Use browser_screenshot to capture the visual state of the page for inspection."],
    parameters: toolSchema({
      path: { type: "string", description: "Output path (default: ./screenshot.png)" },
    }),
    async execute(_id: string, params: { path?: string }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const { data } = await client.send<{ data: string }>("Page.captureScreenshot", { format: "png" });
        const outPath = params.path ?? "./screenshot.png";
        await writeFile(outPath, Buffer.from(data, "base64"));
        return {
          content: [{ type: "text", text: truncate(`Screenshot saved to ${outPath}`) }],
          details: { path: outPath },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_capture_console",
    label: "Browser: capture console logs",
    description: "Collect console.log/warn/error messages emitted by the page for a given duration.",
    promptSnippet: "Capture browser console output for N milliseconds.",
    promptGuidelines: ["Use browser_capture_console before an action to catch logs emitted during that action, e.g. beforeunload."],
    parameters: toolSchema({
      durationMs: { type: "number", description: "How long to listen in ms (default 1000)", default: 1000 },
    }),
    async execute(_id: string, params: { durationMs?: number }) {
      return await withReconnect(async () => {
        const client = await getClient();
        const logs: Array<{ level: string; text: string }> = [];
        const off = client.onEvent("Runtime.consoleAPICalled", (event: any) => {
          const text = (event.args ?? []).map((arg: any) => arg.value ?? arg.description ?? "").join(" ");
          logs.push({ level: event.type, text });
        });
        await new Promise((resolve) => setTimeout(resolve, params.durationMs ?? 1000));
        off();
        const summary = logs.length === 0 ? "(no console output)" : logs.map((entry) => `[${entry.level}] ${entry.text}`).join("\n");
        return {
          content: [{ type: "text", text: truncate(summary) }],
          details: { logs },
        };
      })();
    },
  });

  pi.registerTool({
    name: "browser_history_back",
    label: "Browser: history back",
    description: "Navigate back in browser history and wait for the page to load.",
    promptSnippet: "Go back in browser history.",
    promptGuidelines: ["Use browser_history_back to restore the previous page after testing navigation."],
    parameters: toolSchema({}, []),
    async execute() {
      return await withReconnect(async () => {
        const client = await getClient();
        const nav = waitForNavigation(client);
        await jsEval(client, "history.back()");
        await nav;
        const url = await jsEval<string>(client, "location.href");
        return {
          content: [{ type: "text", text: truncate(`Back to: ${url}`) }],
          details: { url },
        };
      })();
    },
  });
}
