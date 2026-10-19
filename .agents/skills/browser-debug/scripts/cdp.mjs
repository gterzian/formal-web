import WebSocket from "ws";

export class CDPClient {
  constructor(wsUrl) {
    this.wsUrl = wsUrl;
    this.socket = null;
    this.pending = new Map();
    this.eventHandlers = new Map();
    this.messageId = 0;
    this.isClosed = false;
    this.readyPromise = new Promise((resolve, reject) => {
      this.resolveReady = resolve;
      this.rejectReady = reject;
    });
    this.connect();
  }

  connect() {
    this.isClosed = false;
    this.socket = new WebSocket(this.wsUrl);

    this.socket.on("open", () => {
      this.resolveReady?.();
      this.resolveReady = null;
      this.rejectReady = null;
    });

    this.socket.on("error", (error) => {
      this.rejectReady?.(error);
      this.resolveReady = null;
      this.rejectReady = null;
    });

    this.socket.on("message", (raw) => {
      let message;
      try {
        message = JSON.parse(raw.toString());
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

    this.socket.on("close", () => {
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
      this.pending.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }), (error) => {
        if (!error) return;
        this.pending.delete(id);
        reject(error);
      });
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
}

export function resolvePort(explicitPort) {
  if (explicitPort !== undefined) return explicitPort;
  const fromEnvironment = process.env.FORMAL_WEB_CDP_PORT;
  if (fromEnvironment) {
    const parsed = Number.parseInt(fromEnvironment, 10);
    if (Number.isInteger(parsed)) return parsed;
  }
  return 9222;
}

export async function listTargets(port) {
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

export async function connect(port) {
  const wsUrl = await resolvePageWsUrl(port);
  const client = new CDPClient(wsUrl);
  await client.ready;

  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("DOM.enable").catch(() => undefined);
  await client.send("Log.enable").catch(() => undefined);

  return client;
}

function isConnectionError(error) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes("CDP socket closed") ||
    message.includes("WebSocket") ||
    message.includes("ECONNRESET") ||
    message.includes("fetch failed") ||
    message.includes("CDP list failed") ||
    message.includes("No page target found")
  );
}

/** Run an operation and retry once when the connection fails. */
export async function withRetry(port, operation) {
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

export async function jsEval(client, expression) {
  const { result, exceptionDetails } = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (exceptionDetails) {
    throw new Error(
      exceptionDetails.exception?.description ?? exceptionDetails.text
    );
  }
  return result.value;
}

export function waitForLoad(client, timeoutMs = 8000) {
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

export function waitForNavigation(client, timeoutMs = 8000) {
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

export async function getBoundingRect(client, selector) {
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

/**
 * Attempt a CDP Input command and report whether the server accepted it.
 * formal-web may stub or reject Input methods, in which case callers fall
 * back to a JS-based implementation.
 */
export async function tryCdpInput(client, method, params) {
  try {
    await client.send(method, params);
    return true;
  } catch {
    return false;
  }
}

export async function jsSetInputValue(client, selector, text) {
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

export async function jsHoverElement(client, selector) {
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

export async function jsUnhoverElement(client, selector) {
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
