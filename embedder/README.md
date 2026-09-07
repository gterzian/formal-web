# embedder

The embedder layer owns the top-level application lifecycle, window management,
browser chrome, and the redraw loop. It delegates to content and net
processes through the `webview` and `user_agent` crates.

The embedder crates never depend on the ipc crates (`ipc`, `ipc-channel`,
`ipc_messages`); the `webview` crate re-exports every browser-facing type and
interface they need.

## Crate layout

Three crates, sharing nothing but the `webview` crate API:

| Crate | Purpose |
|-------|---------|
| `embedder` (root) | Thin dispatcher: CLI entry points (`run_default`, `run_webdriver`, `run_cdp`) and the windowed-backend selection (AppKit on macOS by default, winit elsewhere or when `winit_embedder` is enabled). Builds the `formal-web-embedder` binary. |
| `mac-embedder` | Self-contained AppKit app (macOS only): `NSApplication` lifecycle, native chrome, `CVDisplayLink` pacing, zero-copy IOSurface presentation. No winit, Blitz, or GPU dependencies. |
| `winit-embedder` | Self-contained winit app: a windowed app with a Blitz-rendered chrome and a headless app for automation (WebDriver, CDP, WPT). The windowed app is gated behind the `windowed` feature (on by default); the headless app is always available and pulls no graphics dependencies. |

Each embedder owns its own user-event bus (`FormalWebUserEvent`, its own
`UserEventSink`, and its own `webview::Embedder` implementation) and its own
copy of the shared helpers (clipboard, screenshot encoding, startup URL
resolution, viewport snapshot). The two embedders are deliberately
independent so the AppKit app never builds winit/Blitz/GPU code.

## Windowed backend selection

The headed app is provided by one of two backends, selected at compile time
in the root `embedder` crate: on macOS the AppKit backend is the default and
the winit windowed backend is **not compiled** unless the `winit_embedder`
feature is enabled (`--features winit_embedder`); on other platforms the
winit windowed backend is the only option and the feature is a no-op.

**Automation always runs on the winit embedder — never the AppKit one.**
`run_cdp`/`run_webdriver` dispatch to winit unconditionally (headless or
headed), and `mac-embedder` has no automation entry points; the AppKit
app is only ever the headed default browser. Headless automation works on
any configuration; headed automation on macOS requires the `winit_embedder`
feature (without it the winit windowed app is not compiled and the command
fails with a clear error).

- **`mac-embedder`** (AppKit): the default on macOS. Native AppKit chrome
  (menu bar, `NSToolbar`, tab strip), zero-copy IOSurface presentation via
  the content layer's `contents`, and a `CVDisplayLink` pacing animated
  content via `WebviewProvider::frame_needed`.  Menu key equivalents run
  through `NSMenu::performKeyEquivalent` so unbound ⌘-combinations still
  reach the web content.  Mouse events over the titlebar/toolbar/tab-strip
  region pass through to AppKit; the web viewport is the window's
  `contentLayoutRect` minus the tab strip row.
- **`winit-embedder`**: winit windows with a Blitz-rendered chrome. The
  only option on non-macOS platforms; on macOS it is built and used only
  when the `winit_embedder` feature is enabled.  `WindowedApp`
  (`winit-embedder/src/windowed.rs`) owns a `HashMap<WindowId, WindowState>`
  of windows, each with a `ChromeUi` instance (address bar + tab strip) and
  a set of webview-backed tabs; `HeadlessEmbedderApp`
  (`winit-embedder/src/headless.rs`) is the headless app used by automation.
  Tabs are created from the user agent's `NewWebview` events and tracked
  per-tab with `pending_url`/`committed_url` until session history lands;
  viewport changes propagate to the provider on window/tab/navigation
  events.

### Multi-window and multi-tab

`WindowedApp` owns a `HashMap<WindowId, WindowState>`; a
`webview_to_window` mapping routes `WebviewId`-scoped events
(`NavigationRequested`, `NavigationCompleted`, `NewWebview`, `RequestRedraw`)
to the correct window.  When tab state changes, the chrome HTML is fully
regenerated with one tab button per open tab; hit-testing uses the `id`
attribute from the DOM (not node IDs) to avoid stale references after HTML
rebuilds.

