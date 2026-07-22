//! Per-webview compositor — receives PaintFrames and VideoFrames, composes
//! them into a single final scene, and publishes the result plus hit-testing
//! info back to the user agent.

use anyrender::{PaintScene, Scene as RenderScene};
use ipc_messages::content::{
    EmbedBackgroundPolicy, EmbedSite, FontTransportReceiver, FrameCompositionMetadata, FrameId,
    IframeEmbedSite, RecordedScene,
};
use ipc_messages::graphics::FrameHitInfo;

use crate::ComposedScene;
use ipc_messages::media::VideoPaintId;
use kurbo::{Affine, Point, Rect, RoundedRect, Shape};
use log::trace;
use peniko::{Color, Fill, ImageAlphaType, ImageBrushRef, ImageData, ImageFormat};
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

    fn contains_local_point(&self, point: Point) -> bool {
        point.x >= 0.0 && point.y >= 0.0 && point.x < self.width && point.y < self.height
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

#[derive(Clone)]
struct CachedFrame {
    viewport_width: u32,
    viewport_height: u32,
    parent_frame_id: Option<FrameId>,
    resolved_viewport: Option<ResolvedViewport>,
    child_frames: Vec<NavigableContainerLayout>,
    composition: FrameCompositionMetadata,
    scene: RecordedScene,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitTestResult {
    pub frame_id: FrameId,
    pub local_x: f32,
    pub local_y: f32,
    pub is_child_frame: bool,
    pub has_child_frames: bool,
}

/// Carries the latest decoded video frame for a given pipeline, ready to paint.
#[derive(Clone)]
pub struct CompositorVideoFrame {
    pub video_paint_id: VideoPaintId,
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixel data, width * height * 4 bytes.
    pub data: std::sync::Arc<[u8]>,
}

#[derive(Clone, Default)]
pub struct Compositor {
    root_frame_id: Option<FrameId>,
    committed_frames: HashMap<FrameId, CachedFrame>,
    pending_frames: HashMap<FrameId, CachedFrame>,
    replace_root_on_next_paint: bool,
    resolved_tree_dirty: bool,
    /// Latest frame per video paint id.
    video_frames: HashMap<VideoPaintId, CompositorVideoFrame>,
}

impl Compositor {
    pub fn note_navigation_finalized(&mut self) {
        self.pending_frames.clear();
        self.replace_root_on_next_paint = true;
        self.resolved_tree_dirty = true;
    }

    pub fn update_video_frame(&mut self, frame: CompositorVideoFrame) {
        self.video_frames.insert(frame.video_paint_id, frame);
    }

    pub fn remove_video_frame(&mut self, paint_id: VideoPaintId) {
        self.video_frames.remove(&paint_id);
    }

    pub fn note_child_navigation_finalized(&mut self, frame_id: FrameId) {
        if Some(frame_id) == self.root_frame_id {
            self.note_navigation_finalized();
            return;
        }

        let mut stale_frame_ids = HashSet::new();
        let mut stack = HashSet::from([frame_id]);
        self.collect_scene_descendant_frames(frame_id, &mut stale_frame_ids, &mut stack);
        for stale_frame_id in stale_frame_ids {
            self.committed_frames.remove(&stale_frame_id);
            self.pending_frames.remove(&stale_frame_id);
        }
        self.resolved_tree_dirty = true;
    }

    pub fn store_frame(
        &mut self,
        frame_id: FrameId,
        viewport_width: u32,
        viewport_height: u32,
        composition: FrameCompositionMetadata,
        scene: RecordedScene,
        is_root_candidate: bool,
    ) {
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

        let frame = CachedFrame {
            viewport_width,
            viewport_height,
            parent_frame_id: None,
            resolved_viewport: None,
            child_frames: Vec::new(),
            composition,
            scene,
        };

        if self.replace_root_on_next_paint {
            self.pending_frames.insert(frame_id, frame);
            if is_root_candidate {
                self.root_frame_id = Some(frame_id);
                self.committed_frames = std::mem::take(&mut self.pending_frames);
                self.replace_root_on_next_paint = false;
            }
            self.resolved_tree_dirty = true;
            return;
        }

        if self.root_frame_id.is_none() && is_root_candidate {
            self.root_frame_id = Some(frame_id);
        }

        self.committed_frames.insert(frame_id, frame);
        self.resolved_tree_dirty = true;
    }

    pub fn committed_root_frame_id(&self) -> Option<FrameId> {
        self.root_frame_id
    }

    /// Compose the final scene for this compositor and return it with
    /// hit-testing info. Caller is responsible for resetting state.
    pub fn compose_scene(
        &mut self,
        font_receiver: &FontTransportReceiver,
        webview_id: ipc_messages::content::WebviewId,
    ) -> Option<ComposedScene> {
        let root_frame_id = self.root_frame_id?;
        self.reset_composed_frame_state();
        self.prepare_root_frame(root_frame_id)?;
        let mut stack = HashSet::from([root_frame_id]);
        let scene = self.compose_frame(root_frame_id, font_receiver, &mut stack, Affine::IDENTITY);
        self.resolved_tree_dirty = false;

        let scene = scene?;
        let frame_hit_info = self.build_frame_hit_info(webview_id);

        Some(ComposedScene {
            webview_id,
            scene,
            frame_hit_info,
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
        self.collect_frame_hit_info(root_frame_id, webview_id, &mut hit_info);
        hit_info
    }

    fn collect_frame_hit_info(
        &self,
        frame_id: FrameId,
        webview_id: ipc_messages::content::WebviewId,
        hit_info: &mut Vec<FrameHitInfo>,
    ) {
        let Some(frame) = self.committed_frames.get(&frame_id) else {
            return;
        };

        let child_ids: Vec<FrameId> = frame
            .child_frames
            .iter()
            .map(|c| c.child_frame_id)
            .collect();
        // Use the first child's root clip bounds as the clip for this frame
        // (the frame's own clip is its viewport, which covers the full content area).
        let root_clip = frame
            .child_frames
            .first()
            .map(|c| {
                [
                    c.root_clip_bounds.x0,
                    c.root_clip_bounds.y0,
                    c.root_clip_bounds.x1,
                    c.root_clip_bounds.y1,
                ]
            })
            .unwrap_or([
                0.0,
                0.0,
                f64::from(frame.viewport_width),
                f64::from(frame.viewport_height),
            ]);

        // Compute child_to_parent_transform from the inverse of each child's local transform.
        // For the frame itself, the transform from its local space to parent is identity
        // unless it has a parent frame with a recorded layout.
        let parent_transform = if let Some(parent_id) = frame.parent_frame_id {
            if let Some(parent_frame) = self.committed_frames.get(&parent_id) {
                parent_frame
                    .child_frames
                    .iter()
                    .find(|c| c.child_frame_id == frame_id)
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
            frame_id,
            webview_id,
            parent_frame_id: frame.parent_frame_id,
            viewport_width: frame.viewport_width,
            viewport_height: frame.viewport_height,
            root_clip_bounds: root_clip,
            child_to_parent_transform: parent_transform,
            child_frame_ids: child_ids,
        });

        for child in &frame.child_frames {
            self.collect_frame_hit_info(child.child_frame_id, webview_id, hit_info);
        }
    }

    pub fn visible_frame_viewports(
        &mut self,
        font_receiver: &FontTransportReceiver,
    ) -> Vec<super::VisibleFrameViewport> {
        let refresh_needed = self.resolved_tree_dirty
            || self
                .root_frame_id
                .and_then(|frame_id| self.committed_frames.get(&frame_id))
                .and_then(|frame| frame.resolved_viewport.as_ref())
                .is_none();
        if refresh_needed {
            if let Some(root_frame_id) = self.root_frame_id {
                self.reset_composed_frame_state();
                if self.prepare_root_frame(root_frame_id).is_some() {
                    let mut stack = HashSet::from([root_frame_id]);
                    let _ = self.compose_frame(
                        root_frame_id,
                        font_receiver,
                        &mut stack,
                        Affine::IDENTITY,
                    );
                }
                self.resolved_tree_dirty = false;
            }
        }

        let Some(root_frame_id) = self.root_frame_id else {
            return Vec::new();
        };

        let mut viewports = Vec::new();
        self.collect_visible_frame_viewports(root_frame_id, &mut viewports);
        viewports
    }

    pub fn hit_test(
        &mut self,
        x: f64,
        y: f64,
        font_receiver: &FontTransportReceiver,
    ) -> Option<HitTestResult> {
        let refresh_needed = self.resolved_tree_dirty
            || self
                .root_frame_id
                .and_then(|frame_id| self.committed_frames.get(&frame_id))
                .and_then(|frame| frame.resolved_viewport.as_ref())
                .is_none();
        if input_debug_enabled() {
            trace!(
                "[input-debug][compositor] hit_test client=({x:.1},{y:.1}) refresh_needed={refresh_needed}"
            );
        }
        if refresh_needed {
            let root_frame_id = self.root_frame_id?;
            self.reset_composed_frame_state();
            self.prepare_root_frame(root_frame_id)?;
            let mut stack = HashSet::from([root_frame_id]);
            let _ = self.compose_frame(root_frame_id, font_receiver, &mut stack, Affine::IDENTITY);
            self.resolved_tree_dirty = false;
        }

        let root_frame_id = self.root_frame_id?;
        if self
            .committed_frames
            .get(&root_frame_id)
            .and_then(|frame| frame.resolved_viewport.as_ref())
            .is_none()
        {
            self.prepare_root_frame(root_frame_id)?;
        }
        self.hit_test_frame(root_frame_id, Point::new(x, y))
    }

    fn compose_frame(
        &mut self,
        frame_id: FrameId,
        font_receiver: &FontTransportReceiver,
        stack: &mut HashSet<FrameId>,
        frame_local_to_root: Affine,
    ) -> Option<RenderScene> {
        if input_debug_enabled() {
            trace!("[input-debug][compositor] composing frame {}", frame_id.0);
        }

        let parent_viewport = self
            .committed_frames
            .get(&frame_id)?
            .resolved_viewport
            .clone()?;

        let (embed_sites, decoded_scene) = {
            let frame = self.committed_frames.get(&frame_id)?;
            let embed_sites = frame.composition.embed_sites.clone();
            let scene = frame.scene.clone().into_scene(font_receiver);

            (embed_sites, scene)
        };

        let bg_map: HashMap<_, _> = embed_sites
            .iter()
            .filter_map(|site| match site {
                EmbedSite::Frame(f) => Some((f.embed_site_id, f.background_policy)),
                EmbedSite::Video(_) => None,
            })
            .collect();

        let mut paint_items: Vec<(i32, u32, &EmbedSite)> = embed_sites
            .iter()
            .map(|site| (site.z_index(), site.paint_order(), site))
            .collect();
        paint_items.sort_by_key(|(z, p, _)| (*z, *p));

        let mut composed_scene = RenderScene::with_tolerance(decoded_scene.tolerance);

        for command in decoded_scene.commands {
            composed_scene.commands.push(command);
        }

        for (_, _, site) in paint_items {
            match site {
                EmbedSite::Frame(iframe_site) => {
                    let child_frame_id = iframe_site.child_frame_id;
                    let Some(child_local_to_root) = self.record_child_frame_layout(
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

                    if let Some(child_scene) = self.compose_frame(
                        child_frame_id,
                        font_receiver,
                        stack,
                        child_local_to_root,
                    ) {
                        let clip = Self::embed_local_clip(iframe_site);
                        let transform = Affine::new(iframe_site.layout.transform);
                        let child_transform = self
                            .child_scene_transform(&clip, child_frame_id)
                            .map(|scene_transform| transform * scene_transform)
                            .unwrap_or(transform);
                        if matches!(
                            bg_map.get(&iframe_site.embed_site_id),
                            Some(EmbedBackgroundPolicy::OpaqueWhite)
                        ) {
                            composed_scene.fill(
                                Fill::NonZero,
                                transform,
                                Color::WHITE,
                                None,
                                &clip,
                            );
                        }
                        composed_scene.push_clip_layer(transform, &clip);
                        composed_scene.append_scene(child_scene, child_transform);
                        composed_scene.pop_layer();
                        if input_debug_enabled() {
                            trace!(
                                "[input-debug][compositor] composed iframe site {} with child frame {}",
                                iframe_site.embed_site_id.0, child_frame_id.0
                            );
                        }
                    }

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
                    let transform = Affine::new(video_data.layout.transform);

                    let tx = transform.as_coeffs()[4];
                    let ty = transform.as_coeffs()[5];
                    let clip_rect = Rect::new(
                        video_data.layout.clip_bounds[0] - tx,
                        video_data.layout.clip_bounds[1] - ty,
                        video_data.layout.clip_bounds[2] - tx,
                        video_data.layout.clip_bounds[3] - ty,
                    );
                    let rounded_clip: Option<RoundedRect> = if video_data.clip_radius > 0.0 {
                        Some(RoundedRect::from_rect(clip_rect, video_data.clip_radius))
                    } else {
                        None
                    };
                    let local_clip = clip_rect;

                    let pixel_data = video_frame.data.clone();
                    let image_data = ImageData {
                        data: peniko::Blob::from(pixel_data.to_vec()),
                        format: ImageFormat::Rgba8,
                        alpha_type: ImageAlphaType::Alpha,
                        width: video_frame.width,
                        height: video_frame.height,
                    };

                    let local_w = local_clip.width();
                    let local_h = local_clip.height();
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

                    match rounded_clip {
                        Some(ref rc) => composed_scene.push_clip_layer(transform, rc),
                        None => composed_scene.push_clip_layer(transform, &local_clip),
                    };
                    composed_scene.draw_image(ImageBrushRef::from(&image_data), video_transform);
                    composed_scene.pop_layer();
                }
            }
        }

        Some(composed_scene)
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

    fn record_child_frame_layout(
        &mut self,
        parent_frame_id: FrameId,
        parent_viewport: &ResolvedViewport,
        parent_local_to_root: Affine,
        iframe_site: &IframeEmbedSite,
    ) -> Option<Affine> {
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
            frame.child_frames.push(layout);
        }

        if let Some(resolved_viewport) = self.frame_viewport(iframe_site.child_frame_id)
            && let Some(child_frame) = self.committed_frames.get_mut(&iframe_site.child_frame_id)
        {
            child_frame.parent_frame_id = Some(parent_frame_id);
            child_frame.resolved_viewport = Some(resolved_viewport);
        }

        Some(child_local_to_root)
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

    fn hit_test_frame(&self, frame_id: FrameId, point: Point) -> Option<HitTestResult> {
        let frame = self.committed_frames.get(&frame_id)?;
        let resolved_viewport = frame.resolved_viewport.as_ref()?;
        if !resolved_viewport.contains_local_point(point) {
            return None;
        }

        for child in frame.child_frames.iter().rev() {
            if child.clip_bounds.contains(point) {
                let child_point = child.child_local_from_parent * point;
                if let Some(hit) = self.hit_test_frame(child.child_frame_id, child_point) {
                    return Some(hit);
                }
                if let Some(hit) = self.local_hit(child.child_frame_id, child_point) {
                    return Some(hit);
                }
            }
        }

        self.local_hit(frame_id, point)
    }

    fn local_hit(&self, frame_id: FrameId, point: Point) -> Option<HitTestResult> {
        let frame = self.committed_frames.get(&frame_id);
        Some(HitTestResult {
            frame_id,
            local_x: point.x as f32,
            local_y: point.y as f32,
            is_child_frame: frame.is_none_or(|frame| frame.parent_frame_id.is_some()),
            has_child_frames: frame.is_some_and(|frame| !frame.child_frames.is_empty()),
        })
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
