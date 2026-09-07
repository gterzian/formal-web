# webview crate

The `webview` crate is the embedder-facing API of formal-web: the embedder
crates depend on `webview` alone and never name the ipc crates underneath
it.

- Re-exports the `Embedder` host-interface trait (navigation, paint,
  clipboard, viewport, and window-title callbacks). The `user_agent` crate
  defines the trait and calls it from the user-agent thread; the embedder
  backends implement it, and `WebviewProvider` hands their implementation to
  the user agent at startup. New host callbacks are defined on
  `user_agent::Embedder` only — never mirrored on a second trait behind a
  forwarding adapter in `webview`.
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
  event forwarding (serialized in `ui_event`), script evaluation, the
  frame-needed pacing signal, the URL schemes the embedder serves itself, and
  the scripts each new document runs before it is populated. `new` also takes
  the directory the helper executables are spawned from.
