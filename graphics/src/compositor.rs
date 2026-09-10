//! Per-webview compositor — receives PaintFrames and VideoFrames, composes
//! them into a single final scene, and publishes the result plus hit-testing
//! info back to the user agent.

use anyrender::{PaintScene, Scene as RenderScene};
use ipc::IpcSharedRegion;
use ipc_messages::content::{
    CanvasId, EmbedBackgroundPolicy, EmbedSite, FontTransportReceiver, FrameCompositionMetadata,
    FrameId, IframeEmbedSite, PaintFrame, RecordedScene, serialize_scene_to_vec,
};
use ipc_messages::graphics::{CompositingLayerId, FrameHitInfo, LayerTopology, SurfacePayload};

use crate::ComposedScene;
use ipc_messages::media::VideoPaintId;
use kurbo::{Affine, Rect, Shape};
use log::{error, info, trace};
use peniko::{ImageAlphaType, ImageBrushRef, ImageData, ImageFormat};
use std::collections::{HashMap, HashSet};
use std::env;

fn input_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

#[derive(Clone, Debug)]
struct ResolvedViewport {
    width: f64,
    height: f64,
}

impl ResolvedViewport {
    fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    fn intersects_local_rect(&self, rect: Rect) -> bool {
        rect.x0 < self.width && rect.y0 < self.height && rect.x1 > 0.0 && rect.y1 > 0.0
    }
}

#[derive(Clone, Debug)]
struct NavigableContainerLayout {
    child_frame_id: FrameId,
    clip_bounds: Rect,
    root_clip_bounds: Rect,
    child_local_from_parent: Affine,
}

/// How a layer sits within its parent's coordinate space, computed by the
/// parent when it walks an embed site and passed down to the child's compose
/// call so the layer list is built at the same point as the merged scene.
#[derive(Clone)]
struct LayerPlacement {
    /// local_from_parent: maps this layer's local coordinates into its
    /// parent's local space. Identity for the root.
    transform: Affine,
    /// This layer's visible clip rect in its parent's local space.
    clip_bounds: Rect,
    corner_radius: f64,
    /// (z_index, paint_order) within the parent, for sibling ordering.
    z_order: (i32, u32),
    background: Option<EmbedBackgroundPolicy>,
}

/// One per-layer work item produced by the compose walk. Each layer owns its
/// own content scene; the embedder places/orders it by the transform, clip,
/// and z-order fields. `render` is `Some` when this layer's content changed
/// and must be re-rasterized; `None` for a clean layer that keeps its last
/// surface.
#[derive(Clone)]
pub struct LayerUpdate {
    pub layer_id: CompositingLayerId,
    pub parent: Option<CompositingLayerId>,
    pub transform: Affine,
    pub clip_bounds: Rect,
    pub corner_radius: f64,
    pub z_order: (i32, u32),
    pub background: Option<EmbedBackgroundPolicy>,
    /// The layer's content size in its own local space (the surface size).
    pub width: u32,
    pub height: u32,
    pub render: Option<RenderScene>,
}

impl LayerUpdate {
    /// The wire topology for this layer, without a rendered surface.
    pub fn into_layer_topology(&self) -> LayerTopology {
        let transform = self.transform.as_coeffs();
        LayerTopology {
            layer_id: self.layer_id,
            parent: self.parent,
            transform: [
                transform[0],
                transform[1],
                transform[2],
                transform[3],
                transform[4],
                transform[5],
            ],
            clip_bounds: [
                self.clip_bounds.x0,
                self.clip_bounds.y0,
                self.clip_bounds.x1,
                self.clip_bounds.y1,
            ],
            corner_radius: self.corner_radius,
            z_order: self.z_order,
            background: self.background,
            width: self.width,
            height: self.height,
            surface: None,
        }
    }

    /// The wire topology for this layer, attaching a rendered surface.
    pub fn into_layer_topology_with_surface(&self, surface: SurfacePayload) -> LayerTopology {
        let mut topology = self.into_layer_topology();
        topology.surface = Some(surface);
        topology
    }
}

#[derive(Clone)]
struct CachedFrame {
    viewport_width: u32,
    viewport_height: u32,
    parent_frame_id: Option<FrameId>,
    resolved_viewport: Option<ResolvedViewport>,
    child_frames: Vec<NavigableContainerLayout>,
    composition: FrameCompositionMetadata,
    scene: RecordedScene,
    /// A deterministic fingerprint of the serialized scene. RecordedScene's
    /// derived `PartialEq` is unreliable: `Paint::Custom` compares by Arc
    /// pointer and float fields use `==`, so a `NaN` (or a pointer that
    /// differs across a serialization round-trip) makes two byte-identical
    /// scenes unequal, re-rasterizing an unchanged layer on every cycle.
    /// Comparing the serialized bytes instead treats identical bits (NaN
    /// included) as equal, so a layer whose recorded content did not change
    /// stays clean.
    scene_digest: u64,
    /// True when this frame's document contains animated content (video
    /// frames still being produced, CSS animations). The composed scene
    /// aggregates it so the UA keeps noting rendering opportunities.
    animating: bool,
    /// Set whenever store_frame() stores a new scene for this frame;
    /// cleared once this frame's own layer has been re-rendered. Not part
    /// of reset_composed_frame_state(), which clears geometry (parent,
    /// viewport, child layout) — content dirtiness is a separate axis.
    dirty: bool,
}

/// The content of a decoded video frame: CPU bytes (cross-platform) or a
/// GPU texture on the graphics device (macOS zero-copy).
#[derive(Clone)]
pub enum VideoFrameContent {
    /// RGBA8 pixel data, width * height * 4 bytes.
    Bytes(std::sync::Arc<[u8]>),
    /// Fake image data referencing a GPU texture on the graphics device
    /// (registered with the renderer's Vello via `override_image`); drawn
    /// as a plain image brush.
    #[cfg(target_os = "macos")]
    Texture(peniko::ImageData),
}

