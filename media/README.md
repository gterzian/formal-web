# Media crate

Owns the `formal-web-media` process and all media pipeline state. The process is
spawned lazily by the user agent on the first media request.

The media process is built around a **backend-agnostic** core: the generic
`run_media_process` function works with any `MediaBackend` implementation,
selected at compile time via Cargo features.

## Architecture

```
user agent (MediaHandler)  ──IPC──▶  media process (run_media_process<B: MediaBackend>)
                                           │
                                           ├─ B::Pipeline produces VideoFrame
                                           │   └─ crossbeam channel ──▶ frame forwarding
                                           │
                                           └─ VideoFrame (IpcSharedMemory) ──▶ compositor
```

## Quick Start

### GStreamer backend (default)

```bash
# Build everything
cargo build --release

# Run in windowed mode
cargo run --release
```

### AVFoundation backend (macOS)

```bash
# Build the media binary with AVFoundation
cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-avfoundation

# Run (uses the AVFoundation media binary)
cargo run --release
```

> **Note:** `cargo run --release` does NOT rebuild the media binary. Build it
> separately with `--features backend-avfoundation` first.

### Verify which backend is active

```bash
RUST_LOG=info cargo run --release 2>&1 | grep "creating pipeline"
# GStreamer: no "[avf]" prefix
# AVFoundation: "[avf] p0: ready"
```

## Module layout

```
media/
├── Cargo.toml
├── src/
│   ├── lib.rs                       # run_media_process<B>, run_media_process_from_args
│   ├── backend/
│   │   ├── mod.rs                   # MediaBackend + PipelineHandle traits, BackendEvent enum
│   │   ├── gstreamer/
│   │   │   ├── mod.rs               # GStreamerBackend impl
│   │   │   └── pipeline.rs          # GstPipeline (uridecodebin → videoconvert → appsink)
│   │   └── avfoundation/
│   │       ├── mod.rs               # AvfBackend impl
│   │       └── pipeline.rs          # AvfPipeline (AVPlayer + AVPlayerItemVideoOutput)
│   └── bin/
│       └── media_process.rs         # binary entrypoint
```

## Backend traits

### `BackendEvent`

A backend-agnostic event type produced by the backend's notification mechanism
(GStreamer bus, AVFoundation KVO/notifications) and consumed by the generic
dispatch loop.

```rust
pub enum BackendEvent {
    Eos { pipeline_id: MediaPipelineId },
    Error { pipeline_id: MediaPipelineId, message: String },
    DurationChanged { pipeline_id: MediaPipelineId, duration_secs: f64 },
}
```

### `PipelineHandle`

Represents one running media pipeline. Each backend provides its own concrete type.

```rust
pub trait PipelineHandle: Send + 'static {
    fn play(&self) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
    fn seek(&self, position_secs: f64) -> Result<(), String>;
    fn destroy(self) -> Result<(), String>;
}
```

### `MediaBackend`

Factory and event source.

```rust
pub trait MediaBackend: Send + 'static {
    type Pipeline: PipelineHandle;
    fn init() -> Result<Self, String>;
    fn create_pipeline(&mut self, id: MediaPipelineId, url: String,
                       frame_tx: Sender<MediaEvent>) -> Result<Self::Pipeline, String>;
    fn event_receiver(&self) -> Receiver<BackendEvent>;
}
```

## Cargo Features

```toml
[features]
default = ["backend-gstreamer"]
backend-gstreamer    = ["dep:gstreamer", "dep:gstreamer-app"]
backend-avfoundation = [
    "dep:objc2",
    "dep:objc2-foundation",
    "dep:objc2-av-foundation",
    "dep:objc2-core-media",
    "dep:objc2-core-video",
]
```

A compile-time guard in `backend/mod.rs` ensures exactly one backend is active.

## GStreamer backend

### Pipeline topology

```
uridecodebin ──▶ videoconvert ──▶ appsink (format=RGBA)
```

- `uridecodebin` dynamically creates a video pad when it detects a video stream.
- `videoconvert` converts to a format compatible with the appsink caps.
- `appsink` is configured with caps `video/x-raw,format=RGBA` and a
  `new_sample` callback that fires on the GStreamer streaming thread.
- Bus messages (EOS, error, duration-changed) are converted to `BackendEvent`
  inside a sync handler and forwarded on a crossbeam channel.

### Frame extraction (push model)

The `new_sample` callback fires for every decoded frame. This is a push model —
GStreamer calls us when a frame is ready. Compare with AVFoundation (poll model).

### Required imports (GStreamer 0.23 / 1.28)

```rust
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_app::prelude::*;
```

### Key API patterns

