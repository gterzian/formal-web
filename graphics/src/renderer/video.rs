//! macOS video texture import: wraps a decoded NV12 `CVPixelBuffer`'s two
//! planes as Metal textures (zero-copy when the buffer is GPU-backed) and
//! converts them to RGBA in a compute pass, with the matrix named by the
//! buffer's `kCVImageBufferYCbCrMatrixKey` attachment, into a texture Vello
//! can register (Vello's `register_texture` requires Rgba8Unorm + COPY_SRC).
//! Lives behind the renderer trait's `store_video_frame` (macOS); the
//! AVFoundation media backend delivers `PixelBufferFrame` events and the
//! renderer stores each frame's pixel buffer here without touching the GPU.
//!
//! The import is deferred from frame arrival to compose time: the media
//! callback only stores the latest raw frame (pixel buffer + size +
//! generation counter), and `submit_scene` (via `record_imports`) converts
//! exactly the frames whose generation is newer than the last imported one
//! in their own submission, right before Vello's render submits. That keeps
//! the video import off the media event path and turns "one pass per
//! decoded frame" into "one pass per frame that is actually composited".

use ipc_messages::media::VideoPaintId;
use log::error;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::CFType;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidthOfPlane,
    kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrix_ITU_R_2020,
    kCVImageBufferYCbCrMatrix_SMPTE_240M_1995, kCVImageBufferYCbCrMatrixKey, kCVReturnSuccess,
};
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLTexture};
use peniko::ImageData;
use std::collections::{HashMap, HashSet};
use vello::Renderer as VelloRenderer;
use wgpu::{
    ComputePipelineDescriptor, Extent3d, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor,
};

const NV12_TO_RGBA_WGSL: &str = r#"
struct ColorMatrix {
    red: vec4<f32>,
    green: vec4<f32>,
    blue: vec4<f32>,
    range: vec4<f32>,
}

@group(0) @binding(0) var luma_plane: texture_2d<f32>;
@group(0) @binding(1) var chroma_plane: texture_2d<f32>;
@group(0) @binding(2) var chroma_sampler: sampler;
@group(0) @binding(3) var<uniform> color: ColorMatrix;
@group(0) @binding(4) var dst_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(dst_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) {
        return;
    }
    let luma = textureLoad(luma_plane, vec2<i32>(gid.xy), 0).r;
    // NV12 chroma is subsampled 2x2, so this luma pixel's chroma sits at
    // (pixel * 0.5 + 0.25) texel centres in the half-resolution plane; the
    // linear sampler upsamples it across the 2x2 luma block.
    let chroma_dims = vec2<f32>(textureDimensions(chroma_plane));
    let chroma_uv = (vec2<f32>(gid.xy) * 0.5 + 0.25) / chroma_dims;
    let chroma = textureSampleLevel(chroma_plane, chroma_sampler, chroma_uv, 0.0).rg;
    // range.x/range.y undo the luma black level and excursion of a
    // video-range signal; range.z/range.w centre and scale its chroma.
    let luma_scaled = clamp((luma - color.range.x) * color.range.y, 0.0, 1.0);
    let cb = (chroma.x - color.range.z) * color.range.w;
    let cr = (chroma.y - color.range.z) * color.range.w;
    let ycbcr = vec3<f32>(luma_scaled, cb, cr);
    let rgb = clamp(
        vec3<f32>(
            dot(color.red.xyz, ycbcr),
            dot(color.green.xyz, ycbcr),
            dot(color.blue.xyz, ycbcr),
        ),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    textureStore(dst_tex, vec2<i32>(gid.xy), vec4<f32>(rgb, 1.0));
}
"#;

/// Black level of video-range luma, in `[0, 1]` units (16/255).
const LUMA_VIDEO_OFFSET: f32 = 16.0 / 255.0;
/// Stretches video-range luma, 219/255 of full range, back onto `[0, 1]`.
const LUMA_VIDEO_SCALE: f32 = 255.0 / 219.0;
/// Zero point of video-range chroma, in `[0, 1]` units (128/255).
const CHROMA_OFFSET: f32 = 128.0 / 255.0;
/// Stretches video-range chroma, 224/255 of full range, onto `[-0.5, 0.5]`.
const CHROMA_VIDEO_SCALE: f32 = 255.0 / 224.0;

