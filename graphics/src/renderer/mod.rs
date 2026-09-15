//! Per-webview GPU renderers behind a common [`SurfaceRenderer`] trait. The
//! graphics process event loop operates on the renderer only through this
//! trait; the concrete implementation is chosen at compile time by the
//! surface backend features (see graphics/README.md): the CPU readback
//! backend (`renderer/cpu.rs`) off macOS and with the `cpu_readback`
//! feature, the zero-copy IOSurface backend (`renderer/iosurface.rs`) on
//! macOS by default.
//!
//! Each backend defines its own [`SurfaceRenderer::RenderData`] associated
//! type — the per-frame payload produced at submit time and consumed by
//! [`SurfaceRenderer::handle_render_done`] when the GPU completes the frame.
//! The shared [`GpuContext`] holds what every backend needs (the wgpu
//! device, the Vello renderer, the video texture machinery); each backend
//! owns its surface buffers ([`SurfaceBuffers`]) and its delivery path.

use anyrender::PaintScene;
use ipc_messages::content::{FrameId, WebviewId};
use ipc_messages::graphics::{ChildViewport, CompositingLayerId, FrameHitInfo, GraphicsEvent};
use ipc_messages::media::VideoPaintId;
use kurbo::Affine;
use std::collections::{HashMap, HashSet};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::ProtocolObject;
#[cfg(target_os = "macos")]
use objc2_core_video::CVPixelBuffer;
#[cfg(target_os = "macos")]
use objc2_metal::MTLDevice;
use vello::{
    AaConfig, AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions,
    Scene as VelloScene,
};
use wgpu::{Texture, TextureViewDescriptor};

use crate::ComposedScene;

/// Frame metadata captured at submit time and delivered with the frame when
/// the GPU completes it.
#[derive(Clone)]
pub struct FrameMetadata {
    pub webview_id: WebviewId,
    pub frame_hit_info: Vec<FrameHitInfo>,
    pub child_viewports: Vec<ChildViewport>,
    pub child_frame_to_webview: HashMap<FrameId, WebviewId>,
    pub animating: bool,
    /// The composed frames (the top-level frame and embedded child frames)
    /// that carry the animating flag; the UA notes rendering opportunities
    /// for these navigables.
    pub animating_frame_ids: Vec<FrameId>,
}

/// The outcome of delivering a completed frame, reported to the run loop so
/// the TLA tracing stays there. `graphics_computed` gates the
/// `GraphicsComputed` RenderingOpportunity trace event (PixelFrameReady was
/// actually sent).
pub struct FrameDelivery {
    /// PixelFrameReady was delivered to the UA (the frame is fully computed).
    pub graphics_computed: bool,
}

/// The outcome of submitting a composed scene.
#[derive(Debug)]
pub enum RenderError {
    /// Buffer allocation or GPU failure; the scene was dropped.
    Failed,
}

/// A request for the GPU poll thread to block until the given device
/// submission completes. The readback map callbacks (CPU path) fire there
/// and deliver the backend's `RenderData`; when `done` is present
/// (zero-copy path, which has no map callback) the poll thread delivers it
/// after the poll.
pub struct PollRequest<D> {
    pub device: wgpu_context::DeviceHandle,
    /// The submission to wait for; `None` waits for all submitted work
    /// (used by the shared-texture path, where Vello submits internally).
    pub submission_index: Option<wgpu::SubmissionIndex>,
    /// Zero-copy path: delivered after the GPU work completes.
    pub done: Option<D>,
}

/// The channels connecting the renderers to the GPU poll thread and the
/// main loop, created once at graphics-process startup. `D` is the surface
/// backend's `RenderData`.
pub struct ReadbackChannels<D> {
    /// Requests for the poll thread to block on a device submission.
    pub poll_tx: crossbeam_channel::Sender<PollRequest<D>>,
    /// Completed frames: delivered by the readback map callbacks (CPU path)
    /// and by the poll thread (zero-copy path).
    pub render_done_tx: crossbeam_channel::Sender<D>,
}

