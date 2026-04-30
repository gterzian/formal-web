use anyrender::{PaintScene, Scene as RenderScene};
use anyrender::recording::RenderCommand;
use crate::log_iframe_debug;
use ipc_messages::content::{FontTransportReceiver, FrameId, RecordedScene, ScrollOffset};
use kurbo::{Affine, Shape};
use peniko::{Color, Fill};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct CachedFrame {
    viewport_scroll: ScrollOffset,
    viewport_width: u32,
    viewport_height: u32,
    scene: RecordedScene,
}

#[derive(Clone, Default)]
pub struct Compositor {
    root_frame_id: Option<FrameId>,
    committed_frames: HashMap<FrameId, CachedFrame>,
    pending_frames: HashMap<FrameId, CachedFrame>,
    replace_root_on_next_paint: bool,
}

impl Compositor {
    fn frame_can_be_root(frame_id: FrameId) -> bool {
        frame_id.0 < (1_u64 << 63)
    }

    pub fn note_navigation_finalized(&mut self) {
        log_iframe_debug(format!(
            "note_navigation_finalized root_before={:?} committed_frames={} pending_frames={}",
            self.root_frame_id.map(|id| id.0),
            self.committed_frames.len(),
            self.pending_frames.len()
        ));
        self.pending_frames.clear();
        self.replace_root_on_next_paint = true;
    }

    pub fn store_frame(
        &mut self,
        frame_id: FrameId,
        viewport_width: u32,
        viewport_height: u32,
        viewport_scroll: ScrollOffset,
        scene: RecordedScene,
    ) {
        let frame = CachedFrame {
            viewport_scroll,
            viewport_width,
            viewport_height,
            scene,
        };

        log_iframe_debug(format!(
            "store_frame frame={} viewport={}x{} replace_root={} current_root={:?}",
            frame_id.0,
            viewport_width,
            viewport_height,
            self.replace_root_on_next_paint,
            self.root_frame_id.map(|id| id.0)
        ));

        if self.replace_root_on_next_paint {
            self.pending_frames.insert(frame_id, frame);
            if Self::frame_can_be_root(frame_id) {
                self.root_frame_id = Some(frame_id);
                self.committed_frames = std::mem::take(&mut self.pending_frames);
                self.replace_root_on_next_paint = false;
                log_iframe_debug(format!(
                    "commit_new_root frame={} committed_frames={}",
                    frame_id.0,
                    self.committed_frames.len()
                ));
            }
            return;
        }

        if self.root_frame_id.is_none() && Self::frame_can_be_root(frame_id) {
            self.root_frame_id = Some(frame_id);
            log_iframe_debug(format!("select_initial_root frame={}", frame_id.0));
        }

        self.committed_frames.insert(frame_id, frame);
        log_iframe_debug(format!(
            "stored_committed_frame frame={} root={:?} committed_frames={}",
            frame_id.0,
            self.root_frame_id.map(|id| id.0),
            self.committed_frames.len()
        ));
    }

    pub fn committed_root_frame_id(&self) -> Option<FrameId> {
        self.root_frame_id
    }

    pub fn root_viewport_scroll(&self) -> ScrollOffset {
        self.root_frame_id
            .and_then(|frame_id| self.committed_frames.get(&frame_id))
            .map(|frame| frame.viewport_scroll.clone())
            .unwrap_or(ScrollOffset { x: 0.0, y: 0.0 })
    }

    pub fn compose_scene(&self, font_receiver: &FontTransportReceiver) -> Option<RenderScene> {
        let root_frame_id = self.root_frame_id?;
        log_iframe_debug(format!(
            "compose_scene root={} committed_frames={}",
            root_frame_id.0,
            self.committed_frames.len()
        ));
        let mut stack = HashSet::from([root_frame_id]);
        self.compose_frame(root_frame_id, font_receiver, &mut stack)
    }

    fn compose_frame(
        &self,
        frame_id: FrameId,
        font_receiver: &FontTransportReceiver,
        stack: &mut HashSet<FrameId>,
    ) -> Option<RenderScene> {
        let cached_frame = self.committed_frames.get(&frame_id)?;
        let decoded_scene = cached_frame.scene.clone().into_scene(font_receiver);
        let mut composed_scene = RenderScene::with_tolerance(decoded_scene.tolerance);

        for command in decoded_scene.commands {
            match command {
                RenderCommand::IframePlaceholder(placeholder) => {
                    let child_frame_id = FrameId(placeholder.frame_id);
                    let clip_bounds = placeholder.clip.bounding_box();
                    if !stack.insert(child_frame_id) {
                        log_iframe_debug(format!(
                            "skip_placeholder_cycle parent={} child={} clip=({}, {}, {}x{})",
                            frame_id.0,
                            child_frame_id.0,
                            clip_bounds.x0,
                            clip_bounds.y0,
                            clip_bounds.width(),
                            clip_bounds.height()
                        ));
                        continue;
                    }

                    if let Some(child_scene) =
                        self.compose_frame(child_frame_id, font_receiver, stack)
                    {
                        let child_transform = self
                            .child_scene_transform(&placeholder.clip, child_frame_id)
                            .map(|transform| placeholder.transform * transform)
                            .unwrap_or(placeholder.transform);
                        log_iframe_debug(format!(
                            "compose_placeholder parent={} child={} clip=({}, {}, {}x{})",
                            frame_id.0,
                            child_frame_id.0,
                            clip_bounds.x0,
                            clip_bounds.y0,
                            clip_bounds.width(),
                            clip_bounds.height()
                        ));
                        composed_scene.fill(
                            Fill::NonZero,
                            placeholder.transform,
                            Color::WHITE,
                            None,
                            &placeholder.clip,
                        );
                        composed_scene.push_clip_layer(placeholder.transform, &placeholder.clip);
                        composed_scene.append_scene(child_scene, child_transform);
                        composed_scene.pop_layer();
                    } else {
                        log_iframe_debug(format!(
                            "missing_child_frame parent={} child={} clip=({}, {}, {}x{})",
                            frame_id.0,
                            child_frame_id.0,
                            clip_bounds.x0,
                            clip_bounds.y0,
                            clip_bounds.width(),
                            clip_bounds.height()
                        ));
                    }

                    stack.remove(&child_frame_id);
                }
                command => composed_scene.commands.push(command),
            }
        }

        Some(composed_scene)
    }

    fn child_scene_transform(&self, clip: &impl Shape, child_frame_id: FrameId) -> Option<Affine> {
        let child_frame = self.committed_frames.get(&child_frame_id)?;
        if child_frame.viewport_width == 0 || child_frame.viewport_height == 0 {
            log_iframe_debug(format!(
                "child_scene_transform_zero_viewport frame={} viewport={}x{}",
                child_frame_id.0,
                child_frame.viewport_width,
                child_frame.viewport_height
            ));
            return None;
        }

        let clip_bounds = clip.bounding_box();
        let scale_x = clip_bounds.width() / f64::from(child_frame.viewport_width);
        let scale_y = clip_bounds.height() / f64::from(child_frame.viewport_height);
        log_iframe_debug(format!(
            "child_scene_transform frame={} child_viewport={}x{} clip=({}, {}, {}x{}) scale=({:.4}, {:.4})",
            child_frame_id.0,
            child_frame.viewport_width,
            child_frame.viewport_height,
            clip_bounds.x0,
            clip_bounds.y0,
            clip_bounds.width(),
            clip_bounds.height(),
            scale_x,
            scale_y
        ));
        Some(Affine::new([scale_x, 0.0, 0.0, scale_y, 0.0, 0.0]))
    }
}