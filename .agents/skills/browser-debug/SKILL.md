---
name: browser-debug
description: Drive a running formal-web or Chrome instance over the Chrome DevTools Protocol: navigate, evaluate JavaScript, click, type, hover, read text, attributes and computed styles, take screenshots and capture console output. Use for live interactive debugging while developing a feature and for reproducing failing WPT behavior on a minimal page.
compatibility: Requires Node.js 22 or newer and a CDP endpoint, by default port 9222, for example `formal-web-embedder cdp --port 9222`.
---

# browser-debug

Use this skill for the iterative "navigate → inspect → mutate → screenshot"
loop during feature work, and to reproduce a failing WPT case on a minimal
page. WebDriver is reserved for automated regression suites.

Start a CDP endpoint first:

```bash
formal-web-embedder cdp --port 9222 --startup-url "file:///path/to/page.html"
```

Each invocation opens a WebSocket, runs one command, and closes. Page state
lives in the browser and persists across invocations. The endpoint is `--port`
or `FORMAL_WEB_CDP_PORT`, default 9222. A failed connection is retried once.

## Commands

Run from this skill directory (`scripts/browser.mjs`), or with the full path
from the repository root.

```
node scripts/browser.mjs navigate <url>
node scripts/browser.mjs reload [--ignore-cache]
node scripts/browser.mjs back
node scripts/browser.mjs eval <expression>
node scripts/browser.mjs click <selector>
node scripts/browser.mjs type <selector> <text> [--clear-first]
node scripts/browser.mjs hover [selector]
node scripts/browser.mjs text [selector] [--max <n>]
node scripts/browser.mjs attr <selector> <name>
node scripts/browser.mjs style <selector> <property>
node scripts/browser.mjs screenshot [--out <path>]
node scripts/browser.mjs console --ms <n>
node scripts/browser.mjs scroll <deltaY> [x] [y]
node scripts/browser.mjs status
```

- `click` tries `el.click()` first and falls back to a CDP physical click.
- `type` sets the input value through the native setter and dispatches
  `input`/`change` so reactive frameworks see it.
- `hover` tries CDP `mouseMoved`, verifies `:hover`, and falls back to
  dispatching mouse events. With no selector it moves the mouse away.
- `text` returns up to 4000 characters; `--max` overrides the cap.
- Structured results print as JSON. `text`, `attr` and `style` print the raw
  value.
- `screenshot` writes a PNG to `--out` (default a timestamped file under the
  system temp directory) and prints the path.

## formal-web CDP support

`references/cdp-support.md` lists which CDP methods formal-web supports and the
known limitations: new webviews missing from the target list, DOM `click`
synthesis from pointer events, `Runtime.consoleAPICalled` events, the `history`
global, and render latency after `Page.loadEventFired`. Read it before relying
on a command whose result looks empty.
