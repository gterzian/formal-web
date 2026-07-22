use crate::content::EmbedLayout;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies a pipeline within the media or graphics process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaPipelineId(pub Uuid);

/// Opaque paint-layer identifier for a video element.
/// <https://html.spec.whatwg.org/#the-video-element>
///
/// Assigned once per video element at construction time using a UUID so that the
/// identifier is globally unique across all documents and traversables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VideoPaintId(pub Uuid);

impl VideoPaintId {
    /// Create a new globally unique video paint identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Construct from the raw 16-byte representation of a UUID.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    /// Return the inner UUID as 16 bytes.
    pub fn as_bytes(self) -> [u8; 16] {
        *self.0.as_bytes()
    }
}

impl Default for VideoPaintId {
    fn default() -> Self {
        Self::new()
    }
}

/// A decoded video frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrame {
    pub pipeline_id: MediaPipelineId,
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixel data. `#[serde(skip)]` — not serialized over IPC.
    /// Carried locally within the graphics process and
    /// extracted into the IPC shared memory map before serialization.
    #[serde(skip)]
    pub data: Vec<u8>,
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