/// Carries the latest decoded video frame for a given pipeline, ready to paint.
#[derive(Clone)]
pub struct CompositorVideoFrame {
    pub video_paint_id: VideoPaintId,
    pub width: u32,
    pub height: u32,
    pub content: VideoFrameContent,
    /// Set when this frame is a fresh decode (a new frame arrived for the
    /// paint id); cleared once the video's layer has been re-rendered.
    /// Mirrors CachedFrame::dirty on the video axis.
    pub dirty: bool,
}

/// The committed scene of an offscreen canvas: the owning worker draws into
/// an anyrender scene and commits it; the compositor turns it into a canvas
/// layer on the owner document's next composition. A canvas always has a
/// backing store (initially blank), so — unlike a video — it never gates
/// composition; a fresh commit re-notes a composition change instead.
#[derive(Clone)]
pub struct CompositorCanvasFrame {
    pub canvas_id: CanvasId,
    pub width: u32,
    pub height: u32,
    pub scene: RecordedScene,
    /// Set when a fresh commit arrived; cleared once the canvas's layer has
    /// been re-rendered. Mirrors CachedFrame::dirty.
    pub dirty: bool,
}

/// The per-webview compositor: receives PaintFrames and VideoFrames,
/// composes them into a single final scene, and publishes the result plus
/// hit-testing info back to the user agent. Owns the webview's font
/// transport state (fonts registered from content PaintFrames, resolved
/// when recorded scenes are turned into render scenes).
#[derive(Default)]
pub struct Compositor {
    root_frame_id: Option<FrameId>,
    committed_frames: HashMap<FrameId, CachedFrame>,
    pending_frames: HashMap<FrameId, CachedFrame>,
    replace_root_on_next_paint: bool,
    resolved_tree_dirty: bool,
    /// Latest frame per video paint id.
    video_frames: HashMap<VideoPaintId, CompositorVideoFrame>,
    /// Latest committed scene per offscreen canvas id.
    canvas_frames: HashMap<CanvasId, CompositorCanvasFrame>,
    /// True when the latest top-level frame arrived but its composition is
    /// deferred until every embedded frame it references has arrived.
    composition_pending: bool,
    /// Accumulated across the frames of the current composition: whether any
    /// composed frame is animating, and which frames are.
    composing_animating: bool,
    composing_animating_frames: Vec<FrameId>,
    /// Font transport state for this webview: fonts registered from content
    /// PaintFrames, resolved when recorded scenes are turned into render
    /// scenes.
    font_receiver: FontTransportReceiver,
    /// Child frame ids whose navigable has been removed (the iframe was torn
    /// down): a deferred composition must not wait for a frame from these,
    /// since one will never arrive. Populated from UnregisterWebview for
    /// child navigables.
    removed_child_frames: HashSet<FrameId>,
}

impl Compositor {
    pub fn note_navigation_finalized(&mut self) {
        info!(
            "[render-pipe] Compositor navigation finalized reset root={:?} committed={} pending={} videos={}",
            self.root_frame_id.map(|id| id.0),
            self.committed_frames.len(),
            self.pending_frames.len(),
            self.video_frames.len(),
        );
        // A navigation finalized: the stored frames belong to the outgoing
        // document. Every frame this compositor holds (the top-level frame
        // plus all embedded child frames) and every video frame is dropped
        // so a deferred composition can never wait on a frame that will not
        // arrive. The next top-level frame starts a fresh pipeline:
        // replace_root_on_next_paint makes it the sole committed frame. The
        // webview and traversable are unchanged.
        self.pending_frames.clear();
        self.committed_frames.clear();
        self.video_frames.clear();
        // Canvas frames are NOT cleared here: a canvas is registered and its
        // first scene committed by the incoming document's script, which runs
        // before the navigation is finalized.  A stale canvas frame is keyed
        // by its own CanvasId (never reused) and is only referenced by a frame
        // whose embed site carries that id, so it is inert after its document
        // is gone.
        self.root_frame_id = None;
        self.replace_root_on_next_paint = true;
        self.resolved_tree_dirty = true;
        self.composition_pending = false;
    }

    /// Mark a child frame id as gone: its navigable was removed, so a
    /// deferred composition must not wait for a frame from it. Also drops
    /// any stored frame with that id.
    pub fn mark_child_frame_removed(&mut self, frame_id: FrameId) {
        info!(
            "[render-pipe] Compositor mark child frame removed id={}",
            frame_id.0
        );
        self.removed_child_frames.insert(frame_id);
        self.committed_frames.remove(&frame_id);
        self.pending_frames.remove(&frame_id);
        self.resolved_tree_dirty = true;
    }

    /// Insert a decoded video frame. Returns `true` when this is the first
    /// video frame arriving (transition from idle to animated), which the
    /// graphics process can use to trigger a one-time wakeup composition.
    pub fn update_video_frame(&mut self, frame: CompositorVideoFrame) -> bool {
        let was_idle = self.video_frames.is_empty();
        self.video_frames.insert(frame.video_paint_id, frame);
        was_idle
    }

    pub fn remove_video_frame(&mut self, paint_id: VideoPaintId) {
        self.video_frames.remove(&paint_id);
    }

    /// Store a committed offscreen canvas scene, replacing the previous one.
    /// Returns `true` when this is the first commit for the canvas id (the
    /// canvas layer has no surface yet).
    pub fn store_canvas_frame(&mut self, frame: CompositorCanvasFrame) -> bool {
        let was_absent = !self.canvas_frames.contains_key(&frame.canvas_id);
        self.canvas_frames.insert(frame.canvas_id, frame);
        was_absent
    }