impl<D> ReadbackChannels<D> {
    /// Create the channels plus the receivers, for the poll thread and the
    /// main loop.
    pub fn new() -> (
        Self,
        crossbeam_channel::Receiver<PollRequest<D>>,
        crossbeam_channel::Receiver<D>,
    ) {
        let (poll_tx, poll_rx) = crossbeam_channel::unbounded();
        let (render_done_tx, render_done_rx) = crossbeam_channel::unbounded();
        (
            Self {
                poll_tx,
                render_done_tx,
            },
            poll_rx,
            render_done_rx,
        )
    }
}

impl<D> Clone for ReadbackChannels<D> {
    fn clone(&self) -> Self {
        Self {
            poll_tx: self.poll_tx.clone(),
            render_done_tx: self.render_done_tx.clone(),
        }
    }
}

/// The shared per-webview GPU state every surface backend needs: the wgpu
/// device, the Vello renderer, the video texture machinery, and the frame
/// generation counter. Backend-specific state (surface buffers, staging
/// buffers) lives on the concrete renderers.
pub(crate) struct GpuContext {
    pub(crate) device_handle: wgpu_context::DeviceHandle,
    vello_renderer: VelloRenderer,
    vello_scene: VelloScene,
    pub(crate) generation: u64,
    #[cfg(target_os = "macos")]
    video: video::VideoTextures,
}

impl GpuContext {
    pub(crate) fn new() -> Result<Self, String> {
        let features = wgpu::Features::CLEAR_TEXTURE | wgpu::Features::PIPELINE_CACHE;
        let context = wgpu_context::WGPUContext::with_features_and_limits(Some(features), None);
        let device_handle = pollster::block_on(context.create_device_handle(None))
            .map_err(|e| format!("failed to create wgpu device: {e}"))?;

        let vello_renderer = VelloRenderer::new(
            &device_handle.device,
            RendererOptions {
                use_cpu: false,
                num_init_threads: None,
                antialiasing_support: AaSupport::area_only(),
                pipeline_cache: None,
            },
        )
        .map_err(|e| format!("failed to create Vello renderer: {e}"))?;

        Ok(Self {
            device_handle,
            vello_renderer,
            vello_scene: VelloScene::new(),
            generation: 0,
            #[cfg(target_os = "macos")]
            video: video::VideoTextures::new(),
        })
    }

    /// The raw Metal device backing this renderer (macOS), needed to create
    /// IOSurface-backed Metal textures and the video Metal texture cache.
    #[cfg(target_os = "macos")]
    pub(crate) fn raw_metal_device(&self) -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
        // SAFETY: the hal device is this renderer's own device; the returned
        // raw Metal device is used only to create textures on it.
        let hal_device = unsafe { self.device_handle.device.as_hal::<wgpu::hal::metal::Api>() }?;
        Some(hal_device.raw_device().clone())
    }

    /// Paint `scene` into `target` with Vello's compute renderer, after
    /// importing (blitting) the pending video frames in their own
    /// submission: two submits back to back, the blit then Vello's render
    /// (GPU execution order guarantees the blit completes before the
    /// render reads it). Blits run only for paints whose stored raw frame
    /// is newer than the last imported one; unchanged frames reuse their
    /// RGBA texture.
    pub(crate) fn render_into(
        &mut self,
        scene: &anyrender::Scene,
        target: &Texture,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        self.import_video_frames();

        self.vello_scene.reset();
        {
            let mut painter = anyrender_vello::VelloScenePainter::new(&mut self.vello_scene);
            painter.append_scene(scene.clone(), Affine::IDENTITY);
        }

        let view = target.create_view(&TextureViewDescriptor::default());
        self.vello_renderer
            .render_to_texture(
                &self.device_handle.device,
                &self.device_handle.queue,
                &self.vello_scene,
                &view,
                &RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| format!("Vello render failed: {e:?}"))
    }

    /// Blit the pending video frames (macOS) in their own submission, right
    /// before Vello's render submits. Every `queue.submit` on the main
    /// thread blocks on the gpu poll thread's fence lock until its current
    /// `device.poll(Wait)` finishes; two back-to-back submits per composed
    /// frame are accepted — the win is that the media event path no longer
    /// submits at all, and frames that are never composited are never
    /// blitted.
    #[cfg(target_os = "macos")]
    fn import_video_frames(&mut self) {
        let Some(raw_device) = self.raw_metal_device() else {
            return;
        };
        let mut encoder =
            self.device_handle
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("video-import"),
                });
        if self.video.record_imports(
            &mut encoder,
            &self.device_handle.device,
            &mut self.vello_renderer,
            Some(&raw_device),
        ) {
            self.device_handle.queue.submit([encoder.finish()]);
        }
    }

    /// Store a video frame for `paint_id` without touching the GPU: the
    /// media callback keeps the latest decoded pixel buffer plus its
    /// metadata here, and the compose-time import (blit) happens in
    /// [`render_into`](Self::render_into). Returns the fake `ImageData`
    /// (empty blob, real size) composed scenes embed as a plain image
    /// brush; its blob is registered with Vello's `override_image` when
    /// the frame is imported.
    #[cfg(target_os = "macos")]
    pub(crate) fn store_video_frame(
        &mut self,
        paint_id: VideoPaintId,
        pixel_buffer: &Retained<CVPixelBuffer>,
        width: u32,
        height: u32,
    ) -> Option<peniko::ImageData> {
        self.video
            .store_frame(paint_id, pixel_buffer, width, height)
    }

    /// Drop the video textures, stored frames, and Vello override
    /// registrations for the paint ids the compositor no longer holds, so a
    /// removed video's GPU targets and pixel buffers do not accumulate over
    /// the renderer's lifetime.
    #[cfg(target_os = "macos")]
    pub(crate) fn retain_video_paints(&mut self, live: &HashSet<VideoPaintId>) {
        self.video.retain(live, &mut self.vello_renderer);
    }
}

