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

### AVFoundation backend (macOS only, NOT WORKING)

```bash
# Build the media binary
cargo build --release -p media --bin formal-web-media \
  --no-default-features --features backend-avfoundation

# Run
cargo run --release
```
Currently does not deliver frames (always returns `AVPlayerItemStatusUnknown`).

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

## AVFoundation backend — current state

### Pipeline topology

```
AVPlayer ──▶ AVPlayerItem ──▶ AVPlayerItemVideoOutput ──▶ poll loop
```

### Frame extraction (poll model)

```
loop {
    NSRunLoop::currentRunLoop().runUntilDate(…33ms…);
    drain commands via try_recv;
    check item status;
    poll video_output.hasNewPixelBufferForItemTime(…);
    drain more commands;
}
```

### Status: NOT WORKING — always `AVPlayerItemStatusUnknown`

The AVFoundation backend creates the pipeline and play() is called, but
`AVPlayerItem.status` is always `Unknown` (0) and never transitions to
`ReadyToPlay` (1). No frames are ever extracted.

#### Symptoms

```
[avf] p0: status=0     ← repeated forever, never becomes 1
```

#### Root cause analysis

`NSRunLoop::runUntilDate()` on a bare background thread with no input
sources returns **immediately** instead of blocking. AVFoundation's URL
loading machinery relies on the run loop being serviced — when it isn't,
the `AVPlayerItem` never loads its media, so it never becomes
`ReadyToPlay`.

The original fix (adding a `recv_timeout`-based sleep) is the pacing
mechanism, but the first 33 ms loop iterations happen before any run
loop time is given to AVFoundation. By the time `runUntilDate` runs,
the item has been given <1 ms of cumulative run loop time from the very
first drain call, which is insufficient to even begin URL loading.

#### Attempted fixes (all failed)

| Attempt | What changed | Result |
|---|---|---|
| 1. `recv_timeout` only, no run loop drain | Removed `runUntilDate` entirely | Status always 0, no change |
| 2. `try_recv` + `runUntilDate(8ms)` | Run loop 8ms then try_recv | 100% CPU (run loop returned instantly) |
| 3. `recv_timeout(33ms)` first, then `runUntilDate(8ms)` | Two sequential sleeps totalling 41ms | Status always 0, AVFoundation starved |
| 4. `runUntilDate(33ms)` + `try_recv` (current) | Run loop owns full cadence | Status always 0 — run loop still returns instantly |
| 5. Create `AVPlayer` on main thread before `spawn` | Pipeline.init() uses `MainThreadMarker` | `AvPlayer` is `!Send`, can't move into closure |

#### Why NSRunLoop returns immediately on background threads

`NSRunLoop` has two modes:
- **With sources/timers**: blocks until the next timer fire or input source
  event, up to the specified timeout.
- **Without sources/timers**: returns immediately, regardless of the timeout.

A freshly-created background thread has no automatic run loop sources.
`AVFoundation` does NOT automatically register its URL loading with a
background thread's run loop — it registers with the **main thread's**
run loop (or a thread that the `AVPlayer`/`AVPlayerItem` was created on).

Hypothesis: `AVPlayer::playerWithURL:` registers URL loading callbacks
on the creating thread's run loop. If that thread never has its run loop
serviced, the callbacks never fire.

#### What would be needed to fix

1. **Run the AVFoundation worker on the main thread** — swap the generic
   select loop to a background thread and keep the main thread for
   `NSRunLoop` + AVFoundation operations. This requires restructuring
   `run_media_process` or having the backend own its own threading.

2. **Or: create a `CFRunLoopTimer` or `dispatch_source` on the worker
   thread** so `runUntilDate` actually has something to wait on. This
   would keep run loop occupied and let AVFoundation callbacks fire.

3. **Or: use `dispatch_async` to the main queue** for all AVFoundation
   operations, keeping the worker thread purely as a command router.

Option 1 is the most architecturally sound but requires non-trivial
refactoring of the generic run loop.

### Common pitfalls (when it does work)

| Pitfall | Symptom | Fix |
|---|---|---|
| Using `currentTime()` instead of `itemTimeForHostTime` | Only first frame ever delivered | Use `CVGetCurrentHostTime()` → `itemTimeForHostTime` |
| Using unix wall clock for host time | Item time doesn't match video timeline | Use `CVGetCurrentHostTime()` / `CVGetHostClockFrequency()` |
| Reading duration before asset loads | `kCMTimeIndefinite`, `seconds()` returns `NaN` | Poll `item.status() == ReadyToPlay` first |

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
