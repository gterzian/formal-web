# embedder crate

The embedder crate owns two app implementations: a headed embedder for the native window, browser chrome, and redraw loop, and a headless embedder for automation-only hosting without the window/chrome stack.

- Keep headed embedder behavior in `embedder/src/event_loop/windowed/mod.rs` and `embedder/src/event_loop/windowed/chrome.rs`, and keep shared event-loop orchestration in `embedder/src/event_loop/mod.rs` and `embedder/src/event_loop/winit.rs`.
- Keep the headed embedder focused on winit, window state, chrome UI, and redraw integration; content sidecar ownership and cross-process orchestration belong in `user_agent`.
- Keep the headless embedder focused on automation hosting, fixed viewport publication, and event-loop plumbing; it should not recreate the headed window/chrome path behind a hidden window flag.
- The current embedder chrome is address-bar only.
- Forward content UI events to the user agent promptly, and let the event-loop and windowing layers coalesce work.
- Request redraw on visible input and on new paint frames, then present from the next `RedrawRequested` instead of maintaining a separate paint scheduler.
- Browser automation should target the current top-level traversable or webview through the headless embedder, whether the client speaks WebDriver or CDP, and content-side element activation should resolve selectors in content instead of synthesizing viewport guesses in the headed embedder.