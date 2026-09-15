//! macOS video texture import: wraps a CVPixelBuffer as a Metal texture
//! (zero-copy when the buffer is GPU-backed) and blits BGRA → RGBA into a
//! texture Vello can register (Vello's `register_texture` requires
//! Rgba8Unorm + COPY_SRC). Lives behind the renderer trait's
//! `store_video_frame` (macOS); the AVFoundation media backend delivers
//! `PixelBufferFrame` events and the renderer stores each frame's pixel
//! buffer here without touching the GPU.
//!
//! The import is deferred from frame arrival to compose time: the media
//! callback only stores the latest raw frame (pixel buffer + size +
//! generation counter), and `submit_scene` (via `record_imports`) blits
//! exactly the frames whose generation is newer than the last imported one
//! in their own submission, right before Vello's render submits. That keeps
//! the video import off the media event path and turns "one blit per
//! decoded frame" into "one blit per frame that is actually composited".

use ipc_messages::media::VideoPaintId;
use log::error;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer, kCVReturnSuccess,
};
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLTexture};
use peniko::ImageData;
use std::collections::{HashMap, HashSet};
use vello::Renderer as VelloRenderer;
use wgpu::{
    ComputePipelineDescriptor, Extent3d, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor,
};

const BGRA_TO_RGBA_WGSL: &str = r#"
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var dst_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(src_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) {
        return;
    }
    // wgpu presents a bgra8unorm texel in RGBA semantic order (the Metal
    // backend applies the format's channel swizzle), so the copy is direct.
    let c = textureLoad(src_tex, vec2<i32>(gid.xy), 0);
    textureStore(dst_tex, vec2<i32>(gid.xy), c);
}
"#;

/// A decoded video frame stored on arrival, not yet imported: the pixel
/// buffer plus its metadata. The generation counter tags the frame so the
/// compose-time import can skip frames whose contents already reached the
/// RGBA texture.
struct StoredFrame {
    pixel_buffer: Retained<CVPixelBuffer>,
    width: u32,
    height: u32,
    generation: u64,
}

/// The imported GPU state for one paint id: the RGBA conversion target
/// Vello samples, the fake image data the composed scenes reference, and
/// the generation of the raw frame last blitted into the target.
struct ImportedVideoTexture {
    /// RGBA8 conversion target; its contents are what Vello samples.
    /// None before the first compose-time import.
    texture: Option<Texture>,
    /// Fake image data referencing `texture` via Vello's override_image;
    /// composed scenes draw this as a plain `Paint::Image` brush.
    image: ImageData,
    /// An image whose override registration must be dropped: set when a
    /// size change replaced `image` at store time; the old blob id is no
    /// longer referenced by any scene but its override still holds the old
    /// target texture.
    stale_image: Option<ImageData>,
    /// Generation of the raw frame last blitted into `texture`.
    imported_generation: u64,
    /// The target was created but never written (neither blitted nor
    /// cleared): a failed first blit must leave black pixels, not
    /// uninitialized memory, for Vello to sample.
    needs_clear: bool,
}

