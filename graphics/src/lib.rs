pub mod compositor;

use std::collections::HashMap;

use compositor::{Compositor, CompositorVideoFrame};
use crossbeam_channel::{select, tick};
use ipc_messages::content::{FontTransportReceiver, FontTransportSender, FrameId, WebviewId};
use ipc_messages::graphics::{FrameHitInfo, GraphicsCommand, GraphicsEvent};
use ipc_messages::media::{MediaPipelineId, VideoPaintId};
use log::{debug, error};

use media::backend::{MediaBackend, MediaBackendEvent, PipelineHandle};

/// The composed scene for one webview — the final result after compositing
/// all iframe and video embed sites into the root scene.
#[derive(Clone)]
pub struct ComposedScene {
    pub webview_id: WebviewId,
    pub scene: anyrender::Scene,
    pub frame_hit_info: Vec<FrameHitInfo>,
}

struct WebviewCompositorSlot {
    compositor: Compositor,
    font_receiver: FontTransportReceiver,
    font_sender: FontTransportSender,
    next_shmem_key: usize,
    child_frame_to_parent: HashMap<FrameId, WebviewId>,
    _child_webview_to_frame: HashMap<WebviewId, (WebviewId, FrameId)>,
}

impl WebviewCompositorSlot {
    fn new() -> Self {
        Self {
            compositor: Compositor::default(),
            font_receiver: FontTransportReceiver::default(),
            font_sender: FontTransportSender::default(),
            next_shmem_key: 1,
            child_frame_to_parent: HashMap::new(),
            _child_webview_to_frame: HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibleFrameViewport {
    pub frame_id: FrameId,
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: u32,
    pub height: u32,
}

/// Run the graphics process event loop.
/// The media backend (if provided) runs directly in this loop — no separate IPC.
/// The pipeline_to_webview mapping is managed via RegisterMediaPipeline from content.
pub fn run_graphics_process<B: MediaBackend + 'static>(
    cmd_rx: crossbeam_channel::Receiver<ipc::IpcIncoming<GraphicsCommand>>,
    graphics_event_tx: ipc::IpcSender<GraphicsEvent>,
    media_backend: Option<B>,
) {
    let mut webviews: HashMap<WebviewId, WebviewCompositorSlot> = HashMap::new();
    let event_sender = graphics_event_tx;

    // Media pipeline state.
    let mut pipelines: HashMap<MediaPipelineId, B::Pipeline> = HashMap::new();
    let sample_tick = tick(std::time::Duration::from_millis(8));
    let _pipeline_webview_map: HashMap<MediaPipelineId, (WebviewId, VideoPaintId)> =
        HashMap::new();

    // Use crossbeam's never() channel when there's no backend so the select! loop
    // has a single uniform structure regardless of whether a backend exists.
    let (mut backend, media_event_rx) = match media_backend {
        Some(b) => {
            let rx = b.event_receiver();
            (Some(b), rx)
        }
        None => (None, crossbeam_channel::never()),
    };

    loop {
        select! {
            recv(cmd_rx) -> cmd => {
                let Ok(incoming) = cmd else { break };
                if handle_command(
                    incoming.payload,
                    &mut webviews,
                    &event_sender,
                    &incoming.shmem_regions,
                    &mut pipelines,
                    &_pipeline_webview_map,
                    backend.as_mut(),
                ) {
                    break;
                }
            }
            recv(media_event_rx) -> event => {
                let Ok(event) = event else { break };
                handle_media_event(event, &_pipeline_webview_map, &mut webviews);
            }
            recv(sample_tick) -> _ => {
                for pipeline in pipelines.values() {
                    pipeline.sample();
                }
            }
        }
    }
}

fn handle_media_event(
    event: MediaBackendEvent,
    pipeline_webview_map: &HashMap<MediaPipelineId, (WebviewId, VideoPaintId)>,
    webviews: &mut HashMap<WebviewId, WebviewCompositorSlot>,
) {
    match event {
        MediaBackendEvent::Frame(mut video_frame) => {
            let pipeline_id = video_frame.pipeline_id;
            let Some(&(webview_id, paint_id)) = pipeline_webview_map.get(&pipeline_id) else {
                debug!("[graphics] frame for unknown pipeline {:?}", pipeline_id);
                return;
            };
            let pixel_bytes: std::sync::Arc<[u8]> = std::mem::take(&mut video_frame.data).into();
            let cf = CompositorVideoFrame {
                video_paint_id: paint_id,
                width: video_frame.width,
                height: video_frame.height,
                data: pixel_bytes,
            };
            // Store the video frame for the next PaintFrame-triggered composition.
            // We do NOT compose here — the video frame is already in the compositor
            // and will be picked up when the next DOM PaintFrame arrives.
            if let Some(slot) = webviews.get_mut(&webview_id) {
                slot.compositor.update_video_frame(cf);
            }
        }
        MediaBackendEvent::Eos { pipeline_id } => {
            debug!("[graphics] pipeline {:?} end of stream", pipeline_id);
        }
        MediaBackendEvent::Error {
            pipeline_id,
            message,
        } => {
            error!("[graphics] pipeline {:?} error: {}", pipeline_id, message);
        }
        MediaBackendEvent::DurationChanged {
            pipeline_id,
            duration_secs,
        } => {
            debug!(
                "[graphics] pipeline {:?} duration: {}s",
                pipeline_id, duration_secs
            );
        }
    }
}

fn handle_command<B: MediaBackend + 'static>(
    cmd: GraphicsCommand,
    webviews: &mut HashMap<WebviewId, WebviewCompositorSlot>,
    composed_scene_sender: &ipc::IpcSender<GraphicsEvent>,
    shmem_regions: &HashMap<usize, ipc::IpcSharedRegion>,
    pipelines: &mut HashMap<MediaPipelineId, B::Pipeline>,
    _pipeline_webview_map: &HashMap<MediaPipelineId, (WebviewId, VideoPaintId)>,
    media_backend: Option<&mut B>,
) -> bool {
    match cmd {
        GraphicsCommand::RegisterWebview { webview_id } => {
            debug!("[graphics] registering webview {:?}", webview_id);
            webviews
                .entry(webview_id)
                .or_insert_with(WebviewCompositorSlot::new);
        }
        GraphicsCommand::UnregisterWebview { webview_id } => {
            debug!("[graphics] unregistering webview {:?}", webview_id);
            webviews.remove(&webview_id);
        }
        GraphicsCommand::PaintFrame { frame } => {
            let webview_id = frame.traversable_id;
            let slot = webviews
                .entry(webview_id)
                .or_insert_with(WebviewCompositorSlot::new);
            let is_root_candidate = !slot.child_frame_to_parent.contains_key(&frame.frame_id);
            let composition = frame.composition.clone();
            let viewport_width = frame.viewport_width;
            let viewport_height = frame.viewport_height;
            let frame_id = frame.frame_id;
            let recorded_scene =
                match frame.into_recorded_scene(&mut slot.font_receiver, shmem_regions) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("[graphics] deserialize paint frame: {e}");
                        return false;
                    }
                };
            slot.compositor.store_frame(
                frame_id,
                viewport_width,
                viewport_height,
                composition,
                recorded_scene,
                is_root_candidate,
            );
            if slot.compositor.committed_root_frame_id() == Some(frame_id) {
                if let Some(composed) = slot
                    .compositor
                    .compose_scene(&slot.font_receiver, webview_id)
                {
                    let _ = send_composed_scene(composed_scene_sender.clone(), slot, composed);
                }
            }
        }
        GraphicsCommand::RemoveVideoFrame {
            webview_id,
            paint_id,
        } => {
            if let Some(slot) = webviews.get_mut(&webview_id) {
                slot.compositor.remove_video_frame(paint_id);
            }
        }
        GraphicsCommand::RegisterChildNavigableHost {
            child_webview_id,
            parent_traversable_id,
            content_frame_id,
        } => {
            if let Some(slot) = webviews.get_mut(&parent_traversable_id) {
                slot.child_frame_to_parent
                    .insert(content_frame_id, parent_traversable_id);
                slot._child_webview_to_frame
                    .insert(child_webview_id, (parent_traversable_id, content_frame_id));
            }
        }
        GraphicsCommand::ChildNavigationFinalized {
            parent_traversable_id,
            content_frame_id,
        } => {
            if let Some(slot) = webviews.get_mut(&parent_traversable_id) {
                slot.compositor
                    .note_child_navigation_finalized(content_frame_id);
            }
        }
        GraphicsCommand::CreateMediaPipeline { pipeline_id, url } => {
            debug!(
                "[graphics:media] create pipeline {:?} url={}",
                pipeline_id, url
            );
            if let Some(backend) = media_backend {
                match backend.create_pipeline(pipeline_id, url) {
                    Ok(pipeline) => {
                        pipelines.insert(pipeline_id, pipeline);
                    }
                    Err(e) => error!("[graphics:media] create failed: {e}"),
                }
            }
        }
        GraphicsCommand::MediaPlay { pipeline_id } => {
            if let Some(p) = pipelines.get(&pipeline_id) {
                if let Err(e) = p.play() {
                    error!("[graphics:media] play: {e}");
                }
            }
        }
        GraphicsCommand::MediaPause { pipeline_id } => {
            if let Some(p) = pipelines.get(&pipeline_id) {
                if let Err(e) = p.pause() {
                    error!("[graphics:media] pause: {e}");
                }
            }
        }
        GraphicsCommand::MediaSeek {
            pipeline_id,
            position_secs,
        } => {
            if let Some(p) = pipelines.get(&pipeline_id) {
                if let Err(e) = p.seek(position_secs) {
                    error!("[graphics:media] seek: {e}");
                }
            }
        }
        GraphicsCommand::MediaDestroy { pipeline_id } => {
            if let Some(p) = pipelines.remove(&pipeline_id) {
                if let Err(e) = p.destroy() {
                    error!("[graphics:media] destroy: {e}");
                }
            }
        }
        GraphicsCommand::Shutdown => return true,
    }
    false
}

fn send_composed_scene(
    sender: ipc::IpcSender<GraphicsEvent>,
    slot: &mut WebviewCompositorSlot,
    composed: ComposedScene,
) -> Result<(), ()> {
    let ComposedScene {
        webview_id,
        scene,
        frame_hit_info,
    } = composed;
    let prepared = slot
        .font_sender
        .prepare_scene(0, scene, &mut slot.next_shmem_key);
    let font_registrations = prepared.registered_fonts.clone();
    use ipc_messages::content::PaintFrame;
    let (pf, shmem) = match PaintFrame::new(
        webview_id,
        ipc_messages::content::FrameId::from_u128(0),
        0,
        0,
        ipc_messages::content::FrameCompositionMetadata::default(),
        prepared,
        &mut slot.next_shmem_key,
    ) {
        Ok(r) => r,
        Err(e) => {
            error!("[graphics] serialize composed: {e}");
            return Err(());
        }
    };
    let key = pf.scene_shmem_key;
    if sender
        .send_with_shmem_map(
            GraphicsEvent::ComposedSceneReady {
                webview_id,
                scene_shmem_key: key,
                font_registrations,
                frame_hit_info,
            },
            shmem,
        )
        .is_err()
    {
        return Err(());
    }
    Ok(())
}