/// The YUV→RGB coefficient set a frame's `kCVImageBufferYCbCrMatrixKey`
/// attachment names.
#[derive(Clone, Copy)]
enum YuvMatrix {
    Bt601,
    Bt709,
    Smpte240M,
    Bt2020,
}

impl YuvMatrix {
    /// Every matrix, in the order [`VideoImport::color_uniforms`] holds them.
    const ALL: [Self; 4] = [Self::Bt601, Self::Bt709, Self::Smpte240M, Self::Bt2020];

    /// This matrix's index into the renderer's per-matrix uniform buffers.
    fn slot(self) -> usize {
        match self {
            Self::Bt601 => 0,
            Self::Bt709 => 1,
            Self::Smpte240M => 2,
            Self::Bt2020 => 3,
        }
    }

    /// The matrix the frame's YCbCr matrix attachment names, BT.709 when the
    /// buffer carries none.
    fn from_pixel_buffer(pixel_buffer: &CVPixelBuffer) -> Self {
        // SAFETY: `kCVImageBufferYCbCrMatrixKey` is a CoreVideo attachment
        // key, and the lookup passes a null mode pointer.
        let attachment =
            unsafe { pixel_buffer.attachment(kCVImageBufferYCbCrMatrixKey, std::ptr::null_mut()) };
        let Some(attachment) = attachment else {
            return Self::Bt709;
        };
        let value: &CFType = &attachment;
        // SAFETY: the `kCVImageBufferYCbCrMatrix_*` statics are CoreVideo
        // `CFString` constants; `CFType` equality compares their contents.
        let bt601: &CFType = unsafe { kCVImageBufferYCbCrMatrix_ITU_R_601_4 };
        // SAFETY: as above.
        let smpte240m: &CFType = unsafe { kCVImageBufferYCbCrMatrix_SMPTE_240M_1995 };
        // SAFETY: as above.
        let bt2020: &CFType = unsafe { kCVImageBufferYCbCrMatrix_ITU_R_2020 };
        if value == bt601 {
            Self::Bt601
        } else if value == smpte240m {
            Self::Smpte240M
        } else if value == bt2020 {
            Self::Bt2020
        } else {
            Self::Bt709
        }
    }

    /// The shader coefficients for a video-range signal.
    fn uniform(self) -> ColorUniform {
        let (red, green, blue) = match self {
            // Rec. 601
            Self::Bt601 => (
                [1.0, 0.0, 1.402],
                [1.0, -0.344_136, -0.714_136],
                [1.0, 1.772, 0.0],
            ),
            // Rec. 709
            Self::Bt709 => (
                [1.0, 0.0, 1.5748],
                [1.0, -0.187_324, -0.468_124],
                [1.0, 1.8556, 0.0],
            ),
            // SMPTE 240M
            Self::Smpte240M => ([1.0, 0.0, 1.5756], [1.0, -0.225_3, -0.5], [1.0, 1.827, 0.0]),
            // Rec. 2020, non-constant luminance
            Self::Bt2020 => (
                [1.0, 0.0, 1.4746],
                [1.0, -0.164_55, -0.571_35],
                [1.0, 1.8814, 0.0],
            ),
        };
        ColorUniform {
            red: [red[0], red[1], red[2], 0.0],
            green: [green[0], green[1], green[2], 0.0],
            blue: [blue[0], blue[1], blue[2], 0.0],
            range: [
                LUMA_VIDEO_OFFSET,
                LUMA_VIDEO_SCALE,
                CHROMA_OFFSET,
                CHROMA_VIDEO_SCALE,
            ],
        }
    }
}

/// The conversion shader's uniform: one coefficient row per RGB channel
/// applied to `(luma, cb, cr)`, plus the range conversion. Four `f32`
/// vectors, matching `ColorMatrix` in the shader.
#[repr(C)]
struct ColorUniform {
    red: [f32; 4],
    green: [f32; 4],
    blue: [f32; 4],
    range: [f32; 4],
}

