use ipc_messages::media::MediaPipelineId;

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

pub trait PipelineHandle: Send + 'static {
    fn play(&self) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
    fn seek(&self, position_secs: f64) -> Result<(), String>;
    fn sample(&self) {}
    fn destroy(self) -> Result<(), String>;
}

pub trait MediaBackend: Send + 'static {
    type Pipeline: PipelineHandle;
    fn init() -> Result<Self, String>
    where
        Self: Sized;
    fn create_pipeline(
        &mut self,
        id: MediaPipelineId,
        url: String,
    ) -> Result<Self::Pipeline, String>;
    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent>;
}

#[cfg(any(
    feature = "backend-gstreamer",
    not(any(target_os = "macos", target_os = "ios"))
))]
pub mod gstreamer;

#[cfg(all(
    any(feature = "backend-avfoundation", avf_default),
    any(target_os = "macos", target_os = "ios")
))]
pub mod avfoundation;
