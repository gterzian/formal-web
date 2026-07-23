//! GPU renderer — renders scenes to a CPU-readable RGBA8 buffer via Vello.
//! Vello renders to an intermediate GPU texture (STORAGE_BINDING), then a
//! GPU → CPU readback copies the pixels to a staging buffer.  The pixel data
//! is shipped to the embedder via IPC shared memory.
//!
//! Cross-process IOSurface sharing is not viable on modern macOS
//! (IOSurfaceLookup is deprecated/inoperative cross-process, and Mach-port
//! bootstrap registration is unreliable).  See graphics/README.md.

use anyrender::PaintScene;
use kurbo::Affine;
use log::{debug, error};
use std::collections::HashMap;

use vello::{
    AaConfig, AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions,
    Scene as VelloScene,
};
use wgpu::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, MapMode,
    Origin3d, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor,
};

pub struct GpuRenderer {
    device_handle: wgpu_context::DeviceHandle,
    vello_renderer: VelloRenderer,
    vello_scene: VelloScene,
    /// Intermediate texture for Vello compute (has STORAGE_BINDING + COPY_SRC).
    render_tex: Option<(Texture, u32, u32)>,
    /// Staging buffer for GPU → CPU readback.
    readback_buffer: Option<(wgpu::Buffer, u32, u32)>,
    generation: u64,
}

impl GpuRenderer {
    pub fn new() -> Result<Self, String> {
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
            render_tex: None,
            readback_buffer: None,
            generation: 0,
        })
    }

    fn ensure_render_tex(&mut self, width: u32, height: u32) {
        if self
            .render_tex
            .as_ref()
            .map(|(_, w, h)| *w == width && *h == height)
            .unwrap_or(false)
        {
            return;
        }
        let tex = self
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

    fn ensure_readback_buffer(&mut self, width: u32, height: u32) -> Option<&wgpu::Buffer> {
        Self::ensure_readback_buffer_inner(
            &mut self.readback_buffer,
            &self.device_handle,
            width,
            height,
        )
    }

    fn ensure_readback_buffer_inner<'a>(
        readback_buffer: &'a mut Option<(wgpu::Buffer, u32, u32)>,
        device_handle: &wgpu_context::DeviceHandle,
        width: u32,
        height: u32,
    ) -> Option<&'a wgpu::Buffer> {
        let size = (width * height * 4) as u64;
        // Check if existing buffer matches size (drop the borrow before mutation).
        let needs_new = match readback_buffer {
            Some((_, w, h)) => *w != width || *h != height,
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

    /// Render a scene and read back RGBA pixels to a CPU buffer.
    /// Returns (iosurface_id, generation, pixels).
    pub fn render_scene(
        &mut self,
        scene: &anyrender::Scene,
        width: u32,
        height: u32,
    ) -> Option<(u32, u64, Vec<u8>)> {
        let (width, height) = (width.max(1), height.max(1));
        self.ensure_render_tex(width, height);

        // Step 1: Vello compute render into intermediate texture.
        self.vello_scene.reset();
        {
            let mut painter = anyrender_vello::VelloScenePainter::new(&mut self.vello_scene);
            painter.append_scene(scene.clone(), Affine::IDENTITY);
        }

        let view = self
            .render_tex
            .as_ref()
            .map(|(tex, _, _)| tex.create_view(&TextureViewDescriptor::default()))?;

        if let Err(e) = self.vello_renderer.render_to_texture(
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
        ) {
            error!("[gpu-renderer] Vello render failed: {:?}", e);
            return None;
        }

        // Step 2: Read back GPU texture to CPU staging buffer.
        // Destructure self to avoid borrow conflicts.
        let device_handle = &self.device_handle;
        let readback_buffer = &mut self.readback_buffer;
        let render_tex = &self.render_tex;
        let readback_buf = Self::ensure_readback_buffer_inner(readback_buffer, device_handle, width, height)?;
        let mut encoder =
            device_handle
                .device
                .create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("surface-readback"),
                });
        let (src_tex, _, _) = render_tex.as_ref()?;
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
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        device_handle.queue.submit([encoder.finish()]);

        // Wait for GPU work to finish so we can map the buffer.
        let buf_slice = readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buf_slice.map_async(MapMode::Read, move |r: Result<(), wgpu::BufferAsyncError>| {
            let _ = tx.send(r);
        });
        let _ = device_handle.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        if rx.recv().is_err() {
            return None;
        }
        let data = buf_slice.get_mapped_range();
        let pixels = data.to_vec();
        drop(data);
        readback_buf.unmap();

        self.generation += 1;
        debug!(
            "[gpu-renderer] rendered {}x{} gen={} pixels={}B",
            width, height, self.generation, pixels.len(),
        );
        Some((0, self.generation, pixels))
    }
}