impl ColorUniform {
    /// The uniform buffer contents in the shader struct's field order.
    fn to_bytes(&self) -> [u8; std::mem::size_of::<Self>()] {
        let mut bytes = [0u8; std::mem::size_of::<Self>()];
        let values = self
            .red
            .iter()
            .chain(self.green.iter())
            .chain(self.blue.iter())
            .chain(self.range.iter());
        for (index, value) in values.enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
        }
        bytes
    }
}

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
/// the generation of the raw frame last converted into the target.
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
    /// Generation of the raw frame last converted into `texture`.
    imported_generation: u64,
    /// The target was created but never written (neither converted nor
    /// cleared): a failed first conversion must leave black pixels, not
    /// uninitialized memory, for Vello to sample.
    needs_clear: bool,
}

struct VideoImport {
    /// Metal texture cache for this renderer's device.
    cache: Retained<CVMetalTextureCache>,
    /// The NV12→RGBA conversion pipeline.
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    /// One coefficients buffer per [`YuvMatrix`], indexed by
    /// [`YuvMatrix::slot`]: the frame's attachment picks the slot, so every
    /// matrix is uploaded once when the import is created.
    color_uniforms: [wgpu::Buffer; YuvMatrix::ALL.len()],
    /// Linear sampler that upsamples the half-resolution chroma plane.
    chroma_sampler: wgpu::Sampler,
}

impl VideoImport {
    /// Create the Metal texture cache and NV12→RGBA pipeline on this
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
            label: Some("nv12-to-rgba"),
            source: wgpu::ShaderSource::Wgsl(NV12_TO_RGBA_WGSL.into()),
        });
        let color_uniform_size = wgpu::BufferSize::new(std::mem::size_of::<ColorUniform>() as u64)?;
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nv12-to-rgba-layout"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(color_uniform_size),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
            label: Some("nv12-to-rgba-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("nv12-to-rgba"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let color_uniforms =
            YuvMatrix::ALL.map(|matrix| color_uniform_buffer(device, matrix.uniform()));
        let chroma_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video-chroma-upsample"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Some(VideoImport {
            cache,
            pipeline,
            bind_group_layout,
            color_uniforms,
            chroma_sampler,
        })
    }

    /// Wrap `pixel_buffer`'s two NV12 planes as wgpu textures on this device
    /// and record a compute pass converting them into `target` (RGBA8Unorm
    /// storage) on `encoder`, without submitting. The caller submits the
    /// encoder once, batching this pass with the Vello render. The source
    /// textures reference the pixel buffer, which the stored frame keeps
    /// alive until the next frame replaces it.
    fn convert_nv12_into_rgba(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        device: &wgpu::Device,
        pixel_buffer: &Retained<CVPixelBuffer>,
        target: &Texture,
    ) -> Option<()> {
        let plane_count = CVPixelBufferGetPlaneCount(pixel_buffer);
        if plane_count < 2 {
            error!("[gpu-renderer] video frame has {plane_count} planes, NV12 needs 2");
            return None;
        }
        let luma_width = CVPixelBufferGetWidthOfPlane(pixel_buffer, 0) as u32;
        let luma_height = CVPixelBufferGetHeightOfPlane(pixel_buffer, 0) as u32;
        let chroma_width = CVPixelBufferGetWidthOfPlane(pixel_buffer, 1) as u32;
        let chroma_height = CVPixelBufferGetHeightOfPlane(pixel_buffer, 1) as u32;

        let luma = plane_texture(
            device,
            &self.cache,
            pixel_buffer,
            0,
            luma_width,
            luma_height,
        )?;
        let chroma = plane_texture(
            device,
            &self.cache,
            pixel_buffer,
            1,
            chroma_width,
            chroma_height,
        )?;

        let luma_view = luma.create_view(&TextureViewDescriptor::default());
        let chroma_view = chroma.create_view(&TextureViewDescriptor::default());
        let target_view = target.create_view(&TextureViewDescriptor::default());
        let matrix = YuvMatrix::from_pixel_buffer(pixel_buffer);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12-to-rgba-bind"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&luma_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&chroma_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.chroma_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.color_uniforms[matrix.slot()].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&target_view),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("nv12-to-rgba"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(luma_width.div_ceil(8), luma_height.div_ceil(8), 1);
        }
        Some(())
    }
}

