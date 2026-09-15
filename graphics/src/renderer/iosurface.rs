//! Zero-copy IOSurface surface backend (the default on macOS): renders Vello
//! directly into a shared IOSurface texture from the webview's ring; the
//! embedder imports the same surface and blits it, with no readback and no
//! IPC pixel bytes.

use super::{
    FrameDelivery, FrameMetadata, GpuContext, MAX_SURFACE_DIMENSION, PollRequest, ReadbackChannels,
    RenderError, SurfaceBuffers, SurfaceRenderer, SurfaceRingState, frame_metadata,
};
use crate::iosurface::{IosurfaceTexture, create_shared_texture};
use ipc_messages::content::WebviewId;
use ipc_messages::graphics::{CompositingLayerId, GraphicsEvent, LayerTopology, SurfacePayload};
use ipc_messages::media::VideoPaintId;
use log::{debug, error, info};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_video::CVPixelBuffer;
use objc2_metal::MTLDevice;
use std::collections::{HashMap, HashSet};

use crate::ComposedScene;

/// Per-frame data for the zero-copy path: delivered by the poll thread once
/// the render into the shared textures completes (the embedder's blit of the
/// shared surfaces is then GPU-safe).
pub struct SharedRenderData {
    pub webview_id: WebviewId,
    pub generation: u64,
    /// The per-layer topology for this cycle; `surface` is present only for
    /// layers re-rendered this cycle.
    pub layers: Vec<LayerTopology>,
    pub metadata: FrameMetadata,
}

/// The zero-copy IOSurface renderer: a [`GpuContext`] plus the per-layer
/// shared IOSurface double buffers and the shared-texture id counter.
pub struct IosurfaceRenderer {
    gpu: GpuContext,
    channels: ReadbackChannels<SharedRenderData>,
    /// Per-layer shared IOSurface double buffers (two textures each),
    /// reallocated on resize. A layer that no longer changes sits in its own
    /// ring and is touched by nobody.
    buffers: HashMap<CompositingLayerId, SurfaceBuffers<[IosurfaceTexture; 2]>>,
    /// Monotonic identity for shared IOSurface textures; changes on resize.
    texture_id_counter: u64,
}

impl IosurfaceRenderer {
    /// Allocate the two shared IOSurface textures for the double buffer,
    /// using `first_texture_id` as the id of the first texture.
    fn allocate_textures(
        renderer: &IosurfaceRenderer,
        width: u32,
        height: u32,
        first_texture_id: u64,
    ) -> Option<[IosurfaceTexture; 2]> {
        Some([
            create_shared_texture(renderer, width, height, first_texture_id)?,
            create_shared_texture(renderer, width, height, first_texture_id + 1)?,
        ])
    }

    /// The wgpu device this renderer composites with (also used to create
    /// the shared IOSurface textures).
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.gpu.device_handle.device
    }

    /// The raw Metal device backing this renderer, needed to create
    /// IOSurface-backed Metal textures.
    pub(crate) fn raw_metal_device(&self) -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
        self.gpu.raw_metal_device()
    }
}

impl SurfaceRenderer for IosurfaceRenderer {
    type RenderData = SharedRenderData;

    fn new(channels: ReadbackChannels<SharedRenderData>) -> Result<Self, String> {
        Ok(Self {
            gpu: GpuContext::new()?,
            channels,
            buffers: HashMap::new(),
            texture_id_counter: 1,
        })
    }

