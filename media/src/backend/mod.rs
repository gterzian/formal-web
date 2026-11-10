use ipc_messages::media::MediaPipelineId;

// ---------------------------------------------------------------------------
// MediaBackendEvent — backend-agnostic event delivered from the backend's
// notification mechanism (GStreamer bus, AVFoundation KVO/notifications) to
// the generic dispatch loop.
// ---------------------------------------------------------------------------

// No compile-time guard for mutual exclusion: the library compiles with 0, 1,
// or 2 backends. Backend selection happens at runtime in
// `run_media_process_from_args` via cfg-based priority (see lib.rs).
// The `avf_default` cfg is emitted by build.rs on Apple platforms when no
// explicit backend feature is selected.
// ---------------------------------------------------------------------------

use ipc_messages::media::VideoFrame;

/// A decoded video frame delivered as a GPU-backed pixel buffer (macOS
/// AVFoundation). The graphics process wraps its NV12 planes as Metal
/// textures and converts them to RGBA as it composites, avoiding the CPU
/// readback and the pixel-format conversion a BGRA pool would cost.
#[cfg(all(
    any(feature = "backend-avfoundation", avf_default),
    target_os = "macos"
))]
#[derive(Clone)]
pub struct PixelBufferVideoFrame {
    pub pipeline_id: MediaPipelineId,
    pub width: u32,
    pub height: u32,
    /// The decoded NV12 pixel buffer; must stay alive while the Metal
    /// textures wrapping its planes are in use.
    pixel_buffer: objc2::rc::Retained<objc2_core_video::CVPixelBuffer>,
    /// The thread that produced the frame.
    producer: std::thread::ThreadId,
}

#[cfg(all(
    any(feature = "backend-avfoundation", avf_default),
    target_os = "macos"
))]
impl PixelBufferVideoFrame {
    pub fn new(
        pipeline_id: MediaPipelineId,
        width: u32,
        height: u32,
        pixel_buffer: objc2::rc::Retained<objc2_core_video::CVPixelBuffer>,
    ) -> Self {
        Self {
            pipeline_id,
            width,
            height,
            pixel_buffer,
            producer: std::thread::current().id(),
        }
    }

    /// The decoded pixel buffer, after checking that this is the thread that
    /// produced the frame.
    pub fn pixel_buffer(&self) -> &objc2::rc::Retained<objc2_core_video::CVPixelBuffer> {
        assert_eq!(
            std::thread::current().id(),
            self.producer,
            "pixel buffer frame consumed on a thread other than the one that produced it"
        );
        &self.pixel_buffer
    }
}

// SAFETY: the pixel buffer is produced by the AVFoundation pipeline and
// consumed by the graphics event loop on the same thread (the media backend
// runs inside the graphics process's select loop); `pixel_buffer()` asserts
// that thread, so the only cross-thread operation left is dropping the frame,
// which releases the buffer.
#[cfg(all(
    any(feature = "backend-avfoundation", avf_default),
    target_os = "macos"
))]
// SAFETY: see above; the buffer is only read through `pixel_buffer()`, which
// asserts the producing thread, and dropping a `CFType` is thread-safe.
unsafe impl Send for PixelBufferVideoFrame {}

#[cfg(all(
    any(feature = "backend-avfoundation", avf_default),
    target_os = "macos"
))]
impl std::fmt::Debug for PixelBufferVideoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PixelBufferVideoFrame")
            .field("pipeline_id", &self.pipeline_id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixel_buffer", &(&*self.pixel_buffer as *const _))
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum MediaBackendEvent {
    /// A decoded video frame is ready (CPU bytes).
    Frame(VideoFrame),
    /// A decoded video frame is ready as a GPU-backed pixel buffer (macOS).
    #[cfg(all(
        any(feature = "backend-avfoundation", avf_default),
        target_os = "macos"
    ))]
    PixelBufferFrame(PixelBufferVideoFrame),
    /// The pipeline reached end of stream.
    Eos { pipeline_id: MediaPipelineId },
    /// An unrecoverable error occurred in the backend.
    Error {
        pipeline_id: MediaPipelineId,
        message: String,
    },
    /// The media duration became known (seconds).
    DurationChanged {
        pipeline_id: MediaPipelineId,
        duration_secs: f64,
    },
}

