use crate::content::{FrameId, PaintFrame, RegisteredFont, WebviewId};
use crate::media::{VideoFrame, VideoPaintId};
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
    /// A decoded video frame from the media backend, ready for compositing.
    VideoFrameReady {
        webview_id: WebviewId,
        paint_id: VideoPaintId,
        /// RGBA8 pixel data.
        data: VideoFrame,
    },
    /// Remove a video frame slot (pipeline destroyed).
    RemoveVideoFrame {
        webview_id: WebviewId,
        paint_id: VideoPaintId,
    },
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

/// Hit-testing info for a single frame in the composed scene.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHitInfo {
    pub frame_id: FrameId,
    /// The webview that owns this frame.
    pub webview_id: WebviewId,
    /// Local viewport size (logical pixels).
    pub viewport_width: u32,
    pub viewport_height: u32,
    /// Offset from the parent frame's origin (in root coordinates).
    pub offset_x: f32,
    pub offset_y: f32,
    /// Whether this frame is a child of another frame.
    pub is_child_frame: bool,
    /// Whether this frame has child frames.
    pub has_child_frames: bool,
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
