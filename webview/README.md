# webview crate

Core webview state management for paint frames and rendering scenes.

## Responsibility

The `webview` crate owns:
- Paint frame reception and scene decoding from content process
- Per-webview render state and active navigable tracking
- Font transport receiver lifecycle
- Redraw signaling through the `EmbedderApi` trait

## Design Notes

- **`WebviewProvider`**: Manages a HashMap of webviews keyed by `WebviewId` (stable traversable identifier). Each visible webview tracks its committed root navigable plus any child content navigables whose host webviews paint into that parent traversable's compositor.
- Each webview keeps recorded paint frames in a hidden compositor keyed by `frame_id`; the compositor lazily replays the committed root frame, resolves typed `Placeholder` commands against cached child frames for the variants it understands, and refreshes placeholder-derived child viewport metadata before hit testing.
- Rebuild child hit-test regions from the same `compose_frame` walk that decides which iframe placeholders are visible; invisible placeholders must not leave behind child-frame viewports for later hit tests.
- If input arrives before a redraw has recomposed the scene, refresh the hit-test metadata by running that same compose walk first instead of rebuilding child-frame coordinates from a separate scene-tree pass.
- The compositor stores iframe placeholder clips in device pixels because paint frames use physical viewport sizes; convert incoming UI-event coordinates into that space for hit testing, then convert child-frame offsets and routed pointer or wheel coordinates back to CSS pixels before forwarding them into content.
- Treat only top-level traversable frame ids as compositor roots; nested iframe frame ids stay cached as child content navigables, use placeholder bounds as their on-screen viewport for hit testing, and route pointer or wheel events to the child-navigable host webview registered for that content navigable without changing the browser-ui active traversable.
- When a hit lands in a visible child frame, reuse the compositor's child-local hit point when rewriting pointer or wheel coordinates for the child content traversable; subtracting only the placeholder origin loses the scale transform from the embedded scene.
- Track the last focused composed frame per root webview and route non-positional events such as key, IME, and standard-keybinding input to that frame's owning content traversable; cross-origin iframe shortcuts should keep following the iframe after pointer focus even though those events carry no coordinates.
- Each UI event should request redraw on the visible root webview, dispatch the event to the hit-tested frame's owning content traversable, and fan rendering-opportunity messages across every frame that participates in the current composed scene.

- **`EmbedderApi` trait**: Abstract interface for embedder-specific concerns like redraw signaling. This allows the webview crate to remain independent of platform/window details (winit, rendering backend, etc.).

- **Embedder-specific helpers**: Navigation, viewport updates, and UI event dispatch remain in the embedder crate as they require access to Lean runtime hooks and global state. The embedder providers the implementations (`webview_provider_navigate`, `webview_provider_send_ui_event`, etc.) that wrap WebviewProvider state access with embedder-specific messaging.
