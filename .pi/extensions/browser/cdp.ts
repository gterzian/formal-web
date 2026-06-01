import WebSocket from "ws";

export class CDPClient {
  private ws: WebSocket | null = null;
  private wsUrl: string;
  private pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void }>();
  private eventHandlers = new Map<string, Array<(params: any) => void>>();
  private msgId = 0;
  private _ready: Promise<void>;
  private _resolveReady: (() => void) | null = null;
  private _rejectReady: ((error: Error) => void) | null = null;
  private _closed = false;

  constructor(wsUrl: string) {
    this.wsUrl = wsUrl;
    this._ready = new Promise((resolve, reject) => {
      this._resolveReady = resolve;
      this._rejectReady = reject;
    });
    this.connect();
  }

  private connect() {
    this._closed = false;
    this.ws = new WebSocket(this.wsUrl);
    this.ws.on("open", () => {
      if (this._resolveReady) {
        this._resolveReady();
        this._resolveReady = null;
        this._rejectReady = null;
      }
    });
    this.ws.on("error", (error) => {
      if (this._rejectReady) {
        this._rejectReady(error);
        this._resolveReady = null;
        this._rejectReady = null;
      }
    });
    this.ws.on("message", (raw) => {
      try {
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
      } catch {
        // Ignore malformed messages
      }
    });
    this.ws.on("close", () => {
      this._closed = true;
      const error = new Error("CDP socket closed");
      for (const { reject } of this.pending.values()) {
        reject(error);
      }
      this.pending.clear();
    });
  }

  get ready(): Promise<void> {
    return this._ready;
  }

  get closed(): boolean {
    return this._closed;
  }

  async send<T = any>(method: string, params: object = {}): Promise<T> {
    if (this._closed || !this.ws) {
      throw new Error("CDP socket closed");
    }
    return new Promise((resolve, reject) => {
      const id = ++this.msgId;
      this.pending.set(id, { resolve, reject });
      try {
        this.ws!.send(JSON.stringify({ id, method, params }), (error) => {
          if (!error) {
            return;
          }
          this.pending.delete(id);
          reject(error);
        });
      } catch (error: any) {
        this.pending.delete(id);
        reject(error);
      }
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
    this._closed = true;
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }
}

let _client: CDPClient | null = null;
let _port = 9222;

/** Track the number of consecutive failures to detect truly dead connections. */
let _consecutiveFailures = 0;
const MAX_CONSECUTIVE_FAILURES = 3;

export function setPort(port: number) {
  _port = port;
}

export function disconnect() {
  _client?.close();
  _client = null;
  _consecutiveFailures = 0;
}

export async function getClient(reconnect = false): Promise<CDPClient> {
  // If reconnection is forced or too many failures, clear the cached client.
  if (reconnect || _consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
    _client?.close();
    _client = null;
    _consecutiveFailures = 0;
  }

  if (_client && !_client.closed) {
    return _client;
  }

  // If cached client was closed, create a new one.
  if (_client?.closed) {
    _client = null;
    _consecutiveFailures = 0;
  }

  const wsUrl = await resolvePageWsUrl(_port);
  const client = new CDPClient(wsUrl);
  await client.ready;

  // Enable required domains. DOM.enable and Log.enable are optional;
  // formal-web's CDP server may not support them but it handles unknown
  // methods gracefully (returns {} without error). Still, we catch
  // any unexpected errors so the connection can proceed.
  await Promise.all([
    client.send("Page.enable"),
    client.send("Runtime.enable"),
  ]);
  await Promise.all([
    client.send("DOM.enable").catch(() => {}),
    client.send("Log.enable").catch(() => {}),
  ]);

  _client = client;
  _consecutiveFailures = 0;
  return client;
}

/**
 * Mark a failure on the current connection.
 * Returns true if the caller should reconnect before retrying.
 */
export function noteSendFailure(): boolean {
  _consecutiveFailures++;
  return _consecutiveFailures >= MAX_CONSECUTIVE_FAILURES;
}

/**
 * Check connection status and available targets.
 */
export async function browserStatus(): Promise<{
  connected: boolean;
  port: number;
  targets: Array<{ type: string; url: string; webSocketDebuggerUrl?: string }>;
}> {
  const targets: Array<{ type: string; url: string; webSocketDebuggerUrl?: string }> = [];
  let connected = false;

  try {
    const response = await fetch(`http://localhost:${_port}/json/list`);
    if (response.ok) {
      const list = await response.json() as Array<{ type: string; url: string; webSocketDebuggerUrl?: string }>;
      targets.push(...list);
    }
  } catch {
    // Can't fetch target list
  }

  if (_client && !_client.closed) {
    connected = true;
  }

  return { connected, port: _port, targets };
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

/**
 * Fallback for tools that need CDP Input domain commands that formal-web
 * may not support (dispatchKeyEvent, dispatchMouseEvent for hover).
 * Attempts the CDP command; if it throws due to an unsupported domain,
 * returns false so the caller can use a JS-based fallback.
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

/**
 * Set an input field value via JS evaluation.
 */
export async function jsSetInputValue(
  client: CDPClient,
  selector: string,
  text: string
): Promise<void> {
  const nativeInputValueSetter = await jsEval<boolean>(
    client,
    `(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return false;
      // Set the value and dispatch input/change events to trigger reactivity.
      const nativeSetter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype, 'value'
      )?.set;
      if (nativeSetter) {
        nativeSetter.call(el, ${JSON.stringify(text)});
      } else {
        el.value = ${JSON.stringify(text)};
      }
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    })()`
  );
  if (!nativeInputValueSetter) {
    throw new Error(`Element not found: ${selector}`);
  }
}

/**
 * Simulate a mouse hover via JS by dispatching mouseenter/mouseover events.
 */
export async function jsHoverElement(
  client: CDPClient,
  selector: string
): Promise<void> {
  const hovered = await jsEval<boolean>(
    client,
    `(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return false;
      el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true, relatedTarget: null }));
      el.dispatchEvent(new MouseEvent('mouseenter', { bubbles: false, relatedTarget: null }));
      return true;
    })()`
  );
  if (!hovered) {
    throw new Error(`Element not found: ${selector}`);
  }
}

/**
 * Unhover (move mouse away) via JS.
 */
export async function jsUnhoverElement(
  client: CDPClient,
  selector: string
): Promise<void> {
  await jsEval(
    client,
    `(() => {
      const el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return;
      el.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: null }));
      el.dispatchEvent(new MouseEvent('mouseleave', { bubbles: false, relatedTarget: null }));
    })()`
  );
}
