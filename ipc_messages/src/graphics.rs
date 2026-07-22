use crate::content::{FrameId, PaintFrame, RegisteredFont, WebviewId};
use crate::media::{MediaPipelineId, VideoPaintId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies a per-webview compositor slot within the graphics process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompositorSlotId(pub Uuid);

impl CompositorSlotId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CompositorSlotId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GraphicsCommand — messages from user agent → graphics process
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphicsCommand {
    /// Register a new webview compositor slot.
    RegisterWebview { webview_id: WebviewId },
    /// Unregister a webview compositor slot.
    UnregisterWebview { webview_id: WebviewId },
    /// A paint frame (scene + composition metadata) from a content process.
    /// The full PaintFrame with its shmem regions is reconstructed before sending.
    PaintFrame { frame: PaintFrame },
    /// Remove a video frame slot (pipeline destroyed).
    RemoveVideoFrame {
        webview_id: WebviewId,
        paint_id: VideoPaintId,
    },
    /// Create a media pipeline (video playback) internally in the graphics process.
    CreateMediaPipeline {
        pipeline_id: MediaPipelineId,
        url: String,
    },
    /// Start or resume playback of a media pipeline.
    MediaPlay { pipeline_id: MediaPipelineId },
    /// Pause playback of a media pipeline.
    MediaPause { pipeline_id: MediaPipelineId },
    /// Seek a media pipeline to a position.
    MediaSeek {
        pipeline_id: MediaPipelineId,
        position_secs: f64,
    },
    /// Destroy a media pipeline.
    MediaDestroy { pipeline_id: MediaPipelineId },
    /// Register a child navigable host mapping.
    RegisterChildNavigableHost {
        child_webview_id: WebviewId,
        parent_traversable_id: WebviewId,
        content_frame_id: FrameId,
    },
    /// Notify the compositor that a child navigation was finalized.
    ChildNavigationFinalized {
        parent_traversable_id: WebviewId,
        content_frame_id: FrameId,
    },
    /// Shut down the graphics process.
    Shutdown,
}

// ---------------------------------------------------------------------------
// GraphicsEvent — messages from graphics process → user agent
// ---------------------------------------------------------------------------

/// Frame tree node layout data — published by the graphics process for the UA
/// to do hit-testing and event routing. Each node represents one frame (root,
/// iframe child, or video frame slot) with its position and clip rect in root
/// coordinates, plus the transform from child local space to parent space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHitInfo {
    pub frame_id: FrameId,
    /// The webview that owns this frame.
    pub webview_id: WebviewId,
    /// Parent frame, if this is a child frame.
    pub parent_frame_id: Option<FrameId>,
    /// Viewport width in logical pixels.
    pub viewport_width: u32,
    /// Viewport height in logical pixels.
    pub viewport_height: u32,
    /// Clip rectangle in root coordinates [x0, y0, x1, y1].
    /// The UA checks if a pointer event falls within this rect
    /// to determine which frame the event targets.
    pub root_clip_bounds: [f64; 4],
    /// Affine transform [a, b, c, d, tx, ty] from this frame's local
    /// coordinate space to its parent frame's space. The UA uses this
    /// to convert pointer coordinates when traversing the frame tree.
    pub child_to_parent_transform: [f64; 6],
    /// IDs of direct child frames in this frame's embed tree.
    pub child_frame_ids: Vec<FrameId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphicsEvent {
    /// A composed scene is ready for one webview. The scene bytes and font data
    /// are placed in the IPC shared memory map under the provided keys.
    ComposedSceneReady {
        webview_id: WebviewId,
        /// Key into the IPC shared memory map for the serialized scene bytes.
        scene_shmem_key: usize,
        /// Font registrations for the scene's glyph runs. Each font's data
        /// is in the shared memory map under its `data_shmem_key`.
        font_registrations: Vec<RegisteredFont>,
        /// Hit-testing info for the frame tree.
        frame_hit_info: Vec<FrameHitInfo>,
    },
    /// The graphics process is shutting down.
    ShutdownComplete,
}
