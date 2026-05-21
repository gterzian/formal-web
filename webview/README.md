# webview crate

The `webview` crate owns per-webview compositor state: committed paint frames, scene composition, iframe placeholder composition, hit testing, and redraw signaling through `EmbedderApi`.

- `WebviewProvider` tracks webviews by stable `WebviewId`, stores committed root and child frames, and composes child placeholders into the visible scene.
- Hit testing and event routing operate on the same composed-frame geometry used for painting, including child-frame transforms and visibility.
- Root webviews track the focused composed frame so non-positional input routes to the correct content traversable.
- `EmbedderApi` keeps redraw signaling and similar host integration abstract.
- Navigation, viewport updates, and window-specific messaging stay in the embedder crate, which wraps `WebviewProvider` with the owned user-agent handle and platform state.
