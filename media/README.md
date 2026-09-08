# media crate

Backend library for video/audio playback. There is no separate media
process: the `media` crate compiles its media backends into the
`formal-web-graphics` process, which owns both scene composition and media
pipeline state. See the root `README.md` for the graphics-process build
commands that select the backend (AVFoundation is the default on macOS;
`-p graphics --features backend-gstreamer,cpu_readback` selects GStreamer).

The crate is a **backend-agnostic** core: the backends implement a shared
`MediaBackend` trait (`backend/mod.rs`) and are selected at compile time via
Cargo features. The graphics process event loop drives playback — the
`GraphicsCommand` create/play/pause/seek/destroy variants in
`ipc_messages::graphics` map onto `MediaBackend`/`PipelineHandle` calls — and
forwards decoded frames to the compositor; a backend contributes a pipeline
implementation, an event source, and frame delivery only.

## Backend selection and features

```toml
[features]
default = []
backend-gstreamer    = ["dep:gstreamer", "dep:gstreamer-app"]
backend-avfoundation = []
ipc-channel-backend  = ["ipc/ipc-channel-backend"]
```

`backend-avfoundation` is macOS/iOS only and just gates the code module — the
objc2 dependencies are always present on Apple platforms. `backend-gstreamer`
acts as a marker on Apple (it enables the optional GStreamer deps); on non-Apple
platforms GStreamer is always compiled and the flag only enables the code
module. When no backend feature is selected, `build.rs` emits `avf_default` on
Apple (AVFoundation is the default there) and GStreamer is the only backend off
Apple.

There is no compile-time mutual-exclusion guard: the library compiles with 0,
1, or 2 backends. The active backend is chosen at compile time in the
graphics process binary (`graphics/src/bin/graphics_process.rs`) from the
same feature flags, so the backend compiled into the `media` crate matches
the one the graphics process instantiates.

## Backend traits

`backend/mod.rs` defines three pieces a backend supplies; each backend
provides its own concrete types under `backend/<name>/`:

- `MediaBackend` — factory and event source: `init()`, `create_pipeline(id,
  url)`, `event_receiver()`.  Decoded frames and lifecycle notifications are
  delivered as `MediaBackendEvent`s on the receiver, not on a separate
  frame channel.
- `PipelineHandle` — one running pipeline: `play`/`pause`/`seek`, a
  `sample()` hook called by the graphics process select loop (backends pump
  run loops, poll for frames, etc.), `is_done()` for end-of-stream, `destroy()`.
- `MediaBackendEvent` — the backend-agnostic notification type: decoded
  frames (`Frame(VideoFrame)` CPU bytes, `PixelBufferFrame` GPU-backed
  pixel buffer) plus EOS, error, and duration-changed, produced by the
  backend's notification mechanism (GStreamer bus, AVFoundation
  KVO/notifications) and consumed by the graphics process dispatch loop.

## GStreamer backend

The pipeline is `uridecodebin → videoconvert → appsink (format=RGBA)`;
`uridecodebin` creates a video pad dynamically when it detects a video
stream.  Frames are delivered by push: the appsink `new_sample` callback
fires on the GStreamer streaming thread for every decoded frame, converts
bus messages (EOS, error, duration-changed) to `MediaBackendEvent`s in a sync
handler, and forwards on a crossbeam channel.  No polling, run loop, or
timing infrastructure is required.

Required imports (GStreamer 0.23 / 1.28):

```rust
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_app::prelude::*;
```

## AVFoundation backend

The pipeline is `AVPlayer → AVPlayerItem → AVPlayerItemVideoOutput`, run on
the graphics process main thread — no background thread, because
AVFoundation objects require `MainThreadMarker`.

`sample()` (≈120 Hz via the graphics process's 8ms select-loop tick when at
least one pipeline is active) drains the run loop (`runUntilDate(8ms)`) so
AVFoundation services URL loading, KVO, and video output timing; checks item
status once (wait for `AVPlayerItemStatus::ReadyToPlay` before reporting
duration); then polls `AVPlayerItemVideoOutput` for frames
(`itemTimeForHostTime`, `hasNewPixelBufferForItemTime`) and delivers the
pixel buffer itself as `PixelBufferFrame` — the graphics process wraps it as
a Metal texture (zero-copy when GPU-backed) instead of the CPU byte
conversion.

Key design decisions:

| Decision | Why |
|---|---|
| No background thread | AVFoundation objects require `MainThreadMarker`. The graphics
  process main thread hosts the backend. |
| Timer-driven `sample()`, not message-driven | Without a timer, `sample()` only runs when a command or event arrives,
  starving AVFoundation of CPU time. |
| Frames flow through the same channel as EOS/error/duration | Eliminates the `frame_tx`/`frame_rx` pair from the dispatch loop. |
| Deliver the `CVPixelBuffer` itself (BGRA), not CPU-converted bytes | The graphics process imports it as a Metal texture (zero-copy when
  GPU-backed); the BGRA→RGBA conversion Vello needs happens there, as a
  compute blit. |

### Common pitfalls

| Pitfall | Symptom | Fix |
|---|---|---|
| Using `currentTime()` instead of `itemTimeForHostTime` | Only first frame ever delivered | Use `CVGetCurrentHostTime()` → `itemTimeForHostTime` |
| Using unix wall clock for host time | Item time doesn't match video timeline | Use `CVGetCurrentHostTime()` / `CVGetHostClockFrequency()` |
| Reading duration before asset loads | `kCMTimeIndefinite`, `seconds()` returns `NaN` | Poll `item.status() == ReadyToPlay` first |
| `kCVPixelBufferPixelFormatTypeKey` double-ref | Crash during pipeline creation | Use `kCVPixelBufferPixelFormatTypeKey` directly (it's already `&CFString`), not `&kCVPixelBufferPixelFormatTypeKey` |

## Known issues and non-goals

- **Audio output** — Both backends decode audio but it's not yet exposed to the system.
- **Seek optimization** — Initial single-keyframe seek is fine.
- **Live streams** — Not tested.
- **Text tracks** — Not implemented.
