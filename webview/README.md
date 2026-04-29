# webview crate

Core webview state management for paint frames and rendering scenes.

## Responsibility

The `webview` crate owns:
- Paint frame reception and scene decoding from content process
- Per-webview render state (scene, viewport scroll, document ID tracking)
- Font transport receiver lifecycle
- Redraw signaling through the `EmbedderApi` trait

## Design Notes

- **`WebviewProvider`**: Manages a HashMap of webviews keyed by `WebviewId` (stable traversable identifier). Each webview tracks its latest rendered scene and associated document ID.
- Each webview now keeps recorded paint frames in a hidden compositor keyed by `frame_id`; the compositor lazily replays the committed root frame and resolves `IframePlaceholder` commands against cached child frames when the embedder asks for the current scene.

- **`EmbedderApi` trait**: Abstract interface for embedder-specific concerns like redraw signaling. This allows the webview crate to remain independent of platform/window details (winit, rendering backend, etc.).

- **Embedder-specific helpers**: Navigation, viewport updates, and UI event dispatch remain in the embedder crate as they require access to Lean runtime hooks and global state. The embedder providers the implementations (`webview_provider_navigate`, `webview_provider_send_ui_event`, etc.) that wrap WebviewProvider state access with embedder-specific messaging.
