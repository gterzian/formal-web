# embedder crate

The embedder crate owns the native window, browser chrome, redraw integration, and automation-facing host behavior.

- Keep the embedder focused on winit, window state, chrome UI, and automation hooks; content sidecar ownership and cross-process orchestration belong in `user_agent`.
- The current embedder chrome is address-bar only.
- Forward content UI events to the user agent promptly, and let the event-loop and windowing layers coalesce work.
- Request redraw on visible input and on new paint frames, then present from the next `RedrawRequested` instead of maintaining a separate paint scheduler.
- Browser automation should target the current top-level traversable or webview, and content-side element activation should resolve selectors in content instead of synthesizing viewport guesses in the embedder.