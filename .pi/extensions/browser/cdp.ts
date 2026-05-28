import WebSocket from "ws";

export class CDPClient {
  private ws: WebSocket;
  private pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void }>();
  private eventHandlers = new Map<string, Array<(params: any) => void>>();
  private msgId = 0;
  readonly ready: Promise<void>;

  constructor(wsUrl: string) {
    this.ws = new WebSocket(wsUrl);
    this.ready = new Promise((resolve, reject) => {
      this.ws.on("open", () => resolve());
      this.ws.on("error", (error) => reject(error));
    });
    this.ws.on("message", (raw) => {
      const msg = JSON.parse(raw.toString());
      if (msg.id != null && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id)!;
        this.pending.delete(msg.id);
        msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result ?? {});
        return;
      }
      if (msg.method) {
        for (const handler of this.eventHandlers.get(msg.method) ?? []) {
          handler(msg.params);
        }
      }
    });
    this.ws.on("close", () => {
      const error = new Error("CDP socket closed");
      for (const { reject } of this.pending.values()) {
        reject(error);
      }
      this.pending.clear();
    });
  }

  send<T = any>(method: string, params: object = {}): Promise<T> {
    return new Promise((resolve, reject) => {
      const id = ++this.msgId;
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }), (error) => {
        if (!error) {
          return;
        }
        this.pending.delete(id);
        reject(error);
      });
    });
  }

  onEvent(method: string, handler: (params: any) => void): () => void {
    if (!this.eventHandlers.has(method)) {
      this.eventHandlers.set(method, []);
    }
    this.eventHandlers.get(method)!.push(handler);
    return () => {
      const handlers = this.eventHandlers.get(method);
      if (!handlers) {
        return;
      }
      const index = handlers.indexOf(handler);
      if (index !== -1) {
        handlers.splice(index, 1);
      }
    };
  }

  close() {
    this.ws.close();
  }
}

let _client: CDPClient | null = null;
let _port = 9222;

export function setPort(port: number) {
  _port = port;
}

export function disconnect() {
  _client?.close();
  _client = null;
}

export async function getClient(): Promise<CDPClient> {
  if (_client) {
    return _client;
  }
  const wsUrl = await resolvePageWsUrl(_port);
  _client = new CDPClient(wsUrl);
  await _client.ready;
  await Promise.all([
    _client.send("Page.enable"),
    _client.send("Runtime.enable"),
    _client.send("DOM.enable"),
    _client.send("Log.enable"),
  ]);
  return _client;
}

async function resolvePageWsUrl(port: number): Promise<string> {
  const response = await fetch(`http://localhost:${port}/json/list`);
  if (!response.ok) {
    throw new Error(`CDP list failed: ${response.status}`);
  }
  const targets = (await response.json()) as Array<{ type: string; webSocketDebuggerUrl?: string }>;
  const page = targets.find((target) => target.type === "page" && target.webSocketDebuggerUrl);
  if (!page?.webSocketDebuggerUrl) {
    throw new Error(`No page target found on port ${port}`);
  }
  return page.webSocketDebuggerUrl;
}

export async function jsEval<T = any>(client: CDPClient, expression: string): Promise<T> {
  const { result, exceptionDetails } = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (exceptionDetails) {
    throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  }
  return result.value as T;
}

export function waitForLoad(client: CDPClient, timeoutMs = 8000): Promise<void> {
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

export function waitForNavigation(client: CDPClient, timeoutMs = 8000): Promise<void> {
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
  const rect = await jsEval<{ left: number; top: number; width: number; height: number } | null>(
    client,
    `(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height };
    })()`
  );
  if (!rect) {
    throw new Error(`Element not found: ${selector}`);
  }
  return rect;
}
