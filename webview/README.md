# webview crate

The `webview` crate owns per-webview compositor state: committed paint frames, embed-site composition, hit testing, and redraw signaling through `EmbedderApi`.

- `WebviewProvider` tracks webviews by stable `WebviewId`, stores committed root and child frames, and composes child embed sites into the visible scene.
- Hit testing and event routing operate on the same composed-frame geometry used for painting, including child-frame transforms and visibility.
- Root webviews track the focused composed frame so non-positional input routes to the correct content traversable.
- `EmbedderApi` keeps redraw signaling and similar host integration abstract.
- Navigation requests, viewport updates, and window-specific messaging still enter through the embedder event loop, while `WebviewProvider` keeps the owned user-agent handle encapsulated behind its compositor-facing API.
