# Embedder

- Chrome controls use explicit font family, size, and line-height on the address field instead of relying on inherited shorthand so text metrics stay stable in the embedded UI runtime.
- Single-line address inputs in Blitz should clip painted text to the padding box instead of the content box, because glyph bounds can extend above the line box and otherwise get cut off.
- Attach a window-backed `ShellProvider` to the embedder chrome document; address-bar copy and paste shortcuts route through Blitz clipboard hooks instead of touching the platform clipboard directly.
- The current embedder chrome is address-bar only; do not leave browser controls in the UI unless their end-to-end behavior is intentionally exposed.
- Keep embedder focused on winit, browser chrome, and automation; content sidecar ownership and content IPC routing belong in `user_agent`.
- Forward raw content UI events into the Rust user-agent immediately instead of batching or deduplicating them inside the embedder; the event-loop queue owns coalescing and fairness across `DispatchEvent` and `UpdateTheRendering` work.
- Request a window redraw for visible content input and when a new paint frame arrives, then let winit coalesce those redraw requests; the embedder should present whatever paint frame it has from `RedrawRequested` rather than maintaining its own paint scheduler.
- Hide the native window as soon as close is accepted, before runtime teardown continues, so visible shutdown latency is not coupled to user-agent, fetch, timer, or content-process cleanup.
- Browser automation should target the current top-level traversable or webview id, not a transient document id; browser-UI replacement navigation can retire the old document before the next WebDriver script step runs.