    pub fn remove_canvas_frame(&mut self, canvas_id: CanvasId) {
        self.canvas_frames.remove(&canvas_id);
    }

    /// Clear the dirty flag for the layers that were actually re-rendered this
    /// cycle. Called by the event loop after `submit_layers` succeeds; a layer
    /// whose content no longer changed keeps its last surface on the next
    /// cycles, which is what lets clean layers skip Vello work.
    pub fn mark_layers_rendered(&mut self, rendered: &[CompositingLayerId]) {
        for layer_id in rendered {
            match layer_id {
                CompositingLayerId::Navigable(frame_id) => {
                    if let Some(frame) = self.committed_frames.get_mut(frame_id) {
                        frame.dirty = false;
                    }
                }
                CompositingLayerId::Video(paint_id) => {
                    if let Some(frame) = self.video_frames.get_mut(paint_id) {
                        frame.dirty = false;
                    }
                }
                CompositingLayerId::Canvas(canvas_id) => {
                    if let Some(frame) = self.canvas_frames.get_mut(canvas_id) {
                        frame.dirty = false;
                    }
                }
            }
        }
    }

    pub fn note_child_navigation_finalized(&mut self, frame_id: FrameId) {
        if Some(frame_id) == self.root_frame_id {
            self.note_navigation_finalized();
            return;
        }

        let mut stale_frame_ids = HashSet::new();
        let mut stack = HashSet::from([frame_id]);
        self.collect_scene_descendant_frames(frame_id, &mut stale_frame_ids, &mut stack);
        info!(
            "[render-pipe] Compositor child navigation finalized frame={} clearing={:?}",
            frame_id.0,
            stale_frame_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
        );
        for stale_frame_id in stale_frame_ids {
            self.committed_frames.remove(&stale_frame_id);
            self.pending_frames.remove(&stale_frame_id);
        }
        self.resolved_tree_dirty = true;
    }