## Known gaps in the AppKit backend relative to winit

- **IME is not implemented.** The AppKit backend sends `KeyDown`/`KeyUp` events
  only; text composition (CJK and other marked-text input) requires the
  `NSTextInputClient` protocol on the web content view, which is not yet wired.
  Basic ASCII text input into page fields works through `KeyDown` text.
- **Touch events are not handled** (desktop macOS has no touch input; winit's
  touch path is for trackpads/tablets).
- **JS-driven titles are not propagated.** The content process reports a
  top-level document's parsed `<title>` after parsing, but titles changed
  later via JS (`document.title = …` or DOM manipulation of the title
  element) are not sent, so a page that sets its title after load keeps the
  parsed title.
- **Session history is not implemented.** The History menu's Back/Forward items
  are disabled; Reload re-navigates to the tab's committed URL because the
  user agent has no reload command.
- **Closing a tab does not tear down its traversable.** The user agent has no
  webview-teardown path, so a closed tab's webview keeps living there (the
  same situation as closing a window).

## Current implementation status

- Multi-window support (one winit event loop, many windows); multi-tab
  support per window (webview-backed tabs); chrome with address bar, tab
  strip, and a `+` button (shift+click opens a new window); tab labels show
  the page URL (truncated) or "New Tab"; viewport tracking per window/tab;
  automation (WebDriver/CDP) targets the active tab in the active window.

Remaining work (roughly in priority order):

- **Address-bar Enter opens a new tab instead of navigating** (under
  investigation).
- **Tab close button** — needs a `ChromeAction::CloseTab(usize)` action and
  cleanup of the tab state, compositor, and webview-to-window mapping.
- **Tab reordering** — drag-and-drop in the chrome HTML plus `tab_order`
  updates; tab drag-out to a new window would follow.
- **Window title sync** — the content→UA title plumbing exists (parse-time
  titles); the winit window title is not yet set from it.
- **About:blank navigation of an existing tab** logs a content-process
  "unknown document id" error during navigation finalization, although the
  URL still ends up as `about:blank`.  New top-level traversables to
  `about:blank` (new tabs, new windows, CDP startup) work; only the
  existing-tab path is affected.
- **Browser history integration** — remove the per-tab `committed_url` /
  `pending_url` tracking in favour of the user agent's session history once
  that's implemented.
- **CDP multi-target support** — expose each tab/window as a separate CDP
  target (`Target.getTargets`, `Target.attachToTarget`).
- **Headless/headed sharing** — some input-event dispatch helpers are
  duplicated between `WindowedApp` and `HeadlessEmbedderApp` and could be
  extracted into shared utilities.
- **Performance** — the chrome HTML is fully rebuilt whenever tab count
  changes; a virtual-scrolling tab strip or incremental DOM updates would
  scale better for many tabs.

## Key files

| File | Purpose |
|------|---------|
| `embedder/src/main.rs` | `formal-web-embedder` CLI entry point |
| `embedder/src/lib.rs` | CLI entry points + windowed-backend selection |
| `mac-embedder/src/app.rs` | AppKit application, window/chrome/event routing |
| `mac-embedder/src/window.rs` | Layer-hosting view, IOSurface presentation |
| `mac-embedder/src/input.rs` | NSEvent → Blitz input mapping |
| `mac-embedder/src/events.rs` | AppKit user-event bus + `webview::Embedder` impl |
| `mac-embedder/src/platform.rs` | Clipboard, screenshot, startup/URL helpers |
| `winit-embedder/src/windowed.rs` | `WindowedApp` — winit window/chrome/events |
| `winit-embedder/src/headless.rs` | `HeadlessEmbedderApp` — automation-only winit app |
| `winit-embedder/src/chrome.rs` | `ChromeUi` — Blitz-based browser chrome |
| `winit-embedder/src/winit_integration.rs` | Winit integration (shell provider, key/mouse mapping) |
| `winit-embedder/src/events.rs` | Winit user-event bus + `webview::Embedder` impl |
| `winit-embedder/src/shared.rs` | Clipboard, screenshot, startup/URL helpers |
