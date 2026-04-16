# Embedder

- Chrome controls use explicit font family, size, and line-height on the address field instead of relying on inherited shorthand so text metrics stay stable in the embedded UI runtime.
- Single-line address inputs in Blitz should clip painted text to the padding box instead of the content box, because glyph bounds can extend above the line box and otherwise get cut off.
- The current embedder chrome is address-bar only; do not leave browser controls in the UI unless their end-to-end behavior is intentionally exposed.
- Keep embedder focused on winit and browser chrome; content-process bridging, IPC listeners, and Lean callback plumbing for content events belong in `ffi`.
- Forward raw content UI events into Lean immediately instead of batching or deduplicating them inside the embedder; the event-loop queue owns coalescing and fairness across `DispatchEvent` and `UpdateTheRendering` work.
- Request a window redraw for visible content input and when a new paint frame arrives, then let winit coalesce those redraw requests; the embedder should present whatever paint frame it has from `RedrawRequested` rather than maintaining its own paint scheduler.