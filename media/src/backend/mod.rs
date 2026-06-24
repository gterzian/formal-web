use ipc_messages::media::MediaPipelineId;

// ---------------------------------------------------------------------------
// Compile-time guard: exactly one backend feature must be active.
// ---------------------------------------------------------------------------

#[cfg(not(any(feature = "backend-gstreamer", feature = "backend-avfoundation")))]
compile_error!(
    "Exactly one media backend feature must be enabled: \
     backend-gstreamer or backend-avfoundation"
);

#[cfg(all(feature = "backend-gstreamer", feature = "backend-avfoundation"))]
compile_error!(
    "Only one media backend feature can be enabled at a time: \
     backend-gstreamer or backend-avfoundation"
);

// ---------------------------------------------------------------------------
// BackendEvent — backend-agnostic event delivered from the backend's
// notification mechanism (GStreamer bus, AVFoundation KVO/notifications) to
// the generic dispatch loop.
// ---------------------------------------------------------------------------

use ipc_messages::media::VideoFrame;

#[derive(Debug, Clone)]
pub enum BackendEvent {
    /// A decoded video frame is ready.
    Frame(VideoFrame),
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

    /// Called once per select-loop iteration.  Backends can pump run loops,
    /// poll for frames, etc.  Default is a no-op.
    fn tick(&self) {}

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
    /// as `BackendEvent::Frame`.
    fn create_pipeline(
        &mut self,
        id: MediaPipelineId,
        url: String,
    ) -> Result<Self::Pipeline, String>;

    /// Returns the receiver for backend-originated events (BusEvent → BackendEvent
    /// conversion happens inside the backend). Called once before the select loop.
    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent>;

    /// Called once per select-loop iteration.  Default is a no-op.
    fn tick(&mut self) {}
}

// ---------------------------------------------------------------------------
// Backend modules — gated by Cargo features.
// ---------------------------------------------------------------------------

#[cfg(feature = "backend-gstreamer")]
pub mod gstreamer;

#[cfg(feature = "backend-avfoundation")]
pub mod avfoundation;
