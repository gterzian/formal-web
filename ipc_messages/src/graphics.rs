use crate::content::{CanvasId, EmbedBackgroundPolicy, FrameId, PaintFrame, WebviewId};
use crate::media::{MediaPipelineId, VideoPaintId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[cfg(target_os = "macos")]
use ipc_channel::platform::OsMachPort;

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

/// Identifies one compositable layer within a webview: a cross-origin
/// navigable (iframe), a `<video>` embed site, or an offscreen canvas. Same-
/// origin iframes are baked into their parent's recorded scene and get no
/// layer of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompositingLayerId {
    Navigable(FrameId),
    Video(VideoPaintId),
    Canvas(CanvasId),
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
    /// The user agent started a rendering cycle: the embedder needs a frame
    /// for `webview_id`. The graphics process arms a composition deadline, so
    /// a layer that arrives before the top-level frame (a worker's
    /// `CanvasPaint`, a video frame, a child frame) is composited against the
    /// last committed root when the deadline expires, even though the
    /// top-level frame is late or never arrives (a blocked content event
    /// loop). The deadline is cleared when the top-level frame arrives.
    RenderStarted {
        webview_id: WebviewId,
        /// Milliseconds to wait for the top-level frame before composing with
        /// the last committed root.
        deadline_ms: u64,
    },
    /// A committed scene for an offscreen canvas, from the worker (or window)
    /// realm that owns the transferred `OffscreenCanvas`. The scene bytes
    /// travel in the IPC shared-memory map under `scene_shmem_key`.
    CanvasPaint {
        canvas_id: CanvasId,
        width: u32,
        height: u32,
        scene_shmem_key: usize,
    },
    /// Register an offscreen canvas with the webview it belongs to. The
    /// worker that draws the canvas does not know its owner webview id; the
    /// owner content process sends this when `transferControlToOffscreen`
    /// runs, and `CanvasPaint` is then routed to the registered webview.
    RegisterCanvas {
        webview_id: WebviewId,
        canvas_id: CanvasId,
    },
    /// Remove a video frame slot (pipeline destroyed).
    RemoveVideoFrame {
        webview_id: WebviewId,
        paint_id: VideoPaintId,
    },
    /// Create a media pipeline (video playback) internally in the graphics process.
    CreateMediaPipeline {
        pipeline_id: MediaPipelineId,
        url: String,
        webview_id: WebviewId,
        video_paint_id: VideoPaintId,
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
    /// Notify the compositor that a top-level navigation finalized.
    /// Resets the compositor so the next PaintFrame replaces the old root scene.
    NavigationFinalized { webview_id: WebviewId },
    /// Forward a TLA+ trace sender (dev only, ipc-channel mode).
    /// Sent by the UA right after launch, before any other commands.
    SetTraceSender(Option<verification::TraceSender>),
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
    /// A rendered surface frame is ready for one webview. The layers are
    /// delivered per `LayerTopology`; each layer's pixels travel according to
    /// its `surface`:
    ///
    /// - `CpuShmem`: bytes live in the IPC shared memory region carried
    ///   alongside the message; the embedder uploads them in place.
    /// - `SharedTexture` (macOS): the frame was rendered directly into a
    ///   shared IOSurface; the embedder imports the surface and blits it.
    PixelFrameReady {
        webview_id: WebviewId,
        /// The per-layer topology for every live layer, plus the rendered
        /// surface for each layer that was re-rasterized this cycle.
        layers: Vec<LayerTopology>,
        /// True when the composed scene contains animated content (video)
        /// that requires the UA to re-note a rendering opportunity even
        /// without user input.
        animating: bool,
        /// The composed frames (the top-level frame and embedded child
        /// frames) that carry the animating flag; the UA notes rendering
        /// opportunities for these navigables.
        animating_frame_ids: Vec<FrameId>,
        generation: u64,
        frame_hit_info: Vec<FrameHitInfo>,
        child_viewports: Vec<ChildViewport>,
        child_frame_to_webview: HashMap<FrameId, WebviewId>,
    },

    /// A media pipeline reached end of stream. The UA should forward
    /// this to the relevant content process so it can unset any
    /// animating flags.
    VideoEnded {
        webview_id: WebviewId,
        video_paint_id: VideoPaintId,
    },
    /// A cross-origin child frame's content changed (a source the top-level
    /// content process does not itself drive). The UA notes a rendering
    /// opportunity for the parent traversable so its render cycle re-composes
    /// and includes the child's latest frame — without this, the child's
    /// change sits in the parent's compositor until an unrelated input event
    /// drives a top-level render.
    CompositionChanged { webview_id: WebviewId },
    /// The graphics process is shutting down.
    ShutdownComplete,
}

/// One layer's topology, published to the UA per rendered cycle. Topology is
/// sent for every live layer regardless of dirtiness (geometry can move
/// independent of content); `surface` is present only when that layer was
/// actually re-rendered this cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerTopology {
    pub layer_id: CompositingLayerId,
    pub parent: Option<CompositingLayerId>,
    /// Affine [a, b, c, d, tx, ty] mapping this layer's local coordinates
    /// into its parent's local space.
    pub transform: [f64; 6],
    /// This layer's visible clip rect in its parent's local space.
    pub clip_bounds: [f64; 4],
    pub corner_radius: f64,
    /// (z_index, paint_order) within the parent, for sibling ordering.
    pub z_order: (i32, u32),
    pub background: Option<EmbedBackgroundPolicy>,
    pub width: u32,
    pub height: u32,
    /// Present only for the layers rendered this cycle.
    pub surface: Option<SurfacePayload>,
}

