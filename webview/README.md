# webview crate

The `webview` crate is the embedder-facing API of formal-web: the embedder
crates depend on `webview` alone and never name the ipc crates underneath
it.

- Defines the `Embedder` host-interface trait (navigation, paint, clipboard,
  viewport, and window-title callbacks) that the embedder backends implement.
  `WebviewProvider::new` adapts it to the user agent's own host interface
  (`user_agent::UserAgentHost`).
- Re-exports the host-facing type vocabulary whose definitions live in the
  ipc crates: `WebviewId`, the composed-scene payloads (`RecordedScene`,
  `deserialize_scene_from_slice`, `FontTransportReceiver`, `RegisteredFont`),
  the per-layer frame payloads (`LayerFrame`, `SurfaceFrame`,
  `CompositingLayerId`), and the macOS surface-port deallocation helper.
- Re-exports the input event types (`UiEvent`, `BlitzKeyEvent`,
  `BlitzPointerEvent`, …) that embedder backends convert platform input
  into, and `ColorScheme`.
- `WebviewProvider` owns the user-agent handle and exposes the embedder entry
  points: top-level traversal startup, navigation, viewport publication, UI
  event forwarding (serialized in `ui_event`), script evaluation, and the
  frame-needed pacing signal.
