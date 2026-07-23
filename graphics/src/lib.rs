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
    /// Viewport data for child frames, keyed by child webview_id.
    /// Populated during compose_scene from the compositor's visible_frame_viewports.
    pub child_viewports: HashMap<WebviewId, [f64; 4]>,
    /// Mapping from child frame_id (the content_frame_id used in embed sites)
    /// to the child webview_id. Used by the UA to route UI events to the
    /// correct child traversable instead of the root.
    pub child_frame_to_webview: HashMap<FrameId, WebviewId>,
}

struct WebviewCompositorSlot {
    compositor: Compositor,
    font_receiver: FontTransportReceiver,
    font_sender: FontTransportSender,
    next_shmem_key: usize,
    child_frame_to_parent: HashMap<FrameId, WebviewId>,
}

impl WebviewCompositorSlot {
    fn new() -> Self {
        Self {
            compositor: Compositor::default(),
            font_receiver: FontTransportReceiver::default(),
            font_sender: FontTransportSender::default(),
            next_shmem_key: 1,
            child_frame_to_parent: HashMap::new(),
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
    let mut pipeline_webview_map: HashMap<MediaPipelineId, (WebviewId, VideoPaintId)> =
        HashMap::new();

    // Reverse mapping from child webview -> (parent webview, content_frame_id).
    // Populated by RegisterChildNavigableHost and used in PaintFrame to remap
    // child PaintFrames into the parent's compositor slot.
    let mut child_webview_to_parent: HashMap<WebviewId, (WebviewId, FrameId)> = HashMap::new();

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
                    &mut pipeline_webview_map,
                    backend.as_mut(),
                    &mut child_webview_to_parent,
                ) {
                    break;
                }
            }
            recv(media_event_rx) -> event => {
                let Ok(event) = event else { break };
                handle_media_event(
                    event,
                    &pipeline_webview_map,
                    &mut webviews,
                    &event_sender,
                    &child_webview_to_parent,
                );
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
    composed_scene_sender: &ipc::IpcSender<GraphicsEvent>,
    child_webview_to_parent: &HashMap<WebviewId, (WebviewId, FrameId)>,
) {
    match event {
        MediaBackendEvent::Frame(mut video_frame) => {
            let pipeline_id = video_frame.pipeline_id;
            let Some(&(webview_id, paint_id)) = pipeline_webview_map.get(&pipeline_id) else {
                debug!("[graphics] frame for unknown pipeline {:?}", pipeline_id);
                return;
            };
            debug!(
                "[graphics] video frame arrived pipeline={:?} webview={:?}",
                pipeline_id, webview_id
            );
            let pixel_bytes: std::sync::Arc<[u8]> = std::mem::take(&mut video_frame.data).into();
            let cf = CompositorVideoFrame {
                video_paint_id: paint_id,
                width: video_frame.width,
                height: video_frame.height,
                data: pixel_bytes,
            };
            if let Some(slot) = webviews.get_mut(&webview_id) {
                slot.compositor.update_video_frame(cf);
                // Compose and send the scene so the video appears immediately.
                if slot.compositor.committed_root_frame_id().is_some() {
                    if let Some(mut composed) = slot
                        .compositor
                        .compose_scene(&slot.font_receiver, webview_id)
                    {
                        let (cv, cftw) = build_child_data(
                            &mut slot.compositor,
                            child_webview_to_parent,
                            &slot.font_receiver,
                        );
                        composed.child_viewports = cv;
                        composed.child_frame_to_webview = cftw;
                        let _ = send_composed_scene(composed_scene_sender.clone(), slot, composed);
                    }
                }
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
    pipeline_webview_map: &mut HashMap<MediaPipelineId, (WebviewId, VideoPaintId)>,
    media_backend: Option<&mut B>,
    child_webview_to_parent: &mut HashMap<WebviewId, (WebviewId, FrameId)>,
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
            // Remap child PaintFrames into the parent's compositor slot.
            let (target_webview_id, actual_frame_id, is_root_candidate) =
                if let Some(&(parent_id, content_frame_id)) =
                    child_webview_to_parent.get(&frame.traversable_id)
                {
                    (parent_id, content_frame_id, false)
                } else {
                    (frame.traversable_id, frame.frame_id, true)
                };
            let webview_id = target_webview_id;
            let slot = webviews
                .entry(webview_id)
                .or_insert_with(WebviewCompositorSlot::new);
            let composition = frame.composition.clone();
            let viewport_width = frame.viewport_width;
            let viewport_height = frame.viewport_height;
            let frame_id = actual_frame_id;
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
            // Compose and send when the root frame is updated or any child frame
            // arrives. Child frames are remapped into the parent's compositor slot;
            // when the root exists, every new frame triggers a re-composition so
            // the updated scene is pushed back to the user agent.
            let should_compose =
                slot.compositor.committed_root_frame_id().is_some();
            if should_compose {
                if let Some(mut composed) = slot
                    .compositor
                    .compose_scene(&slot.font_receiver, webview_id)
                {
                    // Populate child data for the UA to publish and route.
                    let (cv, cftw) = build_child_data(
                        &mut slot.compositor,
                        child_webview_to_parent,
                        &slot.font_receiver,
                    );
                    composed.child_viewports = cv;
                    composed.child_frame_to_webview = cftw;
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
            }
            child_webview_to_parent.insert(
                child_webview_id,
                (parent_traversable_id, content_frame_id),
            );
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
        GraphicsCommand::NavigationFinalized { webview_id } => {
            if let Some(slot) = webviews.get_mut(&webview_id) {
                slot.compositor.note_navigation_finalized();
            }
        }
        GraphicsCommand::CreateMediaPipeline {
            pipeline_id,
            url,
            webview_id,
            video_paint_id,
        } => {
            debug!(
                "[graphics:media] create pipeline {:?} url={} webview={:?} paint={:?}",
                pipeline_id, url, webview_id, video_paint_id
            );
            pipeline_webview_map.insert(pipeline_id, (webview_id, video_paint_id));
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

/// Extract child frame data from the compositor and match against the
/// child_webview_to_parent mapping. Returns (child_viewports, child_frame_to_webview).
fn build_child_data(
    compositor: &mut Compositor,
    child_webview_to_parent: &HashMap<WebviewId, (WebviewId, FrameId)>,
    font_receiver: &FontTransportReceiver,
) -> (HashMap<WebviewId, [f64; 4]>, HashMap<FrameId, WebviewId>) {
    let mut viewports = HashMap::new();
    let mut frame_to_webview = HashMap::new();
    // Build a reverse lookup: content_frame_id -> child_webview_id
    let frame_to_child: HashMap<FrameId, WebviewId> = child_webview_to_parent
        .iter()
        .map(|(child, &(_, content_fid))| (content_fid, *child))
        .collect();
    for vp in compositor.visible_frame_viewports(font_receiver) {
        let Some(&child_wv) = frame_to_child.get(&vp.frame_id) else {
            continue;
        };
        frame_to_webview.insert(vp.frame_id, child_wv);
        viewports.insert(
            child_wv,
            [
                f64::from(vp.offset_x),
                f64::from(vp.offset_y),
                f64::from(vp.offset_x) + f64::from(vp.width),
                f64::from(vp.offset_y) + f64::from(vp.height),
            ],
        );
    }
    (viewports, frame_to_webview)
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
        child_viewports,
        child_frame_to_webview,
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
                child_viewports: child_viewports
                    .into_iter()
                    .map(|(child_wv, bounds)| {
                        use ipc_messages::graphics::ChildViewport;
                        ChildViewport {
                            child_webview_id: child_wv,
                            root_clip_bounds: bounds,
                        }
                    })
                    .collect(),
                    child_frame_to_webview,
            },
            shmem,
        )
        .is_err()
    {
        return Err(());
    }
    Ok(())
}