    fn submit_layers(
        &mut self,
        composed: ComposedScene,
        sender: &ipc::IpcSender<GraphicsEvent>,
    ) -> Result<Vec<CompositingLayerId>, RenderError> {
        let ComposedScene {
            webview_id,
            layers,
            frame_hit_info,
            child_viewports,
            child_frame_to_webview,
            animating,
            animating_frame_ids,
        } = composed;
        info!(
            "[render-pipe] Graphics GPU submit layers webview={} layers={} child_frames={} animating={}",
            webview_id.0,
            layers.len(),
            child_viewports.len(),
            animating,
        );

        let metadata = frame_metadata(
            webview_id,
            frame_hit_info,
            child_viewports,
            child_frame_to_webview,
            animating,
            animating_frame_ids,
        );

        let mut rendered = Vec::new();
        let mut topology = Vec::with_capacity(layers.len());

        for layer in layers {
            let Some(ref scene) = layer.render else {
                // Clean layer: keep its last surface, still report topology.
                topology.push(layer.into_layer_topology());
                continue;
            };
            let layer_id = layer.layer_id;
            let width = layer.width.clamp(1, MAX_SURFACE_DIMENSION);
            let height = layer.height.clamp(1, MAX_SURFACE_DIMENSION);
            let needs_new = self.buffers.get(&layer_id).is_none_or(|buffers| {
                buffers.ring().width != width || buffers.ring().height != height
            });
            if needs_new {
                let first_texture_id = self.texture_id_counter;
                let payload = Self::allocate_textures(self, width, height, first_texture_id)
                    .ok_or_else(|| {
                        error!(
                            "[graphics] allocate shared textures {}x{}: failed",
                            width, height
                        );
                        RenderError::Failed
                    })?;
                self.texture_id_counter += 2;
                self.buffers.insert(
                    layer_id,
                    SurfaceBuffers::new(SurfaceRingState::new(width, height), payload),
                );
            }
            let buffer_index = self
                .buffers
                .get_mut(&layer_id)
                .ok_or(RenderError::Failed)?
                .next_buffer();
            let target = &self.buffers[&layer_id].payload()[buffer_index].texture;
            if let Err(error) = self.gpu.render_into(scene, target, width, height) {
                error!("[gpu-renderer] layer render failed: {error}");
                return Err(RenderError::Failed);
            }
            let tex = &self.buffers[&layer_id].payload()[buffer_index];
            topology.push(
                layer.into_layer_topology_with_surface(SurfacePayload::SharedTexture {
                    texture_id: tex.texture_id,
                    surface_id: tex.surface_id(),
                    port: tex.port_for_frame(),
                }),
            );
            rendered.push(layer_id);
        }

        self.gpu.generation += 1;
        let generation = self.gpu.generation;
        if rendered.is_empty() {
            // Nothing was re-rendered this cycle (every layer clean, e.g. a
            // static root with a video that produced no new frame): emit a
            // surface-less PixelFrameReady directly so the UA still learns
            // the composition completed and clears the navigable's pending
            // update-the-rendering. The embedder keeps drawing its last
            // surfaces. No GPU work was submitted, so no poll is needed.
            let frame_event = GraphicsEvent::PixelFrameReady {
                webview_id,
                layers: topology,
                animating: metadata.animating,
                animating_frame_ids: metadata.animating_frame_ids,
                generation,
                frame_hit_info: metadata.frame_hit_info,
                child_viewports: metadata.child_viewports,
                child_frame_to_webview: metadata.child_frame_to_webview,
            };
            if let Err(send_error) = sender.send(frame_event) {
                error!(
                    "[gpu-renderer] failed to send empty PixelFrameReady for {:?}: {send_error}",
                    webview_id
                );
            }
            debug!(
                "[gpu-renderer] no shared layers rendered gen={}",
                generation
            );
            return Ok(rendered);
        }
        let done = SharedRenderData {
            webview_id,
            generation,
            layers: topology,
            metadata,
        };
        if let Err(send_error) = self.channels.poll_tx.send(PollRequest {
            device: self.gpu.device_handle.clone(),
            submission_index: None,
            done: Some(done),
        }) {
            error!("[gpu-renderer] failed to queue shared render poll: {send_error}");
        }
        debug!(
            "[gpu-renderer] rendered {} shared layers gen={}",
            rendered.len(),
            generation
        );

        Ok(rendered)
    }

    fn handle_render_done(
        &mut self,
        data: SharedRenderData,
        sender: &ipc::IpcSender<GraphicsEvent>,
    ) -> FrameDelivery {
        let SharedRenderData {
            webview_id,
            generation,
            layers,
            metadata,
        } = data;
        let mut delivery = FrameDelivery {
            graphics_computed: false,
        };

        let frame_event = GraphicsEvent::PixelFrameReady {
            webview_id,
            layers,
            animating: metadata.animating,
            animating_frame_ids: metadata.animating_frame_ids,
            generation,
            frame_hit_info: metadata.frame_hit_info,
            child_viewports: metadata.child_viewports,
            child_frame_to_webview: metadata.child_frame_to_webview,
        };
        if let Err(send_error) = sender.send(frame_event) {
            error!(
                "[graphics] failed to send PixelFrameReady for {:?} gen={}: {send_error}",
                webview_id, generation
            );
            return delivery;
        }
        delivery.graphics_computed = true;
        delivery
    }

    fn render_done_webview_id(data: &SharedRenderData) -> WebviewId {
        data.webview_id
    }

    fn store_video_frame(
        &mut self,
        paint_id: VideoPaintId,
        pixel_buffer: &Retained<CVPixelBuffer>,
        width: u32,
        height: u32,
    ) -> Option<peniko::ImageData> {
        self.gpu
            .store_video_frame(paint_id, pixel_buffer, width, height)
    }

    fn retain_layers(&mut self, live: &HashSet<CompositingLayerId>) {
        let before = self.buffers.len();
        self.buffers.retain(|layer_id, _| live.contains(layer_id));
        let live_videos: HashSet<VideoPaintId> = live
            .iter()
            .filter_map(|layer_id| match layer_id {
                CompositingLayerId::Video(paint_id) => Some(*paint_id),
                CompositingLayerId::Navigable(_) | CompositingLayerId::Canvas(_) => None,
            })
            .collect();
        self.gpu.retain_video_paints(&live_videos);
        if self.buffers.len() != before {
            debug!(
                "[iosurface] released {} layer buffers (live={})",
                before - self.buffers.len(),
                self.buffers.len()
            );
        }
    }
}