// ---------------------------------------------------------------------------
// PipelineHandle — one running media pipeline.
// ---------------------------------------------------------------------------

pub trait PipelineHandle: Send + 'static {
    /// Transition to Playing.
    fn play(&self) -> Result<(), String>;

    /// Transition to Paused.
    fn pause(&self) -> Result<(), String>;

    /// Seek to an absolute position in seconds.
    fn seek(&self, position_secs: f64) -> Result<(), String>;

    /// Called at a fixed rate by the select loop.  Backends use this to
    /// pump run loops, poll for frames, etc.  Default is a no-op.
    fn sample(&self) {}

    /// Returns true when the pipeline has reached end-of-stream and no
    /// longer needs sampling. Default returns false (always sample).
    fn is_done(&self) -> bool {
        false
    }

    /// Tear down cleanly. Takes self by value so the backend's drop logic applies
    /// without Option gymnastics in the pipeline map.
    fn destroy(self) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// MediaBackend — factory and event source.
// ---------------------------------------------------------------------------

pub trait MediaBackend: Send + 'static {
    /// The concrete pipeline handle type for this backend.
    type Pipeline: PipelineHandle;

    /// One-time initialization: gst::init + macos_main, AVAudioSession setup, etc.
    fn init() -> Result<Self, String>
    where
        Self: Sized;

    /// Create a pipeline for the given URL. The pipeline should start in the
    /// Paused state. Decoded video frames are sent via `event_receiver`
    /// as `MediaBackendEvent::Frame`.
    fn create_pipeline(
        &mut self,
        id: MediaPipelineId,
        url: String,
    ) -> Result<Self::Pipeline, String>;

    /// Returns the receiver for backend-originated events (BusEvent → MediaBackendEvent
    /// conversion happens inside the backend). Called once before the select loop.
    fn event_receiver(&self) -> crossbeam_channel::Receiver<MediaBackendEvent>;
}

// ---------------------------------------------------------------------------
// Backend modules — gated by Cargo features.
// ---------------------------------------------------------------------------

// GStreamer: always included on non-Apple platforms (dep is non-optional there).
// On Apple, gated by the `backend-gstreamer` feature (dep is optional).
#[cfg(any(
    feature = "backend-gstreamer",
    not(any(target_os = "macos", target_os = "ios"))
))]
pub mod gstreamer;

// AVFoundation: Apple only; gated by explicit feature or build.rs avf_default.
#[cfg(all(
    any(feature = "backend-avfoundation", avf_default),
    any(target_os = "macos", target_os = "ios")
))]
pub mod avfoundation;

#[cfg(all(
    test,
    any(feature = "backend-avfoundation", avf_default),
    target_os = "macos"
))]
mod tests {
    use std::sync::mpsc;

    use std::ptr::NonNull;

    use ipc_messages::media::{MediaPipelineId, VideoPaintId};
    use objc2::rc::Retained;
    use objc2_core_video::{CVBuffer, CVPixelBufferCreate};

    use super::PixelBufferVideoFrame;

    fn pixel_buffer() -> Retained<CVBuffer> {
        let mut buffer: *mut CVBuffer = std::ptr::null_mut();
        // SAFETY: the allocator and attributes are optional (null), both
        // dimensions are non-zero, and `buffer` is a live out-parameter.
        let status = unsafe {
            CVPixelBufferCreate(None, 4, 4, 0x4247_5241, None, NonNull::from(&mut buffer))
        };
        assert_eq!(status, 0, "CVPixelBufferCreate failed with {status}");
        // SAFETY: a successful `CVPixelBufferCreate` returns a +1 reference in
        // `buffer`, and `from_raw` takes ownership of that reference.
        unsafe { Retained::from_raw(buffer) }.expect("CVPixelBufferCreate returned null")
    }

    #[test]
    #[should_panic(expected = "consumed on a thread other than the one that produced it")]
    fn pixel_buffer_frame_is_thread_affine() {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let frame = PixelBufferVideoFrame::new(
                MediaPipelineId(VideoPaintId::new().0),
                4,
                4,
                pixel_buffer(),
            );
            sender.send(frame).expect("receiver alive");
        })
        .join()
        .expect("producer thread");
        let frame = receiver.recv().expect("frame delivered");
        let _ = frame.pixel_buffer();
    }
}