/// Allocate a uniform buffer holding one YUV→RGB coefficient set.
fn color_uniform_buffer(device: &wgpu::Device, uniform: ColorUniform) -> wgpu::Buffer {
    let bytes = uniform.to_bytes();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("video-color-matrix"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: true,
    });
    buffer
        .slice(..)
        .get_mapped_range_mut()
        .copy_from_slice(&bytes);
    buffer.unmap();
    buffer
}

/// Wrap one plane of `pixel_buffer` as a wgpu texture. NV12's plane 0 is the
/// luma plane and plane 1 the interleaved CbCr plane. The Metal texture
/// references the pixel buffer; the stored frame keeps it alive until the
/// next frame replaces it.
fn plane_texture(
    device: &wgpu::Device,
    cache: &Retained<CVMetalTextureCache>,
    pixel_buffer: &Retained<CVPixelBuffer>,
    plane: usize,
    width: u32,
    height: u32,
) -> Option<Texture> {
    let (metal_format, format, label) = if plane == 0 {
        (
            MTLPixelFormat::R8Unorm,
            TextureFormat::R8Unorm,
            "video-nv12-luma",
        )
    } else {
        (
            MTLPixelFormat::RG8Unorm,
            TextureFormat::Rg8Unorm,
            "video-nv12-chroma",
        )
    };
    let mut texture_ptr = std::ptr::null_mut();
    let result = unsafe {
        CVMetalTextureCache::create_texture_from_image(
            None,
            cache,
            pixel_buffer,
            None,
            metal_format,
            width as usize,
            height as usize,
            plane,
            std::ptr::NonNull::new(&mut texture_ptr)?,
        )
    };
    if result != kCVReturnSuccess {
        error!("[gpu-renderer] create_texture_from_image failed for plane {plane}: {result}");
        return None;
    }
    // SAFETY: the create call above wrote a valid retained texture.
    let cv_metal_texture: Retained<CVMetalTexture> = unsafe { Retained::from_raw(texture_ptr) }?;
    // SAFETY: CVMetalTextureGetTexture returns a +1 retained Metal texture
    // referencing the pixel buffer.
    let raw_texture: Retained<ProtocolObject<dyn MTLTexture>> =
        CVMetalTextureGetTexture(&cv_metal_texture)?;

    let hal_texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            raw_texture,
            format,
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
    Some(unsafe {
        device.create_texture_from_hal::<wgpu::hal::metal::Api>(
            hal_texture,
            &TextureDescriptor {
                label: Some(label),
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        )
    })
}

/// The per-renderer video texture state: the Metal texture cache + NV12
/// conversion pipeline, the latest raw frame per paint id (stored on arrival,
/// no GPU work), and the imported RGBA target per paint id. The AVFoundation
/// media backend delivers `PixelBufferFrame` events; each frame's pixel
/// buffer is stored here and converted at compose time (see
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
    /// `override_image` when the compose-time import converts the frame.
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

    /// Convert every stored raw frame whose generation is newer than the
    /// last imported one into `encoder`, recording the NV12→RGBA passes.
    /// The caller submits the encoder right before Vello's render submits,
    /// so the conversion completes before the render reads its output (GPU
    /// execution order). Paints with no newer frame are left alone: their
    /// RGBA texture is reused, so a compose without new video does zero GPU
    /// work here (returns `false`). The reconverted images are marked dirty
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
            match video_import.convert_nv12_into_rgba(encoder, device, &frame.pixel_buffer, target)
            {
                Some(()) => {
                    entry.needs_clear = false;
                    // The target contents changed; Vello must recopy them
                    // into its atlas on this render.
                    vello_renderer.mark_override_image_dirty(&entry.image);
                }
                None => {
                    error!("[gpu-renderer] video conversion failed for {paint_id:?}");
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
