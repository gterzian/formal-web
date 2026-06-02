# embedder crate

The embedder crate owns the top-level application lifecycle, window management,
browser chrome, and the redraw loop. It delegates to content and net
processes through the `webview` and `user_agent` crates.

## Architecture

### Two app implementations

- **`WindowedApp`** (`event_loop/windowed/mod.rs`): headed (GUI) application with
  native windows, a Blitz-rendered browser chrome, and multi-window/multi-tab
  support. Runs via winit's event loop.

- **`HeadlessEmbedderApp`** (`event_loop/headless.rs`): headless application for
  automation-only hosting (WebDriver, CDP). No window, no chrome, just a fixed
  viewport and event-loop plumbing.

### Multi-window and multi-tab

`WindowedApp` owns a `HashMap<WindowId, WindowState>` where each `WindowState`
represents one native window (one winit `Window` + one `VelloWindowRenderer`).

Each window has:

- A `ChromeUi` instance — a Blitz-based HTML/CSS chrome with an address bar
  and a tab strip.
- A `HashMap<WebviewId, TabState>` of open tabs, ordered by a `Vec<WebviewId>`
  (`tab_order`).
- One `active_tab` (`Option<WebviewId>`) — the currently displayed tab.
- An `AutomationController` for WebDriver/CDP integration.
- Per-window input state (pointer position, keyboard modifiers, mouse buttons).

A `webview_to_window` mapping routes `WebviewId`-scoped events
(`NavigationRequested`, `NavigationCompleted`, `NewWebview`, `RequestRedraw`)
to the correct window.

### Tab lifecycle

1. A tab is created when the user agent dispatches a `NewWebview` event
   (triggered by `provider.navigate(None, url)` or by the user clicking the
   `+` button in the chrome).
2. The `NewWebview` handler calls `add_tab()` which inserts a `TabState` into
   the window's tab map and pushes the webview ID onto `tab_order`.
3. Navigation state is tracked per-tab via `pending_url` and `committed_url`.
4. The chrome tab strip is rebuilt whenever tab count changes (the
   `ChromeUi` re-generates its HTML template with ordered tab buttons).

### Viewport management

Each window computes its content viewport as
`(window_width, window_height - chrome_height, scale, color_scheme)` and
propagates it to the provider via `set_default_viewport` (for new traversables)
and `set_traversable_viewport` (for the active tab's traversable).

Viewport updates happen on:
- Window creation (`resumed`)
- Tab creation (`NewWebview`)
- Tab switch (`SwitchTab`)
- Navigation progression (`NavigationRequested`, `NavigationCompleted`)
- Window resize (`Resized`)

### Chrome

The chrome is rendered as a Blitz HTML document with CSS styling. It contains:
- An address bar (`<input id="address">`) — shows the active tab's current URL.
- A tab strip with tab buttons (`<button id="tab-N">`) — one per open tab.
- A `+` button (`<div id="new-tab-btn">`) — opens a new tab; shift+click
  opens a new window.

When tab state changes, the entire chrome HTML is regenerated with the correct
number of tab buttons (each with a unique DOM id like `tab-0`, `tab-1`, etc.).
Hit-testing uses the `id` attribute from the DOM (not node IDs) to avoid stale
references after HTML rebuilds.

## Current implementation status

- [x] Multi-window support (one winit event loop, many windows)
- [x] Multi-tab support per window (webview-backed tabs)
- [x] Chrome: address bar with URL display
- [x] Chrome: tab strip with click-to-switch
- [x] Chrome: `+` button for new tab / shift+click for new window
- [x] Tab labels show page URL (truncated) or "New Tab" for blank pages
- [x] Viewport tracking and propagation to provider
- [x] Automation (WebDriver/CDP) targets the active tab in the active window
- [x] `about:blank` navigation fails (pre-existing content-process issue)
- [ ] Address-bar Enter opens new tab instead of navigating (under investigation)
- [ ] Tab close button
- [ ] Tab reordering

## Possible future work

- **Tab close button**: Add an `×` button to each tab for closing. Requires
  a `ChromeAction::CloseTab(usize)` action and cleanup of the tab state,
  compositor, and webview-to-window mapping.
- **Tab reordering**: Make tabs draggable to reorder. Requires drag-and-drop
  in the chrome HTML and updating `tab_order` accordingly.
- **Tab drag-out to new window**: Dragging a tab out of its window creates a
  new window with that tab. Requires moving a `TabState` between windows.
- **URL bar spellcheck/suggestions**: Autocomplete or search-engine integration
  in the address bar.
- **Window title update**: Sync the winit window title with the active tab's
  page title (requires plumbing page title through the user agent).
- **CDP multi-target support**: Expose each tab/window as a separate CDP target
  (`Target.getTargets`, `Target.attachToTarget`) so automation tools can
  interact with specific pages.
- **About:blank fix**: The content process currently fails to handle
  `about:blank` navigation ("builder error"). Fixing this would allow the CDP
  server to start with a blank page instead of requiring a real URL.
- **Browser history integration**: Remove the per-tab `committed_url` /
  `pending_url` tracking in favour of the user agent's session history once
  that's implemented.
- **Performance**: The chrome HTML is fully rebuilt whenever tab count changes.
  For many tabs this could be slow. A virtual-scrolling tab strip or
  incremental DOM updates would scale better.
- **Headless/headed sharing**: Some input-event dispatch helpers are duplicated
  between `WindowedApp` and `HeadlessEmbedderApp`. These could be extracted
  into shared utility functions.

## Key files

| File | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point |
| `src/event_loop.rs` | Event loop orchestration, shared types |
| `src/event_loop/winit.rs` | Winit integration (shell provider, key/mouse mapping) |
| `src/event_loop/windowed/mod.rs` | `WindowedApp` + `WindowState` |
| `src/event_loop/windowed/chrome.rs` | `ChromeUi` — Blitz-based browser chrome |
| `src/event_loop/headless.rs` | `HeadlessEmbedderApp` |
| `src/ui_event.rs` | UI event serialization |
