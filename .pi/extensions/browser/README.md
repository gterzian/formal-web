# pi browser extension

Provides `browser_navigate`, `browser_click`, `browser_evaluate`,
`browser_get_text`, `browser_screenshot`, and the other browser tools
available to the agent. Connects to a CDP-compatible browser.

## Usage

```bash
# Start formal-web's CDP server
embedder/target/debug/formal-web-embedder cdp --port 9222 \
  --startup-url "file:///path/to/page.html"

# Inside pi, connect the extension to the running server
# (this currently doesn't work reliably with formal-web's CDP)
/pi command: /browser-connect 9222
```

## Current state

The extension connects to a CDP endpoint and wraps its commands into
agent-callable tools. It works with standard Chrome/Chromium instances.
Connecting to formal-web's CDP server is the goal but not yet functional —
the WebSocket connection opens but the tools fail with closed-socket errors.

## Plan to fix

1. Determine why the WebSocket connection closes after `/browser-connect`.
   The `resolvePageWsUrl` function fetches `/json/list` and connects to the
   first page target's `webSocketDebuggerUrl`. If formal-web's CDP closes
   the socket after `Page.enable` / `Runtime.enable` / `DOM.enable` / `Log.enable`,
   one of those domain-enable commands may be unsupported.

2. Gracefully handle unsupported CDP domains: send a reduced set of enable
   commands (just `Page.enable` and `Runtime.enable`) and skip `DOM.enable`
   and `Log.enable` if they fail.

3. Add a `/browser-status` command that reports the connection state and
   the list of available targets.
