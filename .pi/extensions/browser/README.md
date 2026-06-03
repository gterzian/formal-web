# pi browser extension

Provides `browser_navigate`, `browser_click`, `browser_evaluate`,
`browser_get_text`, `browser_screenshot`, and other browser tools
available to the agent. Connects to a CDP-compatible browser.

Works with standard Chrome/Chromium instances **and** formal-web's CDP server.

## Usage

```bash
# Start formal-web's CDP server
embedder/target/debug/formal-web-embedder cdp --port 9222 \
  --startup-url "file:///path/to/page.html"

# Inside pi, connect the extension to the running server
/pi command: /browser-connect 9222
```

## Tools

All tools auto-reconnect on socket failures and fall back to JS-based
implementations for CDP domains formal-web does not support.

| Tool | Primary mechanism | Works in formal-web? |
|---|---|---|
| `browser_navigate` | `Page.navigate` | ✅ Full support |
| `browser_reload` | `Page.reload` | ✅ Full support |
| `browser_evaluate` | `Runtime.evaluate` | ✅ Full support |
| `browser_click` | `el.click()` via JS eval, then CDP `Input.dispatchMouseEvent` | ⚠️ `el.click()` throws (not in DOM); CDP physical click sends PointerUp/PointerDown events but DOM `click` synthesis is not yet wired. Agent can fall back to `browser_evaluate` with `location.assign()` etc. |
| `browser_type` | JS input value setter (`jsSetInputValue`) + `input`/`change` events | ✅ `jsSetInputValue` fallback works. CDP `Input.dispatchKeyEvent` is a no-op — the tool skips it. |
| `browser_hover` | CDP `mouseMoved` then verify with `:hover` check | ✅ Falls back to `jsHoverElement` (dispatches `mouseover`/`mouseenter` via DOM) |
| `browser_get_text` | `Runtime.evaluate` reading `innerText` | ⚠️ `body.innerText` may be `null`; use `browser_evaluate` with `textContent` as needed |
| `browser_get_attribute` | `Runtime.evaluate` | ✅ Full support |
| `browser_get_computed_style` | `Runtime.evaluate` | ✅ Full support |
| `browser_screenshot` | `Page.captureScreenshot` | ✅ Full support |
| `browser_capture_console` | Listens for `Runtime.consoleAPICalled` events | ❌ Not emitted by CDP server yet — always returns empty. |
| `browser_history_back` | `history.back()` via JS eval | ❌ `history` is not defined in the JS environment yet. Use `browser_evaluate` with `location.assign()` as fallback. |

## Commands

| Command | Description |
|---|---|
| `/browser-connect [port]` | Connect to a CDP endpoint (default port 9222) |
| `/browser-disconnect` | Disconnect from the CDP endpoint |
| `/browser-status` | Show connection state and available targets |
| `/test-page` | Queue the FormalWeb startup page test suite |

## Architecture

- **`cdp.ts`** — WebSocket CDP client, connection management, reconnection logic,
  JS-based fallbacks for unsupported CDP domains (`jsSetInputValue`,
  `jsHoverElement`, `jsUnhoverElement`).
- **`tools.ts`** — Tool registration with auto-reconnect wrapper.
- **`index.ts`** — Extension entry point and slash commands.
- **`tests/formalweb.ts`** — FormalWeb startup page test plan (run via `/test-page`).

### Formal-web CDP specifics

Formal-web's CDP server implements a subset of the CDP protocol:

- **Supported:** `Page.enable`, `Page.navigate`, `Page.reload`, `Page.captureScreenshot`,
  `Page.getFrameTree`, `Page.createIsolatedWorld`, `Runtime.enable`, `Runtime.evaluate`,
  `Input.dispatchMouseEvent` (only `mouseReleased` triggers click),
  `DOM.getDocument`, `DOM.querySelector`, `DOM.querySelectorAll`,
  `DOM.performSearch`, `DOM.getSearchResults`, `DOM.describeNode`,
  `Target.*`, `Browser.getVersion`, `Accessibility.*`
- **No-op stubs (return {}):** `Input.dispatchKeyEvent`,
  `DOM.enable`, `Log.enable` (unknown methods return `{}`)
- **Not supported (return error):** `Runtime.callFunctionOn`,
  `DOM.getBoxModel`, `DOM.getContentQuads`

The JS fallback in `browser_type` sets input values directly via `Runtime.evaluate`
and dispatches `input`/`change` events to trigger reactive frameworks.

## Future direction

The CDP pi tools are meant for **live interactive debugging** of formal-web
during feature development. WebDriver is reserved for automated regression
suites (WPT, verification smoke tests).

### Known limitations (formal-web CDP server)

1. **New webviews not listed as CDP targets** — When `window.open()` or a
   `_blank` hyperlink creates a new top-level traversable, the new webview
   is created and its content process starts, but the CDP server does not
   register it as a debuggable target. Only the initial page target appears
   in the `GET /json` target list. This makes it impossible to attach to
   popup windows or new tabs via CDP. The new webview is still functional
   (it renders, navigates, etc.) but is invisible to devtools-style inspection.

### Near-term improvements (CDP server side)

1. **Synthesize DOM `click` from pointer events** —
   `automation_click` sends `PointerMove` → `PointerDown` → `PointerUp` to the
   content process, but no DOM `click` event is generated from those yet. Wiring
   this into the content event pipeline would make `browser_click` work for
   interactive elements without needing a JS fallback.

2. **Emit `Runtime.consoleAPICalled` events** — The CDP server emits navigation
   events (`Page.frameNavigated`, `Page.loadEventFired`) but not console events.
   Adding a bridge from the content process's console to the CDP event sink would
   let `browser_capture_console` capture `console.log`/`warn`/`error` output.

3. **Expose `history` global** — Session history (`history.back()`/`forward()`) is
   not yet wired into the JS environment. Once the traversable's session history
   is exposed, `browser_history_back` will work.

4. **Reduce render latency after navigation** — `Page.loadEventFired` fires before
   the first paint completes. Adding a small render-wait or exposing the paint
   state separately would make `browser_navigate` return after the page is visibly
   ready rather than just loaded.

### Future tool additions

- **`browser_scroll`** — The CDP server already handles `Input.dispatchMouseEvent`
  for clicks; adding scroll delta handling would let the agent check
  scroll-triggered rendering.
- **`browser_set_viewport`** — Resize the viewport programmatically for
  responsive-design debugging.
- **`browser_trace`** — Start/stop a performance trace (`Tracing.start`).

### Relationship to WebDriver

| | CDP (pi tools) | WebDriver (WPT) |
|---|---|---|
| **Purpose** | Interactive debugging | Automated regression testing |
| **User** | The pi agent (you) | WPT runner, verification scripts |
| **Connection** | Persistent WebSocket | Request-response HTTP |
| **State** | Live page state | Session-per-test |
| **JS eval** | Direct `Runtime.evaluate` | Wrapped in execute-script commands |

Both are valuable — WebDriver for running the full WPT suite, CDP for the
iterative "navigate → inspect → mutate → screenshot" loop during feature work.