/// The alternating double-buffer lifecycle shared by both backends: each
/// render cycle renders into the buffer the last render did not use. The
/// chosen buffer therefore holds the frame from two cycles ago, which the
/// embedder has long since consumed.
pub(crate) struct SurfaceRingState {
    /// Index of the buffer the most recent render used; the next render
    /// uses the other one (buffer 0 on the first frame after allocation).
    last_used: Option<usize>,
    width: u32,
    height: u32,
}

impl SurfaceRingState {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            last_used: None,
            width,
            height,
        }
    }

    /// The buffer to render into this cycle: the one not used by the last
    /// render.
    pub(crate) fn next_buffer(&mut self) -> usize {
        let next = match self.last_used {
            None => 0,
            Some(last_used) => 1 - last_used,
        };
        self.last_used = Some(next);
        next
    }
}

/// The per-webview frame buffers for one surface backend: the shared ring
/// lifecycle plus the backend's buffer payloads (shared-memory regions on
/// the CPU path, IOSurface textures on the zero-copy path).
pub(crate) struct SurfaceBuffers<P> {
    ring: SurfaceRingState,
    payload: P,
}

impl<P> SurfaceBuffers<P> {
    pub(crate) fn new(ring: SurfaceRingState, payload: P) -> Self {
        Self { ring, payload }
    }

    pub(crate) fn ring(&self) -> &SurfaceRingState {
        &self.ring
    }

    pub(crate) fn payload(&self) -> &P {
        &self.payload
    }

    /// Mutable payload access; only the CPU readback backend writes into its
    /// shared-memory regions when a readback completes.
    #[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
    pub(crate) fn payload_mut(&mut self) -> &mut P {
        &mut self.payload
    }

    /// The buffer to render into this cycle: the one not used by the last
    /// render.
    pub(crate) fn next_buffer(&mut self) -> usize {
        self.ring.next_buffer()
    }
}

/// The maximum surface dimension accepted from content-claimed viewport
/// dimensions. Content is the least-trusted process and reports the viewport
/// it renders at; without an upper bound a buggy or compromised content
/// process could claim an arbitrarily large viewport and drive huge
/// graphics-process allocations (shared memory, IOSurfaces, intermediate
/// textures). Far above any real display size (8K is 7680×4320).
pub(crate) const MAX_SURFACE_DIMENSION: u32 = 8192;