struct VideoImport {
    /// Metal texture cache for this renderer's device.
    cache: Retained<CVMetalTextureCache>,
    /// The BGRA→RGBA blit pipeline.
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl VideoImport {
    /// Create the Metal texture cache and blit pipeline on this
    /// renderer's device. Requires the raw Metal device from
    /// `wgpu_hal::metal::Device::raw_device`.
    fn new(
        device: &wgpu::Device,
        raw_device: &Retained<ProtocolObject<dyn MTLDevice>>,
    ) -> Option<VideoImport> {
        let mut cache_ptr = std::ptr::null_mut();
        let cache_result = unsafe {
            CVMetalTextureCache::create(
                None,
                None,
                raw_device,
                None,
                std::ptr::NonNull::new(&mut cache_ptr)?,
            )
        };
        if cache_result != kCVReturnSuccess {
            error!("[gpu-renderer] CVMetalTextureCacheCreate failed: {cache_result}");
            return None;
        }
        // SAFETY: the create call above wrote a valid retained cache.
        let cache = unsafe { Retained::from_raw(cache_ptr) }?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bgra-to-rgba"),
            source: wgpu::ShaderSource::Wgsl(BGRA_TO_RGBA_WGSL.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgra-to-rgba-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bgra-to-rgba-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("bgra-to-rgba"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        Some(VideoImport {
            cache,
            pipeline,
            bind_group_layout,
        })
    }

    /// Wrap `pixel_buffer` (BGRA) as a wgpu texture on this device and
    /// record a compute pass blitting it into `target` (RGBA8Unorm
    /// storage) on `encoder`, without submitting. The caller submits the
    /// encoder once, batching this blit with the Vello render. The source
    /// Metal texture references the pixel buffer, which the stored frame
    /// keeps alive until the next frame replaces it.
    fn blit_bgra_into_rgba(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        device: &wgpu::Device,
        pixel_buffer: &Retained<CVPixelBuffer>,
        width: u32,
        height: u32,
        target: &Texture,
    ) -> Option<()> {
        let mut texture_ptr = std::ptr::null_mut();
        let result = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                &self.cache,
                pixel_buffer,
                None,
                MTLPixelFormat::BGRA8Unorm,
                width as usize,
                height as usize,
                0,
                std::ptr::NonNull::new(&mut texture_ptr)?,
            )
        };
        if result != kCVReturnSuccess {
            error!("[gpu-renderer] create_texture_from_image failed: {result}");
            return None;
        }
        // SAFETY: the create call above wrote a valid retained texture.
        let cv_metal_texture: Retained<CVMetalTexture> =
            unsafe { Retained::from_raw(texture_ptr) }?;
        // SAFETY: CVMetalTextureGetTexture returns a +1 retained Metal
        // texture referencing the pixel buffer.
        let raw_texture: Retained<ProtocolObject<dyn MTLTexture>> =
            CVMetalTextureGetTexture(&cv_metal_texture)?;

        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw_texture,
                wgpu::TextureFormat::Bgra8Unorm,
                objc2_metal::MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
            )
        };
        let source = unsafe {
            device.create_texture_from_hal::<wgpu::hal::metal::Api>(
                hal_texture,
                &TextureDescriptor {
                    label: Some("video-bgra-source"),
                    size: Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Bgra8Unorm,
                    usage: TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };

        let source_view = source.create_view(&TextureViewDescriptor::default());
        let target_view = target.create_view(&TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bgra-to-rgba-bind"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&target_view),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("bgra-to-rgba"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        Some(())
    }
}

/// The per-renderer video texture state: the Metal texture cache + blit
/// pipeline, the latest raw frame per paint id (stored on arrival, no GPU
/// work), and the imported RGBA target per paint id. The AVFoundation
/// media backend delivers `PixelBufferFrame` events; each frame's pixel
/// buffer is stored here and blitted at compose time (see
/// [`record_imports`](Self::record_imports)), then registered with the
/// renderer's Vello via `override_image`.
pub(super) struct VideoTextures {
    import: Option<VideoImport>,
    frames: HashMap<VideoPaintId, StoredFrame>,
    textures: HashMap<VideoPaintId, ImportedVideoTexture>,
    next_generation: u64,
}

impl VideoTextures {
    pub(super) fn new() -> Self {
        Self {
            import: None,
            frames: HashMap::new(),
            textures: HashMap::new(),
            next_generation: 1,
        }
    }

    /// Store a decoded frame for `paint_id` without touching the GPU: the
    /// pixel buffer plus its size and a fresh generation tag. Returns the
    /// fake `ImageData` (empty blob, real size) composed scenes embed as a
    /// plain image brush; the same image data is reused while the frame
    /// size is unchanged, and its blob is registered with Vello's
    /// `override_image` when the compose-time import blits the frame.
    pub(super) fn store_frame(
        &mut self,
        paint_id: VideoPaintId,
        pixel_buffer: &Retained<CVPixelBuffer>,
        width: u32,
        height: u32,
    ) -> Option<ImageData> {
        let generation = self.next_generation;
        self.next_generation += 1;
        self.frames.insert(
            paint_id,
            StoredFrame {
                pixel_buffer: pixel_buffer.clone(),
                width,
                height,
                generation,
            },
        );

        let same_size = self
            .textures
            .get(&paint_id)
            .map(|entry| entry.image.width == width && entry.image.height == height);
        match same_size {
            Some(true) => {
                let image = self.textures.get(&paint_id)?.image.clone();
                Some(image)
            }
            Some(false) => {
                // Size changed: a fresh image (new blob id) replaces the
                // old one. The old override registration is dropped when
                // the next import re-registers the new blob.
                let image = fake_image(width, height);
                let entry = self.textures.get_mut(&paint_id)?;
                entry.stale_image = Some(std::mem::replace(&mut entry.image, image.clone()));
                Some(image)
            }
            None => {
                // First frame for this paint: no imported texture yet; the
                // compose-time import creates it and registers the override.
                let image = fake_image(width, height);
                self.textures.insert(
                    paint_id,
                    ImportedVideoTexture {
                        texture: None,
                        image: image.clone(),
                        stale_image: None,
                        imported_generation: 0,
                        needs_clear: false,
                    },
                );
                Some(image)
            }
        }
    }

    /// Drop the stored frames, imported RGBA textures, and Vello override
    /// registrations for the paint ids the compositor no longer holds, so a
    /// removed video's GPU targets and pixel buffers do not accumulate over
    /// the renderer's lifetime. The override registration must be removed
    /// explicitly: the fake image's blob id would otherwise keep sampling a
    /// texture nobody references.
    pub(super) fn retain(
        &mut self,
        live: &HashSet<VideoPaintId>,
        vello_renderer: &mut VelloRenderer,
    ) {
        self.frames.retain(|paint_id, _| live.contains(paint_id));
        self.textures.retain(|paint_id, entry| {
            if live.contains(paint_id) {
                return true;
            }
            vello_renderer.override_image(&entry.image, None);
            if let Some(stale_image) = entry.stale_image.take() {
                vello_renderer.override_image(&stale_image, None);
            }
            false
        });
    }

    /// Blit every stored raw frame whose generation is newer than the last
    /// imported one into `encoder`, recording the BGRA→RGBA compute passes.
    /// The caller submits the encoder right before Vello's render submits,
    /// so the blit completes before the render reads its output (GPU
    /// execution order). Paints with no newer frame are left alone: their
    /// RGBA texture is reused, so a compose without new video does zero GPU
    /// work here (returns `false`). The re-blitted images are marked dirty
    /// so Vello recopies their updated contents into its atlas on the
    /// subsequent render.
    pub(super) fn record_imports(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        device: &wgpu::Device,
        vello_renderer: &mut VelloRenderer,
        raw_device: Option<&Retained<ProtocolObject<dyn MTLDevice>>>,
    ) -> bool {
        let pending: Vec<VideoPaintId> = self
            .frames
            .iter()
            .filter(|(paint_id, frame)| {
                self.textures
                    .get(paint_id)
                    .is_none_or(|imported| frame.generation > imported.imported_generation)
            })
            .map(|(paint_id, _)| *paint_id)
            .collect();
        if pending.is_empty() {
            return false;
        }
        let Some(raw_device) = raw_device else {
            return false;
        };
        let video_import = match &self.import {
            Some(video_import) => video_import,
            None => {
                let Some(video_import) = VideoImport::new(device, raw_device) else {
                    return false;
                };
                self.import = Some(video_import);
                let Some(video_import) = self.import.as_ref() else {
                    return false;
                };
                video_import
            }
        };

        for paint_id in pending {
            let Some(frame) = self.frames.get(&paint_id) else {
                continue;
            };
            let Some(entry) = self.textures.get_mut(&paint_id) else {
                continue;
            };

            // (Re)create the RGBA target when it is missing or the frame
            // resized; a resize replaced the fake image at store time, so
            // the stale image's override registration is dropped here.
            let target_changed = entry.texture.as_ref().is_none_or(|texture| {
                texture.width() != frame.width || texture.height() != frame.height
            });
            if target_changed {
                if let Some(stale_image) = entry.stale_image.take() {
                    vello_renderer.override_image(&stale_image, None);
                }
                let target = device.create_texture(&TextureDescriptor {
                    label: Some("video-rgba"),
                    size: Extent3d {
                        width: frame.width,
                        height: frame.height,
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
                entry.needs_clear = true;
                // Fake image: empty blob, real size. Vello never reads the
                // blob; the override makes it sample `target` instead. The
                // registration also marks the image dirty for this render.
                vello_renderer.override_image(
                    &entry.image,
                    Some(wgpu::TexelCopyTextureInfoBase {
                        texture: target.clone(),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    }),
                );
                entry.texture = Some(target);
            }
            let Some(target) = entry.texture.as_ref() else {
                continue;
            };
            match video_import.blit_bgra_into_rgba(
                encoder,
                device,
                &frame.pixel_buffer,
                frame.width,
                frame.height,
                target,
            ) {
                Some(()) => {
                    entry.needs_clear = false;
                    // The target contents changed; Vello must recopy them
                    // into its atlas on this render.
                    vello_renderer.mark_override_image_dirty(&entry.image);
                }
                None => {
                    error!("[gpu-renderer] video blit failed for {:?}", paint_id);
                    // The source could not be wrapped: leave black pixels
                    // (a browser shows black for a video that fails to
                    // decode) instead of uninitialized memory, and retry
                    // on the next compose (the generation stays newer).
                    if entry.needs_clear {
                        encoder.clear_texture(
                            target,
                            &wgpu::ImageSubresourceRange {
                                aspect: wgpu::TextureAspect::All,
                                base_mip_level: 0,
                                mip_level_count: Some(1),
                                base_array_layer: 0,
                                array_layer_count: Some(1),
                            },
                        );
                        entry.needs_clear = false;
                    }
                    continue;
                }
            }
            entry.imported_generation = frame.generation;
        }
        true
    }
}

/// Fake image data: an empty blob with the frame's real size. Vello never
/// reads the blob; the `override_image` registration makes it sample the
/// imported RGBA texture instead.
fn fake_image(width: u32, height: u32) -> ImageData {
    ImageData {
        data: peniko::Blob::new(std::sync::Arc::new(&[])),
        format: peniko::ImageFormat::Rgba8,
        alpha_type: peniko::ImageAlphaType::Alpha,
        width,
        height,
    }
}
