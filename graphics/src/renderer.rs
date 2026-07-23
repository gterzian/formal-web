//! GPU renderer — renders scenes to IOSurface-backed textures via Vello.
//! Two-step approach to avoid Metal compute-shader limitations on IOSurface
//! textures: Vello renders to a regular intermediate texture (with
//! STORAGE_BINDING), then a GPU copy_texture_to_texture blit copies the
//! result to the IOSurface-backed export texture (which only needs
//! COPY_DST + RENDER_ATTACHMENT).

use anyrender::PaintScene;
use kurbo::Affine;
use log::{debug, error};
use std::collections::HashMap;

use vello::{AaConfig, AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions,
            Scene as VelloScene};
use wgpu::{Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
           Extent3d, TextureViewDescriptor, CommandEncoderDescriptor};

pub struct GpuRenderer {
    device_handle: wgpu_context::DeviceHandle,
    vello_renderer: VelloRenderer,
    vello_scene: VelloScene,
    /// Intermediate texture for Vello compute (has STORAGE_BINDING).
    render_tex: Option<(Texture, u32, u32)>,
    /// IOSurface export textures: (Texture, IOSurfaceID).
    export_textures: HashMap<(u32, u32), (Texture, u32)>,
    generation: u64,
}

impl GpuRenderer {
    pub fn new() -> Result<Self, String> {
        let features = wgpu::Features::CLEAR_TEXTURE | wgpu::Features::PIPELINE_CACHE;
        let context =
            wgpu_context::WGPUContext::with_features_and_limits(Some(features), None);
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
            export_textures: HashMap::new(),
            generation: 0,
        })
    }

    fn ensure_render_tex(&mut self, width: u32, height: u32) {
        if self.render_tex.as_ref().map(|(_, w, h)| *w == width && *h == height).unwrap_or(false) {
            return;
        }
        let tex = self.device_handle.device.create_texture(&TextureDescriptor {
            label: Some("vello-intermediate"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
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

    fn ensure_export_tex(&mut self, width: u32, height: u32) -> Option<u32> {
        if let Some((_, id)) = self.export_textures.get(&(width, height)) {
            return Some(*id);
        }
        let (surface, iosurface_id) =
            crate::iosurface_surface::allocate_iosurface(width, height)?;
        let tex = crate::iosurface_surface::import_iosurface_as_wgpu_texture(
            &self.device_handle.device,
            &surface,
            width,
            height,
        )?;
        debug!("[gpu-renderer] created export texture {}x{}", width, height);
        self.export_textures.insert((width, height), (tex, iosurface_id));
        Some(iosurface_id)
    }

    /// Render a scene: Vello → intermediate → GPU blit → IOSurface texture.
    /// Returns (IOSurfaceID, generation) for IPC.
    pub fn render_scene(
        &mut self,
        scene: &anyrender::Scene,
        width: u32,
        height: u32,
    ) -> Option<(u32, u64)> {
        let (width, height) = (width.max(1), height.max(1));
        let iosurface_id = self.ensure_export_tex(width, height)?;
        self.ensure_render_tex(width, height);

        // Step 1: Vello compute render into intermediate texture.
        self.vello_scene.reset();
        {
            let mut painter =
                anyrender_vello::VelloScenePainter::new(&mut self.vello_scene);
            painter.append_scene(scene.clone(), Affine::IDENTITY);
        }

        let view = self.render_tex.as_ref()
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

        // Step 2: GPU blit from intermediate to IOSurface export texture.
        let (src_tex, _, _) = self.render_tex.as_ref()?;
        let (dst_tex, _) = self.export_textures.get(&(width, height))?;
        let src = src_tex.as_image_copy();
        let dst = dst_tex.as_image_copy();

        let mut encoder = self.device_handle.device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("gpu-blit"),
            });
        encoder.copy_texture_to_texture(
            src, dst,
            Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.device_handle.queue.submit([encoder.finish()]);

        self.generation += 1;
        debug!(
            "[gpu-renderer] rendered {}x{} surface={} gen={}",
            width, height, iosurface_id, self.generation,
        );
        Some((iosurface_id, self.generation))
    }
}