/// Build the frame metadata delivered with the completed frame, converting
/// the child viewport map to the wire `ChildViewport` list.
pub(crate) fn frame_metadata(
    webview_id: WebviewId,
    frame_hit_info: Vec<FrameHitInfo>,
    child_viewports: HashMap<WebviewId, [f64; 4]>,
    child_frame_to_webview: HashMap<FrameId, WebviewId>,
    animating: bool,
    animating_frame_ids: Vec<FrameId>,
) -> FrameMetadata {
    let child_ports: Vec<ChildViewport> = child_viewports
        .into_iter()
        .map(|(child_webview_id, root_clip_bounds)| ChildViewport {
            child_webview_id,
            root_clip_bounds,
        })
        .collect();
    FrameMetadata {
        webview_id,
        frame_hit_info,
        child_viewports: child_ports,
        child_frame_to_webview,
        animating,
        animating_frame_ids,
    }
}

/// A per-webview GPU renderer: submits a composed scene's render and, when
/// the GPU completes it, delivers the pixels to the embedder. The associated
/// `RenderData` is the per-frame payload produced at submit time and
/// consumed by [`handle_render_done`](Self::handle_render_done); each
/// surface backend provides its own implementation (CPU readback + shared
/// memory, or zero-copy IOSurface).
pub trait SurfaceRenderer {
    /// Per-frame data produced at submit time, consumed at GPU completion.
    type RenderData: Send + 'static;

    /// Create a renderer for one webview: the wgpu device, Vello renderer,
    /// and readback plumbing.
    fn new(channels: ReadbackChannels<Self::RenderData>) -> Result<Self, String>
    where
        Self: Sized;

    /// Submit `composed` for per-layer rendering. Each layer with a `render`
    /// scene is rasterized into its own surface; clean layers (render `None`)
    /// are skipped and keep their last surface. Returns the layer ids that
    /// were actually re-rendered, so the caller can clear their dirty flags.
    /// The GPU completion is delivered on `ReadbackChannels::render_done_tx`
    /// as `Self::RenderData`. When nothing was re-rendered (every layer
    /// clean), `sender` is used to emit a surface-less `PixelFrameReady` so
    /// the UA still learns the composition completed.
    fn submit_layers(
        &mut self,
        composed: ComposedScene,
        sender: &ipc::IpcSender<GraphicsEvent>,
    ) -> Result<Vec<CompositingLayerId>, RenderError>;

    /// The GPU completed a frame: deliver
    /// the frame to the embedder. The run loop emits the `GraphicsComputed`
    /// RenderingOpportunity trace event from the returned
    /// [`FrameDelivery`].
    fn handle_render_done(
        &mut self,
        data: Self::RenderData,
        sender: &ipc::IpcSender<GraphicsEvent>,
    ) -> FrameDelivery;

    /// The webview a completed frame belongs to (used to look up the
    /// webview's renderer).
    fn render_done_webview_id(data: &Self::RenderData) -> WebviewId;

    /// Store a video frame for `paint_id` without touching the GPU: the
    /// media callback keeps the latest decoded pixel buffer plus its
    /// metadata here, and the compose-time import (blit) happens in
    /// `submit_scene`. Returns the fake `ImageData` (empty blob, real
    /// size) composed scenes embed as a plain image brush.
    #[cfg(target_os = "macos")]
    fn store_video_frame(
        &mut self,
        paint_id: VideoPaintId,
        pixel_buffer: &Retained<CVPixelBuffer>,
        width: u32,
        height: u32,
    ) -> Option<peniko::ImageData>;

    /// Drop the surfaces of layers the compositor no longer holds. `live`
    /// is the compositor's current layer set; every other layer's buffers
    /// are released (a navigated-away frame, a torn-down child, a removed
    /// video or canvas). Layers that are merely offscreen stay in the live
    /// set and keep their buffers, so scrolling does not reallocate them.
    fn retain_layers(&mut self, live: &HashSet<CompositingLayerId>);
}

#[cfg(target_os = "macos")]
mod video;

/// The CPU readback backend: renders into an intermediate texture and ships
/// pixels through the webview's shared-memory ring. The backend off macOS
/// (GStreamer media backend) and on macOS when built with `cpu_readback`.
#[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
pub(crate) mod cpu;
/// The zero-copy IOSurface backend (macOS default): renders directly into a
/// shared IOSurface texture.
#[cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
mod iosurface;

#[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
pub use cpu::{CpuRenderData, CpuRenderer};
#[cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
pub use iosurface::{IosurfaceRenderer, SharedRenderData};
