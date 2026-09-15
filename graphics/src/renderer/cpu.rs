//! CPU readback surface backend: renders Vello into an intermediate texture,
//! submits a GPU → CPU readback, and copies the pixels into the webview's
//! shared-memory ring once the readback completes. This is the backend off
//! macOS (GStreamer media backend) and on macOS when built with the
//! `cpu_readback` feature.

use super::{
    FrameDelivery, FrameMetadata, GpuContext, MAX_SURFACE_DIMENSION, PollRequest, ReadbackChannels,
    RenderError, SurfaceBuffers, SurfaceRenderer, SurfaceRingState, frame_metadata,
};
use ipc_messages::content::WebviewId;
use ipc_messages::graphics::{CompositingLayerId, GraphicsEvent, LayerTopology, SurfacePayload};
use log::{debug, error, info};
use std::collections::{HashMap, HashSet};
use wgpu::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, Origin3d,
    TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

#[cfg(target_os = "macos")]
use ipc_messages::media::VideoPaintId;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_core_video::CVPixelBuffer;

use crate::ComposedScene;

/// The number of readback buffers kept per renderer; must be >= the number
/// of shared-memory surface buffers so each in-flight frame has its own
/// staging buffer.
pub const READBACK_SLOTS: usize = 2;

/// Per-frame data for the CPU readback path: delivered by the readback map
/// callback when the GPU completes the copy. Each message is one layer's
/// readback; the renderer accumulates them per cycle and emits one
/// `PixelFrameReady` when the last one lands.
pub struct CpuRenderData {
    pub webview_id: WebviewId,
    pub generation: u64,
    pub layer_id: CompositingLayerId,
    pub width: u32,
    pub height: u32,
    pub shmem_index: usize,
    pub readback_index: usize,
    pub result: Result<(), wgpu::BufferAsyncError>,
}

/// Accumulates one render cycle's per-layer readbacks so the CPU path emits a
/// single `PixelFrameReady` once every layer submitted this cycle has
/// finished.
struct CycleAccumulator {
    webview_id: WebviewId,
    generation: u64,
    /// Total layers submitted this cycle (readbacks outstanding).
    total: usize,
    /// How many readbacks have completed so far.
    received: usize,
    /// Topology in submission order; an entry's `surface` stays `Some` only
    /// if its readback completed successfully.
    layers: Vec<LayerTopology>,
    /// Shared-memory regions collected, keyed by shmem_key, for the send.
    shmem_regions: HashMap<usize, ipc::IpcSharedRegion>,
    metadata: FrameMetadata,
}

/// The CPU readback renderer: a [`GpuContext`] plus the intermediate
/// texture, the readback staging pool, the per-layer shared-memory rings,
/// and the per-cycle accumulator.
pub struct CpuRenderer {
    gpu: GpuContext,
    channels: ReadbackChannels<CpuRenderData>,
    /// Intermediate texture for Vello compute (has STORAGE_BINDING +
    /// COPY_SRC); the CPU readback source.
    render_tex: Option<(wgpu::Texture, u32, u32)>,
    /// Staging buffers for GPU → CPU readback, one per in-flight frame.
    readback_buffers: [Option<(wgpu::Buffer, u32, u32)>; READBACK_SLOTS],
    /// Generation of the frame whose readback is in flight per slot.
    inflight_readbacks: [Option<u64>; READBACK_SLOTS],
    /// Per-layer shared-memory double buffers (two regions each), reallocated
    /// on resize.
    buffers: HashMap<CompositingLayerId, SurfaceBuffers<[ipc::IpcSharedRegion; 2]>>,
    /// Per-cycle accumulation: outstanding layer readbacks and the topology
    /// collected so far, flushed to one PixelFrameReady when the last layer
    /// readback completes.
    pending_cycle: Option<CycleAccumulator>,
}