| Concept | v0.23 API |
|---|---|
| `ElementFactory::make(name)` | Returns `ElementBuilder`, call `.build()` |
| Object properties | `element.set_property_from_str("name", "value")` |
| `set_state(state)` | Returns `Result<StateChangeSuccess, BoolError>` |
| `query_duration::<ClockTime>()` | On `ElementExtManual` trait |
| `add_many(&[elements])` | On `GstBinExtManual` trait |
| `dynamic_cast::<T>()` | On `Cast` trait |
| `connect_pad_added()` | On `ElementExt` trait |

## AVFoundation backend

### Pipeline topology

```
AVPlayer ──▶ AVPlayerItem ──▶ AVPlayerItemVideoOutput ──▶ poll loop
```

- `AVPlayer` created via `playerWithURL:` (implicitly creates an `AVPlayerItem`).
- `AVPlayerItemVideoOutput` is added to the item to extract raw pixel buffers.
- `suppressesPlayerRendering` is set to `true` — we handle display ourselves.
- A polling loop runs at ~33 ms intervals using `try_recv` + `NSRunLoop` drain.
- Frame data is converted from `CVPixelBuffer` to packed `Vec<u8>` (row-padding
  stripped) and sent as `MediaEvent::Frame(VideoFrame)`.

### Threading model

All AVFoundation objects are `MainThreadOnly` (enforced by `objc2`'s
`MainThreadMarker`). The pipeline runs on a dedicated background thread
("formal-web-avf-worker") with an unsafely-obtained marker. This is safe
because all AVPlayer/AVPlayerItem access is serialized on this single thread
via the command queue.

### Frame extraction (poll model)

Unlike GStreamer's push model, `AVPlayerItemVideoOutput` requires polling:

```
loop {
    // 1. Convert Mach host time to item timeline
    let host_ticks = CVGetCurrentHostTime();
    let freq = CVGetHostClockFrequency();
    let host_secs = host_ticks as f64 / freq;
    let item_time = video_output.itemTimeForHostTime(host_secs);

    // 2. Check for new pixel buffer at that time
    if video_output.hasNewPixelBufferForItemTime(item_time) {
        let pixel_buf = video_output.copyPixelBufferForItemTime(…);
        // 3. Convert CVPixelBuffer → VideoFrame
    }

    // 4. Drain the run loop so AVFoundation's internal timing advances
    NSRunLoop::currentRunLoop().runUntilDate(…);

    // 5. Process any pending commands (non-blocking)
    match cmd_rx.try_recv() { … }
}
```

### Common pitfalls

| Pitfall | Symptom | Fix |
|---|---|---|
| Using `currentTime()` instead of `itemTimeForHostTime` | Only first frame ever delivered; repeated calls with same timestamp | Use `CVGetCurrentHostTime()` → `itemTimeForHostTime` |
| No run loop drain | `hasNewPixelBufferForItemTime` always returns `false` | `NSRunLoop::currentRunLoop().runUntilDate()` each iteration |
| Using unix wall clock for host time | Item time doesn't match video timeline | Use `CVGetCurrentHostTime()` divided by `CVGetHostClockFrequency()` |
| Reading duration before asset loads | `kCMTimeIndefinite`, `seconds()` returns `NaN` | Poll `item.status() == ReadyToPlay` first |
| Creating AVPlayer off main thread without marker | Compile error (missing `MainThreadMarker`) | Use `unsafe { MainThreadMarker::new_unchecked() }` on the dedicated thread |

### Future improvements

- Replace the polling loop with `CVDisplayLink` for vsync-aligned callbacks
  (lower CPU usage, better frame timing).
- Add `AVPlayerItemDidPlayToEndTimeNotification` observer for EOS events.
- Add `AVPlayerItemFailedToPlayToEndTimeNotification` observer for error events.
- Add KVO on `AVPlayerItem.status` → `ReadyToPlay` for duration reporting.
- Request `kCVPixelFormatType_32BGRA` via pixel-buffer-attributes dictionary
  to guarantee the compositor's preferred pixel format.

## What does NOT change

- `MediaCommand` / `MediaEvent` / `MediaPipelineId` / `VideoFrame` in `ipc_messages::media`.
- The frame forwarding loop (crossbeam → shmem mapping → IPC send).
- The crossbeam `select!` loop structure in `run_media_process`.
- The IPC bootstrap in `run_media_process_from_args`.

## Non-goals (initial cut)

- **Audio output** — Both backends decode audio but it's not yet exposed to the system.
- **Zero-copy GPU path** — Future IOSurface/DMA-BUF work.
- **Seek optimization** — Initial single-keyframe seek is fine.
- **Live streams** — Not tested.
- **Text tracks** — Not implemented.
- **AVFoundation notifications** — EOS, error, and duration-changed back-events
  are not yet wired. Frame delivery and play/pause/seek/destroy work in full.
