pub mod backend;
pub mod compositor;

use std::collections::HashMap;

use compositor::{Compositor, CompositorVideoFrame};
use ipc_messages::content::{FontTransportReceiver, FontTransportSender, FrameId, WebviewId};
use ipc_messages::graphics::{FrameHitInfo, GraphicsCommand, GraphicsEvent};
use log::{debug, error};

use backend::MediaBackend;

/// The composed scene for one webview — the final result after compositing
/// all iframe and video embed sites into the root scene.
#[derive(Clone)]
pub struct ComposedScene {
    /// The webview this scene belongs to.
    pub webview_id: WebviewId,
    /// The final composed render scene.
    pub scene: anyrender::Scene,
    /// Hit-testing info for the composed frame tree.
    pub frame_hit_info: Vec<FrameHitInfo>,
}

/// A per-webview compositor slot in the graphics process.
struct WebviewCompositorSlot {
    compositor: Compositor,
    font_receiver: FontTransportReceiver,
    font_sender: FontTransportSender,
    /// Monotonically increasing key for IPC shared memory regions.
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
pub fn run_graphics_process<B: MediaBackend + 'static>(
    cmd_rx: crossbeam_channel::Receiver<ipc::IpcIncoming<GraphicsCommand>>,
    graphics_event_tx: ipc::IpcSender<GraphicsEvent>,
    _media_backend: Option<B>,
) {
    let _ = _media_backend;
    use crossbeam_channel::{select, tick};

    let mut webviews: HashMap<WebviewId, WebviewCompositorSlot> = HashMap::new();
    let composed_scene_sender: ipc::IpcSender<GraphicsEvent> = graphics_event_tx;
    let sample_tick = tick(std::time::Duration::from_millis(8));

    loop {
        select! {
            recv(cmd_rx) -> cmd => {
                let Ok(incoming) = cmd else { break; };
                if handle_command(
                    incoming.payload,
                    &mut webviews,
                    &composed_scene_sender,
                    &incoming.shmem_regions,
                ) {
                    break;
                }
            }
            recv(sample_tick) -> _ => {
                // Media backend sampling placeholder.
            }
        }
    }
}

/// Process a single GraphicsCommand. Returns true if the loop should exit.
fn handle_command(
    cmd: GraphicsCommand,
    webviews: &mut HashMap<WebviewId, WebviewCompositorSlot>,
    composed_scene_sender: &ipc::IpcSender<GraphicsEvent>,
    shmem_regions: &std::collections::HashMap<usize, ipc::IpcSharedRegion>,
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
            debug!(
                "[graphics] received PaintFrame for webview {:?} frame={}",
                webview_id, frame.frame_id.0
            );

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
                    Ok(scene) => scene,
                    Err(error) => {
                        error!("[graphics] failed to deserialize paint frame: {error}");
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
                let composed = slot
                    .compositor
                    .compose_scene(&slot.font_receiver, webview_id);
                if let Some(composed) = composed {
                    debug!(
                        "[graphics] composed scene for webview {:?}, {} hit-info entries",
                        webview_id,
                        composed.frame_hit_info.len(),
                    );
                    let _ = send_composed_scene(composed_scene_sender.clone(), slot, composed);
                }
            }
        }
        GraphicsCommand::VideoFrameReady {
            webview_id,
            paint_id,
            data: video_frame,
        } => {
            let mut video_frame = video_frame;
            // Restore pixel data from shared memory (serde skips the data field).
            if let Some(region) = shmem_regions.get(&0) {
                video_frame.data = region.as_slice().to_vec();
            }
            debug!(
                "[graphics] received video frame: {}x{} paint={:?} webview={:?} data={}",
                video_frame.width,
                video_frame.height,
                paint_id,
                webview_id,
                video_frame.data.len(),
            );
            let frame_data = std::mem::take(&mut video_frame.data);
            let pixel_bytes: std::sync::Arc<[u8]> = frame_data.into();
            let compositor_frame = CompositorVideoFrame {
                video_paint_id: paint_id,
                width: video_frame.width,
                height: video_frame.height,
                data: pixel_bytes,
            };

            if let Some(slot) = webviews.get_mut(&webview_id) {
                slot.compositor.update_video_frame(compositor_frame);

                if slot.compositor.committed_root_frame_id().is_some() {
                    let composed = slot
                        .compositor
                        .compose_scene(&slot.font_receiver, webview_id);
                    if let Some(composed) = composed {
                        let _ = send_composed_scene(composed_scene_sender.clone(), slot, composed);
                    }
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
        GraphicsCommand::Shutdown => {
            return true;
        }
    }
    false
}

/// Send a composed scene back to the user agent via the GraphicsEvent sender.
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

    // Serialize the RenderScene into a RecordedScene (which is Serializable),
    // then place the bytes into IPC shared memory.
    let prepared = slot.font_sender.prepare_scene(
        0, // font namespace — 0 for graphics process
        scene,
        &mut slot.next_shmem_key,
    );
    let font_registrations = prepared.registered_fonts.clone();

    // Create the PaintFrame shmem from the prepared scene.
    use ipc_messages::content::PaintFrame;
    let (_paint_frame, shmem_map) = match PaintFrame::new(
        webview_id,
        ipc_messages::content::FrameId::from_u128(0), // placeholder frame id
        0,                                            // viewport width — UA will replace
        0,                                            // viewport height
        ipc_messages::content::FrameCompositionMetadata::default(),
        prepared,
        &mut slot.next_shmem_key,
    ) {
        Ok(result) => result,
        Err(error) => {
            error!("[graphics] failed to serialize composed scene: {error}");
            return Err(());
        }
    };

    // Send the event with shared memory regions.
    let scene_shmem_key = _paint_frame.scene_shmem_key;
    if sender
        .send_with_shmem_map(
            GraphicsEvent::ComposedSceneReady {
                webview_id,
                scene_shmem_key,
                font_registrations,
                frame_hit_info,
            },
            shmem_map,
        )
        .is_err()
    {
        return Err(());
    }

    Ok(())
}
