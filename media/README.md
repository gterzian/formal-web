# Media crate

Owns the `formal-web-media` process and all GStreamer pipeline state. The process is
spawned lazily by the user agent on the first media request.

## Architecture

```
user agent (MediaHandler)  ──IPC──▶  media process (run_media_process)
                                           │
                                           ├─ uridecodebin
                                           │   └─ videoconvert
                                           │       └─ appsink ◀── IpcSender<MediaEvent>
                                           │
                                           └─ VideoFrame (IpcSharedMemory) ──▶ compositor
```

- `src/main.rs` — media process main loop: drain IPC commands, poll GStreamer bus, yield.
- `src/managed_pipeline.rs` — single-pipeline wrapper: uridecodebin → videoconvert → appsink.
- `src/bin/media_process.rs` — binary entrypoint, calls `media::run_media_process_from_args()`.

## GStreamer Rust API lessons (v0.23)

The `gstreamer` 0.23 API (GStreamer 1.28.x) uses extension traits extensively.
Methods are not available directly on structs — they come from prelude traits.

### Required imports

```rust
use gstreamer as gst;
use gstreamer::prelude::*;           // ElementExt, GstBinExtManual, ElementExtManual
use gstreamer_app as gst_app;
use gstreamer_app::prelude::*;       // AppSinkCallbacks, etc.
```

### Key API changes from earlier versions

| Concept | v0.23 API |
|---|---|
| `ElementFactory::make(name)` | Returns `ElementBuilder`, call `.build()` to get `Result<Element, BoolError>` |
| Object properties | `element.set_property_from_str("name", "value")` or the typed `element.set_property("name", &value)` |
| `set_state(state)` | On `ElementExt` trait, returns `Result<StateChangeSuccess, BoolError>` |
| `query_duration::<ClockTime>()` | On `ElementExtManual` trait, returns `Option<ClockTime>` |
| `add_many(&[elements])` | On `GstBinExtManual` trait (Bin, Pipeline), returns `Result<(), BoolError>` |
| `dynamic_cast::<T>()` | On `Cast` trait, returns `Result<T, Upcast>` |
| `connect_pad_added()` | On `ElementExt` trait |
| `pipeline.bus()` | On `ElementExt` trait, returns `Option<Bus>` |

### `set_property` vs `set_property_from_str`

For string properties like URIs, use `set_property_from_str`. For typed properties
(e.g. caps), use `set_property` with the typed value.

### `seek_simple`

Available on `ElementExt` trait. Signature:
```rust
fn seek_simple(&self, flags: SeekFlags, seek_pos: impl Into<Option<ClockTime>>) -> Result<(), BoolError>
```

### Error types

GStreamer operations use `glib::BoolError` (aliased as `gst::BoolError`). The standard
pattern is `.map_err(|error| format!("...: {error}"))?`.

### Module structure

- All types accessed as `gst::Pipeline`, `gst::Element`, etc.
- Bus messages use `gst::MessageView` variants.
- AppSink callbacks use `gst_app::AppSinkCallbacks::builder().new_sample(...).build()`.
- `IpcSharedMemory::from_bytes(slice)` copies the frame data for IPC transport.

## Non-goals (initial cut)

- **Audio output** — GStreamer decodes audio but it's not yet exposed to the system.
- **Zero-copy GPU path** — Future IOSurface/DMA-BUF work.
- **Seek optimization** — Initial `seek_simple` is fine.
- **Live streams** — Not tested.
- **Text tracks** — Not implemented.
