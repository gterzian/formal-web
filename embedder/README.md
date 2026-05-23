# embedder crate

The embedder crate owns two runtime implementations: a headed embedder for the native window, browser chrome, and redraw loop, and a headless embedder for automation-only hosting without the window/chrome stack.

- Keep headed runtime behavior in `embedder/src/windowed.rs`, and keep `embedder/src/lib.rs` focused on shared event-loop APIs and cross-runtime helpers.
- Keep the headed embedder focused on winit, window state, chrome UI, and redraw integration; content sidecar ownership and cross-process orchestration belong in `user_agent`.
- Keep the headless embedder focused on automation hosting, fixed viewport publication, and event-loop plumbing; it should not recreate the headed window/chrome path behind a hidden window flag.
- The current embedder chrome is address-bar only.
- Forward content UI events to the user agent promptly, and let the event-loop and windowing layers coalesce work.
- Request redraw on visible input and on new paint frames, then present from the next `RedrawRequested` instead of maintaining a separate paint scheduler.
- Browser automation should target the current top-level traversable or webview through the headless embedder, and content-side element activation should resolve selectors in content instead of synthesizing viewport guesses in the headed embedder.