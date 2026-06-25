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

### macOS / iOS (AVFoundation backend, default)

AVFoundation is the default backend on Apple platforms.  No additional
libraries required.

```bash
# Build everything
cargo build --release

# Run in windowed mode
cargo run --release
```

> **Note:** `cargo run --release` only rebuilds the root binary (embedder).
> The `formal-web-media` process must be built separately when switching
> between backends.  Use `cargo build --release -p media --bin formal-web-media`
> after changing the backend feature.

### macOS (GStreamer backend, opt-in)

On macOS, GStreamer can be used instead of AVFoundation by explicitly
selecting the `backend-gstreamer` feature:

```bash
cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-gstreamer
```

### Linux (GStreamer backend)

On Linux, GStreamer is the only available backend and is always compiled.
Build the full workspace as usual:

```bash
cargo build --release
cargo run --release
```

### Without media (no video playback)

```bash
cargo build --release --no-default-features
cargo run --release
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
│   │       ├── pipeline.rs          # AvfPipeline (AVPlayer + AVPlayerItemVideoOutput)
│   │       └── av_sys/              # Safe wrappers around AVFoundation FFI
│   │           ├── mod.rs
│   │           ├── player.rs        # AvPlayer (wraps AVPlayer)
│   │           ├── item.rs          # AvPlayerItem (wraps AVPlayerItem)
│   │           ├── video_output.rs  # AvVideoOutput (wraps AVPlayerItemVideoOutput)
│   │           ├── pixel_buffer.rs  # PixelBufferLock, pixel_buffer_to_frame
│   │           ├── time.rs          # host_time_seconds()
│   │           └── url.rs           # url_from_string()
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
GStreamer calls us when a frame is ready. No polling, run loop, or timing
infrastructure required.

### Required imports (GStreamer 0.23 / 1.28)

```rust
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_app::prelude::*;
```

## AVFoundation backend

### Pipeline topology

```
AVPlayer ──▶ AVPlayerItem ──▶ AVPlayerItemVideoOutput ──▶ sample() poll loop
```

The pipeline runs **on the select-loop thread** (the main thread of the
media process).  No background thread is used.

### Frame extraction

Frame polling happens inside `PipelineHandle::sample()`, which is called
at ≈120 Hz via a `crossbeam_channel::tick(8ms)` timer arm in the generic
`select!` loop:

```rust
// lib.rs: the select loop drives sampling independently of message traffic
let sample_tick = crossbeam_channel::tick(Duration::from_millis(8));

loop {
    crossbeam_channel::select! {
        recv(cmd_rx) -> cmd => { ... },
        recv(backend_event_rx) -> event => { ... },
        recv(sample_tick) -> _ => {
            for pipeline in pipelines.values() {
                pipeline.sample();
            }
        },
    }
}
```

`sample()` does:
1. **Drain the run loop** via `NSRunLoop::currentRunLoop().runUntilDate(8ms)`
   — this lets AVFoundation service URL loading, KVO, and video output
   timing.
2. **Check item status** (once) — wait for `AVPlayerItemStatus::ReadyToPlay`
   before reporting duration.
3. **Poll for frames** via `AVPlayerItemVideoOutput`:
   - Convert `CVGetCurrentHostTime()` to item time via `itemTimeForHostTime`
   - Check `hasNewPixelBufferForItemTime`
   - Copy pixel buffer and convert to `VideoFrame` (BGRA → RGBA swap)
   - Send as `BackendEvent::Frame` through the backend event channel

### Key design decisions

| Decision | Why |
|---|---|
| No background thread | AVFoundation objects require `MainThreadMarker`. The select loop
  provides the main thread. |
| Timer-driven `sample()`, not message-driven | Without a timer, `sample()` only runs when a command or event arrives,
  starving AVFoundation of CPU time. |
| `BackendEvent::Frame` instead of separate `frame_tx` | Frames flow through the same channel as EOS/error/duration. Eliminates
  the `frame_tx`/`frame_rx` pair from the generic loop. |
| BGRA pixel buffer with RGBA swap in software | AVFoundation's decoder outputs BGRA natively; the compositor expects
  RGBA. Requesting RGBA from the decoder fails silently for some files. |

### Pixel format

The video output requests `kCVPixelFormatType_32BGRA` via the pixel-buffer
attributes dictionary.  The `pixel_buffer_to_frame` function then swaps
bytes 0 and 2 in each 4-byte pixel to produce RGBA output matching the
compositor's expectations.

### Common pitfalls

| Pitfall | Symptom | Fix |
|---|---|---|
| Using `currentTime()` instead of `itemTimeForHostTime` | Only first frame ever delivered | Use `CVGetCurrentHostTime()` → `itemTimeForHostTime` |
| Using unix wall clock for host time | Item time doesn't match video timeline | Use `CVGetCurrentHostTime()` / `CVGetHostClockFrequency()` |
| Reading duration before asset loads | `kCMTimeIndefinite`, `seconds()` returns `NaN` | Poll `item.status() == ReadyToPlay` first |
| `kCVPixelBufferPixelFormatTypeKey` double-ref | Crash during pipeline creation | Use `kCVPixelBufferPixelFormatTypeKey` directly (it's already `&CFString`), not `&kCVPixelBufferPixelFormatTypeKey` |
| No timer driving `sample()` | No frames delivered | Add `crossbeam_channel::tick()` arm to `select!` |
| BGRA data sent as RGBA | Blue-tinted video | Swap bytes 0 and 2 in `pixel_buffer_to_frame` |

### What does NOT change

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
