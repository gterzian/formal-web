use crate::content::EmbedLayout;
use ipc_channel::ipc::{IpcReceiver, IpcSender, IpcSharedMemory};
use serde::{Deserialize, Serialize};

/// Identifies a pipeline within the media process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaPipelineId(pub u64);

/// Opaque paint-layer identifier for a video element.
/// Assigned by the user agent, echoed back to content, stamped on VideoEmbedSite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VideoPaintId(pub u64);

/// User agent → media process commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaCommand {
    /// Create a pipeline for this URL. Does not start playback.
    CreatePipeline {
        pipeline_id: MediaPipelineId,
        url: String,
    },
    /// Begin or resume playback.
    Play {
        pipeline_id: MediaPipelineId,
    },
    /// Pause playback. Frames stop arriving; last frame stays visible.
    Pause {
        pipeline_id: MediaPipelineId,
    },
    /// Seek to position in seconds.
    Seek {
        pipeline_id: MediaPipelineId,
        position_secs: f64,
    },
    /// Tear down the pipeline and release all resources.
    Destroy {
        pipeline_id: MediaPipelineId,
    },
    /// Shut down the media process.
    Shutdown,
}

/// A decoded video frame shipped over shared memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrame {
    pub pipeline_id: MediaPipelineId,
    pub width: u32,
    pub height: u32,
    /// RGBA8, width * height * 4 bytes.
    pub data: IpcSharedMemory,
}

/// Media process → user agent events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaEvent {
    /// A decoded frame is ready.
    Frame(VideoFrame),
    /// Pipeline reached end of stream.
    Eos {
        pipeline_id: MediaPipelineId,
    },
    /// An unrecoverable error occurred.
    Error {
        pipeline_id: MediaPipelineId,
        message: String,
    },
    /// Duration became known (seconds). May fire after Play.
    DurationChanged {
        pipeline_id: MediaPipelineId,
        duration_secs: f64,
    },
}

/// Embed-site data for video content, carried inside `EmbedSite::Video`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEmbedData {
    /// Opaque id stamped by content; user agent resolves to MediaPipelineId at compose time.
    pub paint_id: VideoPaintId,
    pub layout: EmbedLayout,
    /// Corner radius for clipping the video frame to match the element's CSS border-radius.
    /// 0.0 means rectangular (no rounding).
    pub clip_radius: f64,
}

/// Bootstrap message from the user agent to the media process.
#[derive(Debug, Serialize, Deserialize)]
pub struct MediaBootstrap {
    pub command_sender: IpcSender<MediaCommand>,
    pub event_receiver: IpcReceiver<MediaEvent>,
}
