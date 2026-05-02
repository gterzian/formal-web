use anyrender::{PaintScene, Scene as RenderScene};
use anyrender::recording::RenderCommand;
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

        if self.replace_root_on_next_paint {
            self.pending_frames.insert(frame_id, frame);
            if Self::frame_can_be_root(frame_id) {
                self.root_frame_id = Some(frame_id);
                self.committed_frames = std::mem::take(&mut self.pending_frames);
                self.replace_root_on_next_paint = false;
            }
            return;
        }

        if self.root_frame_id.is_none() && Self::frame_can_be_root(frame_id) {
            self.root_frame_id = Some(frame_id);
        }

        self.committed_frames.insert(frame_id, frame);
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
                    if !stack.insert(child_frame_id) {
                        continue;
                    }

                    if let Some(child_scene) =
                        self.compose_frame(child_frame_id, font_receiver, stack)
                    {
                        let child_transform = self
                            .child_scene_transform(&placeholder.clip, child_frame_id)
                            .map(|transform| placeholder.transform * transform)
                            .unwrap_or(placeholder.transform);
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
            return None;
        }

        let clip_bounds = clip.bounding_box();
        let scale_x = clip_bounds.width() / f64::from(child_frame.viewport_width);
        let scale_y = clip_bounds.height() / f64::from(child_frame.viewport_height);
        Some(Affine::new([scale_x, 0.0, 0.0, scale_y, 0.0, 0.0]))
    }
}