/// How a rendered frame's pixels are delivered to the embedder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SurfacePayload {
    /// CPU readback pixels in the IPC shared-memory map (the default off
    /// macOS, where the GStreamer media backend delivers CPU video bytes;
    /// opt-in on macOS via the graphics crate's `cpu_readback` build).
    CpuShmem {
        /// Key into the IPC shared memory map for the rendered RGBA pixel buffer.
        shmem_key: usize,
    },
    /// macOS zero-copy: the frame was rendered directly into a shared
    /// IOSurface, whose global IOSurfaceID and Mach port travel in this
    /// variant. The embedder looks the surface up by ID (`IOSurfaceLookup`)
    /// and blits it, falling back to the port lookup when the ID is not
    /// resolvable. The by-ID object is what CoreAnimation composites: a
    /// surface object imported only from the port renders empty in the
    /// layer.
    #[cfg(target_os = "macos")]
    SharedTexture {
        /// Stable identity of the shared IOSurface; changes on resize.
        texture_id: u64,
        /// Global IOSurfaceID of the shared IOSurface.
        surface_id: u32,
        /// Mach port (send right) to the IOSurface, for the fallback lookup.
        port: OsMachPort,
    },
}

/// A rendered surface frame delivered to the embedder: the pixel delivery
/// backend plus its payload. This is the boundary type (user agent →
/// embedder event) — unlike the wire [`SurfacePayload`], the CPU path's
/// shared-memory region is already extracted and carried here.
#[derive(Debug)]
pub enum SurfaceFrame {
    /// CPU readback pixels in a shared-memory region.
    CpuShmem(ipc::IpcSharedRegion),
    /// macOS zero-copy: a shared IOSurface to import and blit.
    #[cfg(target_os = "macos")]
    SharedTexture {
        /// Stable identity of the shared IOSurface; changes on resize.
        texture_id: u64,
        /// Global IOSurfaceID of the shared IOSurface.
        surface_id: u32,
        /// Mach port (send right) to the IOSurface, for the fallback lookup.
        port: OsMachPort,
    },
}

/// A boundary layer frame delivered to the embedder: the wire topology
/// (`surface` always `None`) plus the actual [`SurfaceFrame`] for the layer,
/// present only when the layer was re-rendered this cycle.
#[derive(Debug)]
pub struct LayerFrame {
    pub topology: LayerTopology,
    pub frame: Option<SurfaceFrame>,
}

/// Viewport data for a child frame (iframe), used by the UA to publish
/// viewport dimensions to child traversables via set_traversable_viewport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildViewport {
    /// The child webview that owns this frame.
    pub child_webview_id: WebviewId,
    /// Clip rectangle in root coordinates [x0, y0, x1, y1].
    pub root_clip_bounds: [f64; 4],
}
