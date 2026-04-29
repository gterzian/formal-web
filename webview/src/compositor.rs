use anyrender::{PaintScene, Scene as RenderScene};
use anyrender::recording::RenderCommand;
use ipc_messages::content::{FontTransportReceiver, FrameId, RecordedScene, ScrollOffset};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct CachedFrame {
    viewport_scroll: ScrollOffset,
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
    pub fn note_navigation_finalized(&mut self) {
        self.pending_frames.clear();
        self.replace_root_on_next_paint = true;
    }

    pub fn store_frame(
        &mut self,
        frame_id: FrameId,
        viewport_scroll: ScrollOffset,
        scene: RecordedScene,
    ) {
        let frame = CachedFrame {
            viewport_scroll,
            scene,
        };

        if self.replace_root_on_next_paint {
            self.pending_frames.insert(frame_id, frame);
            self.root_frame_id = Some(frame_id);
            self.committed_frames = std::mem::take(&mut self.pending_frames);
            self.replace_root_on_next_paint = false;
            return;
        }

        if self.root_frame_id.is_none() {
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
                        composed_scene.push_clip_layer(placeholder.transform, &placeholder.clip);
                        composed_scene.append_scene(child_scene, placeholder.transform);
                        composed_scene.pop_layer();
                    }

                    stack.remove(&child_frame_id);
                }
                command => composed_scene.commands.push(command),
            }
        }

        Some(composed_scene)
    }
}