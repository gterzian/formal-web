# formal-web CDP support

formal-web's CDP server implements a subset of the protocol. Start it with:

```bash
formal-web-embedder cdp --port 9222 --startup-url "file:///path/to/page.html"
```

## Method support

- **Supported:** `Page.enable`, `Page.navigate`, `Page.reload`,
  `Page.captureScreenshot`, `Page.getFrameTree`, `Page.createIsolatedWorld`,
  `Runtime.enable`, `Runtime.evaluate`, `Input.dispatchMouseEvent` (only
  `mouseReleased` triggers click), `DOM.getDocument`, `DOM.querySelector`,
  `DOM.querySelectorAll`, `DOM.performSearch`, `DOM.getSearchResults`,
  `DOM.describeNode`, `Target.*`, `Browser.getVersion`, `Accessibility.*`.
- **No-op stubs (return `{}`):** `Input.dispatchKeyEvent`, `DOM.enable`,
  `Log.enable`; unknown methods return `{}`.
- **Not supported (return an error):** `Runtime.callFunctionOn`,
  `DOM.getBoxModel`, `DOM.getContentQuads`.

## Known limitations

1. **New webviews are not CDP targets.** When `window.open()` or a `_blank`
   hyperlink creates a new top-level traversable, the new webview is created
   and its content process starts, but the CDP server does not register it as
   a debuggable target. Only the initial page target appears in `GET /json`.
   The new webview renders and navigates but cannot be attached to over CDP.

2. **No DOM `click` synthesis from pointer events.** `automation_click` sends
   `PointerMove` → `PointerDown` → `PointerUp` to the content process, but no
   DOM `click` event is generated from those. `browser.mjs click` therefore
   tries `el.click()` first and only falls back to a physical click. Use
   `eval` with `location.assign()` for link navigation when a click does not
   take effect.

3. **No `Runtime.consoleAPICalled` events.** The server emits navigation
   events (`Page.frameNavigated`, `Page.loadEventFired`) but not console
   events, so `console --ms <n>` returns an empty list.

4. **No `history` global.** `history.back()` is not available in the JS
   environment, so `back` fails. Navigate explicitly instead.

5. **Render latency after navigation.** `Page.loadEventFired` fires before the
   first paint completes. Take a screenshot after a short delay when checking
   rendered output.

6. **`innerText` is not implemented.** `document.body.innerText` is
   `undefined`, so `browser.mjs text` returns an empty string. Use `eval` with
   `textContent` to read element text.

7. **Viewport metrics are not exposed.** `window.innerWidth` and
   `window.innerHeight` are `undefined`, so `browser.mjs scroll` cannot default
   to the viewport centre and uses `(0, 0)`. Pass explicit `x` and `y`
   coordinates to target the page.
