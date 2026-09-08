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
  frame-needed pacing signal, and the answers to embedder-served fetches.
- What the embedder settles once is passed at instantiation rather than
  published afterwards: `EmbedderConfig` carries the directory the extension
  executables are spawned from, the URL schemes the embedder serves itself,
  and the user scripts a webview it did not ask for by name carries. Webview
  creation is non-blocking, so a setting published after startup would race
  the first document it is meant to apply to; for the same reason the scripts
  of a named webview are an argument to `start`, the call that creates it.
