# automation crate

The automation crate implements the two wire-protocol servers that external
tools use to drive formal-web: WebDriver (W3C standard) and CDP (Chrome
DevTools Protocol).

## CDP Server

The CDP server lets you connect to a running formal-web instance and issue
commands via WebSocket. Any CDP-compatible client (Chrome DevTools, Puppeteer,
Playwright, or a raw WebSocket) can connect.

### Starting

```bash
# From the workspace root, run the embedder directly:
embedder/target/debug/formal-web-embedder cdp --port 9222 \
  --startup-url "file:///path/to/page.html"

# Or via the workspace entrypoint:
cargo run --bin formal-web -- cdp --headless --port 9222 \
  --startup-url "file:///path/to/page.html"
```

### Connecting

Enumerate targets:
```bash
curl http://127.0.0.1:9222/json/list
```

Each target has a `webSocketDebuggerUrl` field. Connect to it with any
WebSocket client and send CDP commands as JSON messages:

```javascript
// Node.js — WebSocket is built-in
const ws = new WebSocket('ws://localhost:9222/devtools/page/<id>');

function send(id, method, params) {
  return new Promise((resolve) => {
    const handler = (event) => {
      const m = JSON.parse(event.data);
      if (m.id === id) {
        ws.removeEventListener('message', handler);
        resolve(m);
      }
    };
    ws.addEventListener('message', handler);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

ws.addEventListener('open', async () => {
  // Read the page title
  const resp = await send(1, 'Runtime.evaluate',
    { expression: 'document.title' });
  console.log(resp.result?.result?.value);

  // Navigate
  await send(2, 'Runtime.evaluate',
    { expression: 'location.href = "other.html"' });
});
```

The pi agent's browser extension (`.pi/extensions/browser/`) wraps this
into the `browser_navigate`, `browser_click`, etc. tools. Before using
those tools, run `/browser-connect [port]` to connect the extension to
formal-web's CDP endpoint.

## WebDriver Server

The WebDriver server implements the W3C WebDriver spec for automated
testing. It is used by `./verification/verify-navigation.sh` for
navigation smoke tests and by the WPT runner.

```bash
# Start with WebDriver:
embedder/target/debug/formal-web-embedder webdriver --port 4451 \
  --startup-url "file:///path/to/page.html"

# Create a session:
curl -X POST http://127.0.0.1:4451/session -H 'Content-Type: application/json' -d '{}'
```

## Relevant source layout

- `src/cdp.rs` — CDP server implementation
- `src/webdriver.rs` — WebDriver server implementation
- `src/lib.rs` — shared types and helpers