    /// Store a content frame. Returns whether this frame's content is dirty
    /// (its scene or viewport changed since the last stored frame for the same
    /// id). The caller uses the dirty signal for a non-root (child) frame to
    /// re-note a rendering opportunity for the parent, so a cross-origin
    /// iframe whose content changed gets re-composed without waiting for an
    /// unrelated input event.
    #[must_use]
    pub fn store_frame(
        &mut self,
        frame_id: FrameId,
        viewport_width: u32,
        viewport_height: u32,
        composition: FrameCompositionMetadata,
        scene: RecordedScene,
        is_root_candidate: bool,
    ) -> bool {
        if input_debug_enabled() {
            let summary = scene.summary();
            trace!(
                "[input-debug][compositor] store_frame frame={} root_candidate={} viewport=({},{}) embed_sites={} commands={}",
                frame_id.0,
                is_root_candidate,
                viewport_width,
                viewport_height,
                composition.embed_sites.len(),
                summary.commands,
            );
        }

        // Decide whether this frame's content actually changed since the
        // last stored frame for the same id. A layer whose scene and
        // viewport are unchanged must keep its last surface (stay clean);
        // re-storing an identical scene (e.g. a static document that keeps
        // re-sending a PaintFrame because a video elsewhere drives the
        // render cycle) would otherwise re-rasterize it and re-present it
        // every cycle. Layer placement (transform/clip) is recomputed from
        // `composition` each compose walk, so a placement-only change does
        // not make the layer's own surface dirty.
        // Decide whether this frame's content actually changed since the
        // last stored frame for the same id. A layer whose scene and
        // viewport are unchanged must keep its last surface (stay clean);
        // re-storing an identical scene (e.g. a static document that keeps
        // re-sending a PaintFrame because a video elsewhere drives the
        // render cycle) would otherwise re-rasterize it and re-present it
        // every cycle. Layer placement (transform/clip) is recomputed from
        // `composition` each compose walk, so a placement-only change does
        // not make the layer's own surface dirty.
        //
        // Compare a digest of the serialized scene rather than the scene's
        // derived `PartialEq`: the recorded scene can contain a float `NaN`
        // (or a `Paint::Custom` whose Arc pointer differs across a
        // round-trip), which makes two byte-identical scenes compare
        // unequal and re-rasterize an unchanged layer every cycle.
        let scene_digest = {
            let bytes = serialize_scene_to_vec(&scene).unwrap_or_default();
            let mut h = 0xcbf29ce484222325u64; // FNV-1a offset basis
            for b in &bytes {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        };
        let scene_changed = self
            .committed_frames
            .get(&frame_id)
            .map(|existing| {
                existing.scene_digest != scene_digest
                    || existing.viewport_width != viewport_width
                    || existing.viewport_height != viewport_height
            })
            .unwrap_or(true);

        let frame = CachedFrame {
            viewport_width,
            viewport_height,
            parent_frame_id: None,
            resolved_viewport: None,
            child_frames: Vec::new(),
            composition,
            scene,
            scene_digest,
            animating: false,
            dirty: scene_changed,
        };

        if self.replace_root_on_next_paint {
            info!(
                "[render-pipe] Compositor store frame id={} root_candidate={} -> pending (replace next paint)",
                frame_id.0, is_root_candidate
            );
            self.pending_frames.insert(frame_id, frame);
            if is_root_candidate {
                self.root_frame_id = Some(frame_id);
                self.committed_frames = std::mem::take(&mut self.pending_frames);
                self.replace_root_on_next_paint = false;
                info!(
                    "[render-pipe] Compositor replace committed with pending root={} committed={:?}",
                    frame_id.0,
                    self.committed_frames
                        .keys()
                        .map(|id| id.0)
                        .collect::<Vec<_>>(),
                );
            }
            self.resolved_tree_dirty = true;
            return scene_changed;
        }

        // Always update root_frame_id when a root candidate arrives,
        // even if the frame_id differs from the current root. During
        // navigation, the content process creates a new document with a
        // new frame_id, and we need to point the compositor at the latest
        // root frame regardless of whether NavigationFinalized has
        // arrived yet.
        if is_root_candidate && self.root_frame_id != Some(frame_id) {
            info!(
                "[render-pipe] Compositor update root from {:?} to {}",
                self.root_frame_id, frame_id.0
            );
            self.root_frame_id = Some(frame_id);
        }

        self.committed_frames.insert(frame_id, frame);
        self.resolved_tree_dirty = true;
        scene_changed
    }

    /// Decode a content PaintFrame into a recorded scene, registering its
    /// fonts in this compositor's font transport state.
    pub fn decode_frame(
        &mut self,
        frame: PaintFrame,
        shmem_regions: &HashMap<usize, IpcSharedRegion>,
    ) -> Result<RecordedScene, String> {
        frame.into_recorded_scene(&mut self.font_receiver, shmem_regions)
    }

    /// The latest top-level frame arrived; its composition must wait for every
    /// embedded frame it references to arrive before it can be composed.
    pub fn mark_composition_pending(&mut self) {
        self.composition_pending = true;
    }

    /// Record the animating flag of a stored frame: its document contains
    /// animated content (video, CSS animations). The composed scene
    /// aggregates it so the UA keeps noting rendering opportunities while
    /// any composed frame animates.
    pub fn note_frame_animating(&mut self, frame_id: FrameId, animating: bool) {
        if let Some(frame) = self.committed_frames.get_mut(&frame_id) {
            frame.animating = animating;
        }
        if let Some(frame) = self.pending_frames.get_mut(&frame_id) {
            frame.animating = animating;
        }
    }

    pub fn has_pending_composition(&self) -> bool {
        self.composition_pending
    }

    pub fn top_level_frame_id(&self) -> Option<FrameId> {
        self.root_frame_id
    }

    /// The frame ids currently committed (top-level plus embedded children),
    /// for diagnostics.
    pub fn committed_frame_ids(&self) -> Vec<FrameId> {
        let mut ids = self.committed_frames.keys().copied().collect::<Vec<_>>();
        ids.sort_by_key(|id| id.0);
        ids
    }

    /// Whether the latest top-level frame can be composed now: video frames
    /// must be present or not expected (no live pipeline for the paint id,
    /// or the pipeline ended/failed).  Child frames do not gate composition:
    /// a child whose navigation is still in flight (or failed) may never
    /// produce a frame, and waiting for it would block the parent's
    /// composition forever (the child's rendering opportunities only arrive
    /// via the parent's composed scene, so the deadlock is permanent).  The
    /// parent composes without a late child frame; the child's own render
    /// (from the published child viewport) propagates a top-level re-render
    /// that includes the frame when it arrives.
    pub fn composition_ready(&self, expected_videos: &HashSet<VideoPaintId>) -> bool {
        let Some(top_level_frame_id) = self.root_frame_id else {
            return false;
        };
        let Some(top_level_frame) = self.committed_frames.get(&top_level_frame_id) else {
            return false;
        };
        for site in &top_level_frame.composition.embed_sites {
            match site {
                EmbedSite::Frame(_iframe_site) => {}
                EmbedSite::Canvas(_) => {}
                EmbedSite::Video(video_data) => {
                    if expected_videos.contains(&video_data.paint_id)
                        && !self.video_frames.contains_key(&video_data.paint_id)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// The embedded frames the latest top-level frame still waits for:
    /// child frame ids absent from the committed set and video paint ids
    /// that are expected but have no stored frame. Used by the caller to
    /// log exactly what a deferred composition is missing.
    pub fn missing_embedded_frames(
        &self,
        expected_videos: &HashSet<VideoPaintId>,
    ) -> (Vec<FrameId>, Vec<VideoPaintId>) {
        let Some(top_level_frame_id) = self.root_frame_id else {
            return (Vec::new(), Vec::new());
        };
        let Some(top_level_frame) = self.committed_frames.get(&top_level_frame_id) else {
            return (Vec::new(), Vec::new());
        };
        let mut missing_child_ids = Vec::new();
        let mut missing_video_ids = Vec::new();
        for site in &top_level_frame.composition.embed_sites {
            match site {
                EmbedSite::Frame(iframe_site) => {
                    if !self
                        .committed_frames
                        .contains_key(&iframe_site.child_frame_id)
                        && !self
                            .removed_child_frames
                            .contains(&iframe_site.child_frame_id)
                    {
                        missing_child_ids.push(iframe_site.child_frame_id);
                    }
                }
                EmbedSite::Canvas(_) => {}
                EmbedSite::Video(video_data) => {
                    if expected_videos.contains(&video_data.paint_id)
                        && !self.video_frames.contains_key(&video_data.paint_id)
                    {
                        missing_video_ids.push(video_data.paint_id);
                    }
                }
            }
        }
        (missing_child_ids, missing_video_ids)
    }

    /// Compose the final scene for this compositor and return it with
    /// hit-testing info. Caller is responsible for resetting state.
    pub fn compose_scene(
        &mut self,
        webview_id: ipc_messages::content::WebviewId,
    ) -> Option<ComposedScene> {
        let root_frame_id = self.root_frame_id?;
        // The pending composition completes now; the next top-level frame
        // arrival re-marks it pending.
        self.composition_pending = false;
        self.composing_animating = false;
        self.composing_animating_frames.clear();
        self.reset_composed_frame_state();
        self.prepare_root_frame(root_frame_id)?;
        let root_placement = self.root_layer_placement(root_frame_id)?;
        let mut stack = HashSet::from([root_frame_id]);
        let mut layers = Vec::new();
        self.compose_layers(
            root_frame_id,
            &mut stack,
            Affine::IDENTITY,
            root_placement,
            &mut layers,
        )?;
        self.resolved_tree_dirty = false;

        let frame_hit_info = self.build_frame_hit_info(webview_id);

        Some(ComposedScene {
            webview_id,
            layers,
            frame_hit_info,
            child_viewports: HashMap::new(),
            child_frame_to_webview: HashMap::new(),
            animating: self.composing_animating,
            animating_frame_ids: std::mem::take(&mut self.composing_animating_frames),
        })
    }

    fn build_frame_hit_info(
        &self,
        webview_id: ipc_messages::content::WebviewId,
    ) -> Vec<FrameHitInfo> {
        let mut hit_info = Vec::new();
        let Some(root_frame_id) = self.root_frame_id else {
            return hit_info;
        };
        // Root frame: clip is its own viewport in root space.
        if let Some(frame) = self.committed_frames.get(&root_frame_id) {
            let root_clip = [
                0.0,
                0.0,
                f64::from(frame.viewport_width),
                f64::from(frame.viewport_height),
            ];
            let child_ids: Vec<FrameId> = frame
                .child_frames
                .iter()
                .map(|c| c.child_frame_id)
                .collect();
            hit_info.push(FrameHitInfo {
                frame_id: root_frame_id,
                webview_id,
                parent_frame_id: None,
                viewport_width: frame.viewport_width,
                viewport_height: frame.viewport_height,
                root_clip_bounds: root_clip,
                child_to_parent_transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                child_frame_ids: child_ids,
            });
        }

        // Children: each child's clip in root space comes from the parent's
        // NavigableContainerLayout.root_clip_bounds (the iframe's visible clip
        // rect in root coordinates), NOT from the child's own viewport dimensions.
        self.collect_child_hit_info(root_frame_id, webview_id, &mut hit_info);
        hit_info
    }

    /// Recursively collect hit-testing info for child frames only.
    /// Each child's root_clip_bounds is taken directly from the parent's
    /// NavigableContainerLayout, preserving the iframe's visible clip area
    /// (which may be smaller than the child's full viewport).
    fn collect_child_hit_info(
        &self,
        parent_frame_id: FrameId,
        webview_id: ipc_messages::content::WebviewId,
        hit_info: &mut Vec<FrameHitInfo>,
    ) {
        let Some(parent_frame) = self.committed_frames.get(&parent_frame_id) else {
            return;
        };
        for child_layout in &parent_frame.child_frames {
            let child_frame_id = child_layout.child_frame_id;
            let Some(child_frame) = self.committed_frames.get(&child_frame_id) else {
                continue;
            };

            // Use the parent's layout clip rect for this child — this is the
            // iframe's visible area in root space, matching hit_test_frame on main.
            let root_clip = [
                child_layout.root_clip_bounds.x0,
                child_layout.root_clip_bounds.y0,
                child_layout.root_clip_bounds.x1,
                child_layout.root_clip_bounds.y1,
            ];

            let child_ids: Vec<FrameId> = child_frame
                .child_frames
                .iter()
                .map(|c| c.child_frame_id)
                .collect();

            let parent_transform = if let Some(parent_id) = child_frame.parent_frame_id {
                if let Some(parent) = self.committed_frames.get(&parent_id) {
                    parent
                        .child_frames
                        .iter()
                        .find(|c| c.child_frame_id == child_frame_id)
                        .map(|layout| {
                            let t = layout.child_local_from_parent.as_coeffs();
                            [t[0], t[1], t[2], t[3], t[4], t[5]]
                        })
                        .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
                } else {
                    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
                }
            } else {
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
            };

            hit_info.push(FrameHitInfo {
                frame_id: child_frame_id,
                webview_id,
                parent_frame_id: child_frame.parent_frame_id,
                viewport_width: child_frame.viewport_width,
                viewport_height: child_frame.viewport_height,
                root_clip_bounds: root_clip,
                child_to_parent_transform: parent_transform,
                child_frame_ids: child_ids,
            });

            // Recurse into nested children.
            self.collect_child_hit_info(child_frame_id, webview_id, hit_info);
        }
    }

    pub fn visible_frame_viewports(&mut self) -> Vec<super::VisibleFrameViewport> {
        let refresh_needed = self.resolved_tree_dirty
            || self
                .root_frame_id
                .and_then(|frame_id| self.committed_frames.get(&frame_id))
                .and_then(|frame| frame.resolved_viewport.as_ref())
                .is_none();
        if refresh_needed && let Some(root_frame_id) = self.root_frame_id {
            self.reset_composed_frame_state();
            if self.prepare_root_frame(root_frame_id).is_some()
                && let Some(root_placement) = self.root_layer_placement(root_frame_id)
            {
                let mut stack = HashSet::from([root_frame_id]);
                let mut scratch_layers = Vec::new();
                if self
                    .compose_layers(
                        root_frame_id,
                        &mut stack,
                        Affine::IDENTITY,
                        root_placement,
                        &mut scratch_layers,
                    )
                    .is_none()
                {
                    error!(
                        "[compositor] refresh compose failed for root frame {}",
                        root_frame_id.0
                    );
                }
            }
            self.resolved_tree_dirty = false;
        }

        let Some(root_frame_id) = self.root_frame_id else {
            return Vec::new();
        };

        let mut viewports = Vec::new();
        self.collect_visible_frame_viewports(root_frame_id, &mut viewports);
        viewports
    }

    fn compose_layers(
        &mut self,
        frame_id: FrameId,
        stack: &mut HashSet<FrameId>,
        frame_local_to_root: Affine,
        placement: LayerPlacement,
        layers: &mut Vec<LayerUpdate>,
    ) -> Option<()> {
        if input_debug_enabled() {
            trace!("[input-debug][compositor] composing frame {}", frame_id.0);
        }

        let parent_viewport = self
            .committed_frames
            .get(&frame_id)?
            .resolved_viewport
            .clone()?;

        let frame = self.committed_frames.get(&frame_id)?;
        if input_debug_enabled() {
            trace!(
                "[input-debug][compositor] frame {} dirty={}",
                frame_id.0, frame.dirty
            );
        }
        let embed_sites = frame.composition.embed_sites.clone();
        let frame_dirty = frame.dirty;
        let frame_width = frame.viewport_width;
        let frame_height = frame.viewport_height;
        let frame_parent = frame.parent_frame_id;

        // Aggregate the animating flag across the composed frames: the
        // composed scene reports it so the UA keeps noting rendering
        // opportunities while any composing frame animates.
        if self
            .committed_frames
            .get(&frame_id)
            .map(|frame| frame.animating)
            .unwrap_or(false)
        {
            self.composing_animating = true;
            self.composing_animating_frames.push(frame_id);
        }

        // Decode this frame's own scene only when dirty; a clean layer keeps
        // its last surface and skips the Vello rasterization entirely.
        let render = if frame_dirty {
            let frame = self.committed_frames.get(&frame_id)?;
            Some(frame.scene.clone().into_scene(&self.font_receiver))
        } else {
            None
        };

        // This frame is its own compositing layer. The parent-relative
        // placement was computed by the parent (or is the root's own
        // viewport) and passed in; it positions this layer's content within
        // the layer tree. Same-origin iframes are baked into their parent's
        // scene and never reach here with their own layer.
        layers.push(LayerUpdate {
            layer_id: CompositingLayerId::Navigable(frame_id),
            parent: frame_parent.map(CompositingLayerId::Navigable),
            transform: placement.transform,
            clip_bounds: placement.clip_bounds,
            corner_radius: placement.corner_radius,
            z_order: placement.z_order,
            background: placement.background,
            width: frame_width,
            height: frame_height,
            render,
        });

        let bg_map: HashMap<_, _> = embed_sites
            .iter()
            .filter_map(|site| match site {
                EmbedSite::Frame(f) => Some((f.embed_site_id, f.background_policy)),
                EmbedSite::Video(_) => None,
                EmbedSite::Canvas(_) => None,
            })
            .collect();

        let mut paint_items: Vec<(i32, u32, &EmbedSite)> = embed_sites
            .iter()
            .map(|site| (site.z_index(), site.paint_order(), site))
            .collect();
        paint_items.sort_by_key(|(z, p, _)| (*z, *p));

        for (z, paint_order, site) in paint_items {
            match site {
                EmbedSite::Frame(iframe_site) => {
                    let child_frame_id = iframe_site.child_frame_id;
                    let Some((child_local_to_root, child_layout)) = self.record_child_frame_layout(
                        frame_id,
                        &parent_viewport,
                        frame_local_to_root,
                        iframe_site,
                    ) else {
                        continue;
                    };

                    if !stack.insert(child_frame_id) {
                        continue;
                    }

                    let clip = Self::embed_local_clip(iframe_site);
                    let transform = Affine::new(iframe_site.layout.transform);
                    let child_transform = self
                        .child_scene_transform(&clip, child_frame_id)
                        .map(|scene_transform| transform * scene_transform)
                        .unwrap_or(transform);
                    let child_placement = LayerPlacement {
                        transform: child_transform,
                        clip_bounds: child_layout.clip_bounds,
                        corner_radius: 0.0,
                        z_order: (z, paint_order),
                        background: bg_map.get(&iframe_site.embed_site_id).copied(),
                    };

                    self.compose_layers(
                        child_frame_id,
                        stack,
                        child_local_to_root,
                        child_placement,
                        layers,
                    );
                    stack.remove(&child_frame_id);
                }
                EmbedSite::Video(video_data) => {
                    let Some(video_frame) = self.video_frames.get(&video_data.paint_id) else {
                        if input_debug_enabled() {
                            trace!(
                                "[input-debug][compositor] video paint_id={:?} no frame yet",
                                video_data.paint_id
                            );
                        }
                        continue;
                    };
                    if input_debug_enabled() {
                        trace!(
                            "[input-debug][compositor] video paint_id={:?} dirty={}",
                            video_data.paint_id, video_frame.dirty
                        );
                    }
                    let transform = Affine::new(video_data.layout.transform);

                    let tx = transform.as_coeffs()[4];
                    let ty = transform.as_coeffs()[5];
                    let clip_rect = Rect::new(
                        video_data.layout.clip_bounds[0] - tx,
                        video_data.layout.clip_bounds[1] - ty,
                        video_data.layout.clip_bounds[2] - tx,
                        video_data.layout.clip_bounds[3] - ty,
                    );
                    let local_w = clip_rect.width();
                    let local_h = clip_rect.height();
                    let scale_x = if video_frame.width > 0 {
                        local_w / video_frame.width as f64
                    } else {
                        1.0
                    };
                    let scale_y = if video_frame.height > 0 {
                        local_h / video_frame.height as f64
                    } else {
                        1.0
                    };
                    let video_transform = Affine::new([scale_x, 0.0, 0.0, scale_y, tx, ty]);

                    // The video embed site is its own layer: a one-node scene
                    // drawing the decoded frame at identity, placed by
                    // `video_transform` (which scales the frame to the clip
                    // rect and positions it at the video's origin).
                    let render = if video_frame.dirty {
                        let mut video_scene = RenderScene::new();
                        match &video_frame.content {
                            VideoFrameContent::Bytes(pixel_bytes) => {
                                let image_data = ImageData {
                                    data: peniko::Blob::from(pixel_bytes.to_vec()),
                                    format: ImageFormat::Rgba8,
                                    alpha_type: ImageAlphaType::Alpha,
                                    width: video_frame.width,
                                    height: video_frame.height,
                                };
                                video_scene
                                    .draw_image(ImageBrushRef::from(&image_data), Affine::IDENTITY);
                            }
                            #[cfg(target_os = "macos")]
                            VideoFrameContent::Texture(image_data) => {
                                // The texture is sampled by the graphics
                                // Vello renderer via its override_image
                                // registration; the scene just references
                                // the fake image data.
                                video_scene
                                    .draw_image(ImageBrushRef::from(image_data), Affine::IDENTITY);
                            }
                        }
                        Some(video_scene)
                    } else {
                        None
                    };

                    layers.push(LayerUpdate {
                        layer_id: CompositingLayerId::Video(video_data.paint_id),
                        parent: Some(CompositingLayerId::Navigable(frame_id)),
                        transform: video_transform,
                        clip_bounds: Rect::new(
                            video_data.layout.clip_bounds[0],
                            video_data.layout.clip_bounds[1],
                            video_data.layout.clip_bounds[2],
                            video_data.layout.clip_bounds[3],
                        ),
                        corner_radius: video_data.clip_radius,
                        z_order: (z, paint_order),
                        background: None,
                        width: video_frame.width,
                        height: video_frame.height,
                        render,
                    });
                }
                EmbedSite::Canvas(canvas_site) => {
                    let Some(canvas_frame) = self.canvas_frames.get(&canvas_site.canvas_id) else {
                        if input_debug_enabled() {
                            trace!(
                                "[input-debug][compositor] canvas {:?} no commit yet",
                                canvas_site.canvas_id
                            );
                        }
                        continue;
                    };
                    if input_debug_enabled() {
                        trace!(
                            "[input-debug][compositor] canvas {:?} dirty={}",
                            canvas_site.canvas_id, canvas_frame.dirty
                        );
                    }
                    let transform = Affine::new(canvas_site.layout.transform);
                    let tx = transform.as_coeffs()[4];
                    let ty = transform.as_coeffs()[5];
                    let clip_rect = Rect::new(
                        canvas_site.layout.clip_bounds[0] - tx,
                        canvas_site.layout.clip_bounds[1] - ty,
                        canvas_site.layout.clip_bounds[2] - tx,
                        canvas_site.layout.clip_bounds[3] - ty,
                    );
                    let local_w = clip_rect.width();
                    let local_h = clip_rect.height();
                    let scale_x = if canvas_frame.width > 0 {
                        local_w / canvas_frame.width as f64
                    } else {
                        1.0
                    };
                    let scale_y = if canvas_frame.height > 0 {
                        local_h / canvas_frame.height as f64
                    } else {
                        1.0
                    };
                    let canvas_transform = Affine::new([scale_x, 0.0, 0.0, scale_y, tx, ty]);

                    // The canvas embed site is its own layer: the worker's
                    // committed anyrender scene drawn at identity, placed by
                    // `canvas_transform` (scaled to the clip rect).
                    let render = if canvas_frame.dirty {
                        Some(canvas_frame.scene.clone().into_scene(&self.font_receiver))
                    } else {
                        None
                    };

                    layers.push(LayerUpdate {
                        layer_id: CompositingLayerId::Canvas(canvas_site.canvas_id),
                        parent: Some(CompositingLayerId::Navigable(frame_id)),
                        transform: canvas_transform,
                        clip_bounds: Rect::new(
                            canvas_site.layout.clip_bounds[0],
                            canvas_site.layout.clip_bounds[1],
                            canvas_site.layout.clip_bounds[2],
                            canvas_site.layout.clip_bounds[3],
                        ),
                        corner_radius: 0.0,
                        z_order: (z, paint_order),
                        background: None,
                        width: canvas_frame.width,
                        height: canvas_frame.height,
                        render,
                    });
                }
            }
        }

        Some(())
    }

    fn embed_local_clip(iframe_site: &IframeEmbedSite) -> Rect {
        let transform = Affine::new(iframe_site.layout.transform);
        let translation_x = transform.as_coeffs()[4];
        let translation_y = transform.as_coeffs()[5];
        Rect::new(
            iframe_site.layout.clip_bounds[0] - translation_x,
            iframe_site.layout.clip_bounds[1] - translation_y,
            iframe_site.layout.clip_bounds[2] - translation_x,
            iframe_site.layout.clip_bounds[3] - translation_y,
        )
    }

    fn reset_composed_frame_state(&mut self) {
        for frame in self.committed_frames.values_mut() {
            frame.parent_frame_id = None;
            frame.resolved_viewport = None;
            frame.child_frames.clear();
        }
    }

    fn prepare_root_frame(&mut self, frame_id: FrameId) -> Option<()> {
        let resolved_viewport = self.frame_viewport(frame_id)?;
        let frame = self.committed_frames.get_mut(&frame_id)?;
        frame.parent_frame_id = None;
        frame.resolved_viewport = Some(resolved_viewport);
        frame.child_frames.clear();
        Some(())
    }

    fn frame_viewport(&self, frame_id: FrameId) -> Option<ResolvedViewport> {
        let frame = self.committed_frames.get(&frame_id)?;
        Some(ResolvedViewport::new(
            f64::from(frame.viewport_width),
            f64::from(frame.viewport_height),
        ))
    }

    /// The root frame's own layer placement: identity transform, padded to
    /// its own viewport as the clip, no corner radius, no background.
    fn root_layer_placement(&self, root_frame_id: FrameId) -> Option<LayerPlacement> {
        let frame = self.committed_frames.get(&root_frame_id)?;
        Some(LayerPlacement {
            transform: Affine::IDENTITY,
            clip_bounds: Rect::new(
                0.0,
                0.0,
                f64::from(frame.viewport_width),
                f64::from(frame.viewport_height),
            ),
            corner_radius: 0.0,
            z_order: (0, 0),
            background: None,
        })
    }

    fn record_child_frame_layout(
        &mut self,
        parent_frame_id: FrameId,
        parent_viewport: &ResolvedViewport,
        parent_local_to_root: Affine,
        iframe_site: &IframeEmbedSite,
    ) -> Option<(Affine, NavigableContainerLayout)> {
        let Some(layout) = self.navigable_container_layout(parent_local_to_root, iframe_site)
        else {
            if input_debug_enabled() {
                trace!(
                    "[input-debug][compositor] parent={} child={} record=skip reason=no-layout",
                    parent_frame_id.0, iframe_site.child_frame_id.0,
                );
            }
            return None;
        };

        if !parent_viewport.intersects_local_rect(layout.clip_bounds) {
            if input_debug_enabled() {
                trace!(
                    "[input-debug][compositor] parent={} child={} record=skip visible=false clip=({:.1},{:.1})-({:.1},{:.1}) parent_viewport=({:.1},{:.1})",
                    parent_frame_id.0,
                    iframe_site.child_frame_id.0,
                    layout.clip_bounds.x0,
                    layout.clip_bounds.y0,
                    layout.clip_bounds.x1,
                    layout.clip_bounds.y1,
                    parent_viewport.width,
                    parent_viewport.height,
                );
            }
            return None;
        };

        let child_local_to_root = parent_local_to_root * layout.child_local_from_parent.inverse();

        if input_debug_enabled() {
            trace!(
                "[input-debug][compositor] parent={} child={} record=ok clip=({:.1},{:.1})-({:.1},{:.1})",
                parent_frame_id.0,
                iframe_site.child_frame_id.0,
                layout.clip_bounds.x0,
                layout.clip_bounds.y0,
                layout.clip_bounds.x1,
                layout.clip_bounds.y1,
            );
        }

        if let Some(frame) = self.committed_frames.get_mut(&parent_frame_id) {
            frame.child_frames.push(layout.clone());
        }

        if let Some(resolved_viewport) = self.frame_viewport(iframe_site.child_frame_id)
            && let Some(child_frame) = self.committed_frames.get_mut(&iframe_site.child_frame_id)
        {
            child_frame.parent_frame_id = Some(parent_frame_id);
            child_frame.resolved_viewport = Some(resolved_viewport);
        }

        Some((child_local_to_root, layout))
    }

    fn navigable_container_layout(
        &self,
        parent_local_to_root: Affine,
        iframe_site: &IframeEmbedSite,
    ) -> Option<NavigableContainerLayout> {
        let child_frame_id = iframe_site.child_frame_id;
        let transform = Affine::new(iframe_site.layout.transform);
        let clip = Self::embed_local_clip(iframe_site);
        let child_scene_transform = self
            .child_scene_transform(&clip, child_frame_id)
            .unwrap_or(Affine::IDENTITY);
        let child_local_from_parent = (transform * child_scene_transform).inverse();
        let mut transformed_clip = clip.to_path(0.1);
        transformed_clip.apply_affine(parent_local_to_root * transform);
        let root_clip_bounds = transformed_clip.bounding_box();

        let mut local_clip = clip.to_path(0.1);
        local_clip.apply_affine(transform);
        let clip_bounds = local_clip.bounding_box();
        Some(NavigableContainerLayout {
            child_frame_id,
            clip_bounds,
            root_clip_bounds,
            child_local_from_parent,
        })
    }

    fn collect_visible_frame_viewports(
        &self,
        frame_id: FrameId,
        viewports: &mut Vec<super::VisibleFrameViewport>,
    ) {
        let Some(frame) = self.committed_frames.get(&frame_id) else {
            return;
        };

        for child in &frame.child_frames {
            let viewport_width = child.root_clip_bounds.width().ceil().max(1.0) as u32;
            let viewport_height = child.root_clip_bounds.height().ceil().max(1.0) as u32;

            viewports.push(super::VisibleFrameViewport {
                frame_id: child.child_frame_id,
                offset_x: child.root_clip_bounds.x0 as f32,
                offset_y: child.root_clip_bounds.y0 as f32,
                width: viewport_width,
                height: viewport_height,
            });
            self.collect_visible_frame_viewports(child.child_frame_id, viewports);
        }
    }

    fn collect_scene_descendant_frames(
        &self,
        frame_id: FrameId,
        frames: &mut HashSet<FrameId>,
        stack: &mut HashSet<FrameId>,
    ) {
        if !frames.insert(frame_id) {
            return;
        }

        let Some(frame) = self.committed_frames.get(&frame_id) else {
            return;
        };

        let child_frame_ids = frame
            .composition
            .embed_sites
            .iter()
            .filter_map(|site| match site {
                EmbedSite::Frame(f) => Some(f.child_frame_id),
                EmbedSite::Video(_) => None,
                EmbedSite::Canvas(_) => None,
            })
            .collect::<Vec<_>>();
        for child_frame_id in child_frame_ids {
            if !stack.insert(child_frame_id) {
                continue;
            }
            self.collect_scene_descendant_frames(child_frame_id, frames, stack);
            stack.remove(&child_frame_id);
        }
    }

    fn child_scene_transform(&self, clip: &impl Shape, child_frame_id: FrameId) -> Option<Affine> {
        let child_frame = self.committed_frames.get(&child_frame_id)?;
        if child_frame.viewport_width == 0 || child_frame.viewport_height == 0 {
            return None;
        }

        let clip_bounds = clip.bounding_box();
        let scale_x = clip_bounds.width() / f64::from(child_frame.viewport_width);
        let scale_y = clip_bounds.height() / f64::from(child_frame.viewport_height);
        Some(Affine::new([scale_x, 0.0, 0.0, scale_y, 0.0, 0.0]))
    }
}