impl CpuRenderer {
    fn ensure_render_tex(&mut self, width: u32, height: u32) {
        if self
            .render_tex
            .as_ref()
            .map(|(_, render_width, render_height)| {
                *render_width == width && *render_height == height
            })
            .unwrap_or(false)
        {
            return;
        }
        let tex = self
            .gpu
            .device_handle
            .device
            .create_texture(&TextureDescriptor {
                label: Some("vello-intermediate"),
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::STORAGE_BINDING
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        self.render_tex = Some((tex, width, height));
    }

    /// Two shared-memory pixel buffers for the double buffer, sized for
    /// `width`×`height` RGBA8.
    fn allocate_shmem(width: u32, height: u32) -> Result<[ipc::IpcSharedRegion; 2], ipc::IpcError> {
        let byte_count = (width as usize) * (height as usize) * 4;
        let region_zero = ipc::IpcSharedRegion::allocate(byte_count)?;
        let region_one = ipc::IpcSharedRegion::allocate(byte_count)?;
        Ok([region_zero, region_one])
    }

    /// Drop the in-flight marker for a readback slot and unmap its staging
    /// buffer. The abort paths (unknown cycle, missing layer buffers, bad
    /// shmem index, map failure) reach here without going through
    /// `copy_readback`, which is the only other place the buffer is
    /// unmapped; a slot freed with its buffer still mapped makes the next
    /// `copy_texture_to_buffer` into that buffer fail with "buffer is still
    /// mapped".
    fn release_readback(
        inflight_readbacks: &mut [Option<u64>; READBACK_SLOTS],
        readback_buffers: &[Option<(wgpu::Buffer, u32, u32)>; READBACK_SLOTS],
        readback_index: usize,
    ) {
        if let Some(generation) = inflight_readbacks[readback_index].take() {
            if let Some((buffer, _, _)) = &readback_buffers[readback_index] {
                buffer.unmap();
            }
            debug!(
                "[gpu-renderer] released readback slot {} gen={}",
                readback_index, generation
            );
        }
    }

    /// Copy the completed readback's pixels into `pixels` (tightly packed,
    /// `width * height * 4` bytes) and release the readback slot.
    /// Returns false when the slot is not in flight.
    fn copy_readback(
        inflight_readbacks: &mut [Option<u64>; READBACK_SLOTS],
        readback_buffers: &mut [Option<(wgpu::Buffer, u32, u32)>; READBACK_SLOTS],
        readback_index: usize,
        pixels: &mut [u8],
        width: u32,
        height: u32,
    ) -> bool {
        let Some(generation) = inflight_readbacks[readback_index].take() else {
            error!(
                "[gpu-renderer] readback slot {} not in flight",
                readback_index
            );
            return false;
        };
        let Some((buf, _, _)) = &readback_buffers[readback_index] else {
            error!(
                "[gpu-renderer] readback slot {} has no buffer",
                readback_index
            );
            return false;
        };
        let data = buf.slice(..).get_mapped_range();
        // Strip alignment padding — write only the actual pixel data into
        // the destination slice, which is tightly packed (width * 4 bytes
        // per row).
        let pixel_count = (width * height * 4) as usize;
        if pixels.len() < pixel_count {
            error!(
                "[gpu-renderer] destination too small: {}B for {}x{} (need {}B)",
                pixels.len(),
                width,
                height,
                pixel_count
            );
            drop(data);
            buf.unmap();
            return false;
        }
        let padded_bytes_per_row = ((width * 4) as usize).div_ceil(256) * 256;
        let row_bytes = (width * 4) as usize;
        if padded_bytes_per_row == row_bytes {
            pixels[..pixel_count].copy_from_slice(&data[..pixel_count]);
        } else {
            for (row_index, row) in data.chunks(padded_bytes_per_row).enumerate() {
                let start = row_index * row_bytes;
                pixels[start..start + row_bytes].copy_from_slice(&row[..row_bytes]);
            }
        }
        drop(data);
        buf.unmap();
        debug!(
            "[gpu-renderer] readback complete slot={} gen={} pixels={}B",
            readback_index, generation, pixel_count
        );
        true
    }

    fn ensure_readback_buffer<'a>(
        readback_buffer: &'a mut Option<(wgpu::Buffer, u32, u32)>,
        device_handle: &wgpu_context::DeviceHandle,
        width: u32,
        height: u32,
    ) -> Option<&'a wgpu::Buffer> {
        // bytes_per_row must be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256).
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes_per_row = (width * 4).div_ceil(alignment) * alignment;
        let size = (bytes_per_row * height) as u64;
        let needs_new = match readback_buffer {
            Some((_, buffer_width, buffer_height)) => {
                *buffer_width != width || *buffer_height != height
            }
            None => true,
        };
        if !needs_new {
            return readback_buffer.as_ref().map(|(b, _, _)| b);
        }
        let buf = device_handle.device.create_buffer(&BufferDescriptor {
            label: Some("surface-readback"),
            size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        *readback_buffer = Some((buf, width, height));
        readback_buffer.as_ref().map(|(b, _, _)| b)
    }
}

impl SurfaceRenderer for CpuRenderer {
    type RenderData = CpuRenderData;

    fn new(channels: ReadbackChannels<CpuRenderData>) -> Result<Self, String> {
        Ok(Self {
            gpu: GpuContext::new()?,
            channels,
            render_tex: None,
            readback_buffers: [None, None],
            inflight_readbacks: [None, None],
            buffers: HashMap::new(),
            pending_cycle: None,
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
            "[render-pipe] Graphics CPU submit layers webview={} layers={} child_frames={} animating={}",
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
        let mut total = 0;

        for layer in layers {
            let Some(ref scene) = layer.render else {
                // Clean layer: keep its last surface, still report topology.
                topology.push(layer.into_layer_topology());
                continue;
            };
            let layer_id = layer.layer_id;
            let width = layer.width.clamp(1, MAX_SURFACE_DIMENSION);
            let height = layer.height.clamp(1, MAX_SURFACE_DIMENSION);

            // The intermediate render target must match this layer's size
            // before the buffers borrow below.
            self.ensure_render_tex(width, height);

            let needs_new = self.buffers.get(&layer_id).is_none_or(|buffers| {
                buffers.ring().width != width || buffers.ring().height != height
            });
            if needs_new {
                let payload = Self::allocate_shmem(width, height).map_err(|error| {
                    error!(
                        "[graphics] allocate surface shmem {}x{}: {error}",
                        width, height
                    );
                    RenderError::Failed
                })?;
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

            // Vello render into the intermediate texture.
            let (src_tex, _, _) = self.render_tex.as_ref().ok_or(RenderError::Failed)?;
            if let Err(error) = self.gpu.render_into(scene, src_tex, width, height) {
                error!("[gpu-renderer] {error}");
                return Err(RenderError::Failed);
            }

            // Submit the readback into a staging buffer.
            let device_handle = &self.gpu.device_handle;
            let Some(readback_index) =
                (0..READBACK_SLOTS).find(|index| self.inflight_readbacks[*index].is_none())
            else {
                error!(
                    "[gpu-renderer] no free readback slot for {}x{}",
                    width, height
                );
                return Err(RenderError::Failed);
            };
            let readback_buf = Self::ensure_readback_buffer(
                &mut self.readback_buffers[readback_index],
                device_handle,
                width,
                height,
            )
            .ok_or(RenderError::Failed)?;
            let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let aligned_bytes_per_row = (width * 4).div_ceil(alignment) * alignment;
            let aligned_size = aligned_bytes_per_row * height;

            let mut encoder =
                device_handle
                    .device
                    .create_command_encoder(&CommandEncoderDescriptor {
                        label: Some("surface-readback"),
                    });
            let (src_tex, _, _) = self.render_tex.as_ref().ok_or(RenderError::Failed)?;
            encoder.copy_texture_to_buffer(
                TexelCopyTextureInfo {
                    texture: src_tex,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                TexelCopyBufferInfo {
                    buffer: readback_buf,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(aligned_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            self.gpu.generation += 1;
            let generation = self.gpu.generation;
            let shmem_index = buffer_index;
            // The map is scheduled to complete after this submission finishes
            // on the GPU; the callback fires on the poll thread and delivers
            // this layer's completed readback to the main loop.
            let render_done_tx = self.channels.render_done_tx.clone();
            encoder.map_buffer_on_submit(
                readback_buf,
                wgpu::MapMode::Read,
                0..aligned_size as u64,
                move |result| {
                    if let Err(send_error) = render_done_tx.send(CpuRenderData {
                        webview_id,
                        generation,
                        layer_id,
                        width,
                        height,
                        shmem_index,
                        readback_index,
                        result,
                    }) {
                        error!("[gpu-renderer] failed to deliver readback ready: {send_error}");
                    }
                },
            );
            let submission_index = self.gpu.device_handle.queue.submit([encoder.finish()]);
            // Ask the poll thread to block until this submission completes.
            if let Err(send_error) = self.channels.poll_tx.send(PollRequest {
                device: self.gpu.device_handle.clone(),
                submission_index: Some(submission_index),
                done: None,
            }) {
                error!("[gpu-renderer] failed to queue poll request: {send_error}");
            }
            self.inflight_readbacks[readback_index] = Some(generation);

            topology.push(
                layer.into_layer_topology_with_surface(SurfacePayload::CpuShmem {
                    shmem_key: generation as usize,
                }),
            );
            rendered.push(layer_id);
            total += 1;
        }

        if total > 0 {
            self.pending_cycle = Some(CycleAccumulator {
                webview_id,
                generation: self.gpu.generation,
                total,
                received: 0,
                layers: topology,
                shmem_regions: HashMap::new(),
                metadata,
            });
        } else {
            // Nothing was re-rendered this cycle (every layer clean): emit a
            // surface-less PixelFrameReady so the UA still learns the
            // composition completed and clears the navigable's pending
            // update-the-rendering. The embedder keeps its last surfaces.
            let frame_event = GraphicsEvent::PixelFrameReady {
                webview_id,
                layers: topology,
                animating: metadata.animating,
                animating_frame_ids: metadata.animating_frame_ids,
                generation: self.gpu.generation,
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
        }
        Ok(rendered)
    }

    fn handle_render_done(
        &mut self,
        data: CpuRenderData,
        sender: &ipc::IpcSender<GraphicsEvent>,
    ) -> FrameDelivery {
        let CpuRenderData {
            webview_id,
            generation,
            layer_id,
            width,
            height,
            shmem_index,
            readback_index,
            result,
        } = data;
        let mut delivery = FrameDelivery {
            graphics_computed: false,
        };
        let Some(cycle) = self.pending_cycle.as_mut() else {
            error!(
                "[graphics] readback for unknown cycle {:?} gen={}",
                webview_id, generation
            );
            Self::release_readback(
                &mut self.inflight_readbacks,
                &self.readback_buffers,
                readback_index,
            );
            return delivery;
        };
        if cycle.generation != generation {
            error!(
                "[graphics] readback for stale cycle {:?} gen={} (current {})",
                webview_id, generation, cycle.generation
            );
            Self::release_readback(
                &mut self.inflight_readbacks,
                &self.readback_buffers,
                readback_index,
            );
            return delivery;
        }

        let mut usable = false;
        if let Err(error) = result {
            error!(
                "[graphics] readback map failed for {:?} gen={}: {error:?}",
                webview_id, generation
            );
            Self::release_readback(
                &mut self.inflight_readbacks,
                &self.readback_buffers,
                readback_index,
            );
        } else if let Some(buffers) = self.buffers.get_mut(&layer_id) {
            if let Some(region) = buffers.payload_mut().get_mut(shmem_index) {
                // SAFETY: this buffer was reserved at submit time and its
                // pixels are delivered exactly once here, before it is marked
                // pending; no other party reads or writes these pages in
                // between.
                let pixel_slice = unsafe { region.as_mut_slice() };
                if Self::copy_readback(
                    &mut self.inflight_readbacks,
                    &mut self.readback_buffers,
                    readback_index,
                    pixel_slice,
                    width,
                    height,
                ) {
                    let shmem_key = generation as usize;
                    cycle
                        .shmem_regions
                        .insert(shmem_key, buffers.payload()[shmem_index].clone());
                    usable = true;
                } else {
                    error!(
                        "[graphics] readback copy failed for {:?} gen={}",
                        webview_id, generation
                    );
                }
            } else {
                error!(
                    "[graphics] bad shmem index {} for layer {:?} gen={}",
                    shmem_index, layer_id, generation
                );
                Self::release_readback(
                    &mut self.inflight_readbacks,
                    &self.readback_buffers,
                    readback_index,
                );
            }
        } else {
            error!(
                "[graphics] no surface buffers for layer {:?} gen={}",
                layer_id, generation
            );
            Self::release_readback(
                &mut self.inflight_readbacks,
                &self.readback_buffers,
                readback_index,
            );
        }

        if !usable && let Some(layer) = cycle.layers.iter_mut().find(|l| l.layer_id == layer_id) {
            layer.surface = None;
        }
        cycle.received += 1;

        if cycle.received == cycle.total {
            let Some(pending) = self.pending_cycle.take() else {
                return delivery;
            };
            if sender
                .send_with_shmem_map(
                    GraphicsEvent::PixelFrameReady {
                        webview_id: pending.webview_id,
                        layers: pending.layers,
                        animating: pending.metadata.animating,
                        animating_frame_ids: pending.metadata.animating_frame_ids,
                        generation: pending.generation,
                        frame_hit_info: pending.metadata.frame_hit_info,
                        child_viewports: pending.metadata.child_viewports,
                        child_frame_to_webview: pending.metadata.child_frame_to_webview,
                    },
                    pending.shmem_regions,
                )
                .is_err()
            {
                error!(
                    "[graphics] failed to send PixelFrameReady for {:?}",
                    pending.webview_id
                );
                return delivery;
            }
            delivery.graphics_computed = true;
        }
        delivery
    }

    fn render_done_webview_id(data: &CpuRenderData) -> WebviewId {
        data.webview_id
    }

    #[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
        {
            let live_videos: HashSet<VideoPaintId> = live
                .iter()
                .filter_map(|layer_id| match layer_id {
                    CompositingLayerId::Video(paint_id) => Some(*paint_id),
                    CompositingLayerId::Navigable(_) | CompositingLayerId::Canvas(_) => None,
                })
                .collect();
            self.gpu.retain_video_paints(&live_videos);
        }
        if self.buffers.len() != before {
            debug!(
                "[cpu-renderer] released {} layer buffers (live={})",
                before - self.buffers.len(),
                self.buffers.len()
            );
        }
    }
}
