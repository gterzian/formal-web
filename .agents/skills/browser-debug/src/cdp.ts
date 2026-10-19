export interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
}

export type EventHandler = (params: any) => void;

export interface CDPTarget {
  type: string;
  url: string;
  webSocketDebuggerUrl?: string;
}

export class CDPClient {
  private socket: WebSocket | null = null;
  private pending = new Map<number, PendingRequest>();
  private eventHandlers = new Map<string, EventHandler[]>();
  private messageId = 0;
  private readyPromise: Promise<void>;
  private resolveReady: (() => void) | null = null;
  private rejectReady: ((error: Error) => void) | null = null;
  private isClosed = false;

  constructor(wsUrl: string) {
    this.readyPromise = new Promise((resolve, reject) => {
      this.resolveReady = resolve;
      this.rejectReady = reject;
    });
    this.connect(wsUrl);
  }

  private connect(wsUrl: string): void {
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
      let message: any;
      try {
        message = JSON.parse(raw);
      } catch {
        return;
      }
      if (message.id != null && this.pending.has(message.id)) {
        const pending = this.pending.get(message.id)!;
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

  get ready(): Promise<void> {
    return this.readyPromise;
  }

  get closed(): boolean {
    return this.isClosed;
  }

  async send<T = any>(method: string, params: object = {}): Promise<T> {
    if (this.isClosed || !this.socket) {
      throw new Error("CDP socket closed");
    }
    return new Promise<T>((resolve, reject) => {
      const id = ++this.messageId;
      this.pending.set(id, {
        resolve: (value) => resolve(value as T),
        reject,
      });
      try {
        this.socket!.send(JSON.stringify({ id, method, params }));
      } catch (error) {
        this.pending.delete(id);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  onEvent(method: string, handler: EventHandler): () => void {
    if (!this.eventHandlers.has(method)) {
      this.eventHandlers.set(method, []);
    }
    this.eventHandlers.get(method)!.push(handler);
    return () => {
      const handlers = this.eventHandlers.get(method);
      if (!handlers) return;
      const index = handlers.indexOf(handler);
      if (index !== -1) {
        handlers.splice(index, 1);
      }
    };
  }

  close(): void {
    this.isClosed = true;
    if (this.socket) {
      this.socket.close();
      this.socket = null;
    }
  }
}

export function resolvePort(explicitPort?: number): number {
  if (explicitPort !== undefined) return explicitPort;
  const fromEnvironment = process.env.FORMAL_WEB_CDP_PORT;
  if (fromEnvironment) {
    const parsed = Number.parseInt(fromEnvironment, 10);
    if (Number.isInteger(parsed)) return parsed;
  }
  return 9222;
}

export async function listTargets(port: number): Promise<CDPTarget[]> {
  const response = await fetch(`http://localhost:${port}/json/list`);
  if (!response.ok) {
    throw new Error(`CDP list failed: ${response.status}`);
  }
  return (await response.json()) as CDPTarget[];
}

async function resolvePageWsUrl(port: number): Promise<string> {
  const targets = await listTargets(port);
  const page = targets.find(
    (target) => target.type === "page" && target.webSocketDebuggerUrl
  );
  if (!page?.webSocketDebuggerUrl) {
    throw new Error(`No page target found on port ${port}`);
  }
  return page.webSocketDebuggerUrl;
}

export async function connect(port: number): Promise<CDPClient> {
  const wsUrl = await resolvePageWsUrl(port);
  const client = new CDPClient(wsUrl);
  await client.ready;

  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("DOM.enable").catch(() => undefined);
  await client.send("Log.enable").catch(() => undefined);

  return client;
}

function isConnectionError(error: unknown): boolean {
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
export async function withRetry<T>(
  port: number,
  operation: (client: CDPClient) => Promise<T>
): Promise<T> {
  const attempt = async (): Promise<T> => {
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

export async function jsEval<T = any>(
  client: CDPClient,
  expression: string
): Promise<T> {
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
  return result.value as T;
}

export function waitForLoad(
  client: CDPClient,
  timeoutMs = 8000
): Promise<void> {
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

export function waitForNavigation(
  client: CDPClient,
  timeoutMs = 8000
): Promise<void> {
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

export async function getBoundingRect(
  client: CDPClient,
  selector: string
): Promise<{ left: number; top: number; width: number; height: number }> {
  const rect = await jsEval<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(
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
export async function tryCdpInput(
  client: CDPClient,
  method: string,
  params: object
): Promise<boolean> {
  try {
    await client.send(method, params);
    return true;
  } catch {
    return false;
  }
}

export async function jsSetInputValue(
  client: CDPClient,
  selector: string,
  text: string
): Promise<void> {
  const applied = await jsEval<boolean>(
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

export async function jsHoverElement(
  client: CDPClient,
  selector: string
): Promise<void> {
  const hovered = await jsEval<boolean>(
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

export async function jsUnhoverElement(
  client: CDPClient,
  selector: string
): Promise<void> {
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
