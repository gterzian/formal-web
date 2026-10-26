#![cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
//! macOS zero-copy surface: a shared IOSurface the graphics process renders
//! into and the embedder imports and blits, with no CPU readback and no IPC
//! pixel bytes. The surface's global IOSurfaceID (plus a Mach port
//! fallback) travels in the `PixelFrameReady` payload; the embedder looks
//! the surface up by ID.

use crate::renderer::IosurfaceRenderer;
use ipc_channel::platform::OsMachPort;
use log::{debug, error};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::{CFDictionary, CFNumber, CFString};
// The producer opts every shared surface into the global namespace so the
// embedder (and the WindowServer, which composites the layer contents) can
// look it up by ID. Apple marks `kIOSurfaceIsGlobal` deprecated ("global
// surfaces are insecure") but it is the only mechanism for cross-process
// IOSurfaceID visibility; the surfaces carry page pixels, not secrets.
#[allow(deprecated)]
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceAllocSize, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
    kIOSurfaceIsGlobal, kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};
use wgpu::{Extent3d, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// Metal requires the width of an IOSurface-backed texture to be a multiple
/// of 64; the surface (and every texture importing it) is created at the
/// padded width and the rendered content occupies the logical width's
/// top-left region.
pub fn padded_width(width: u32) -> u32 {
    (width.max(1) + 63) & !63
}

/// One shared IOSurface texture in the ring: the Metal texture (kept alive
/// by the imported wgpu texture; the Metal texture retains the IOSurface).
/// The surface's global ID is shipped to the embedder on every frame.
pub struct IosurfaceTexture {
    pub texture: Texture,
    pub texture_id: u64,
    /// Retained for diagnostics and the delivery of the surface's global ID.
    surface: CFRetained<IOSurfaceRef>,
    /// A Mach port (send right) to the surface, shipped alongside the
    /// global ID so the embedder can fall back to a port lookup.
    port: OsMachPort,
}

impl IosurfaceTexture {
    /// The surface's global IOSurfaceID, shipped to the embedder so it can
    /// look the surface up by ID (`IOSurfaceLookup`). A surface object
    /// imported from its Mach port cannot be composited by CoreAnimation;
    /// a by-ID lookup of the same surface composites correctly.
    pub fn surface_id(&self) -> u32 {
        self.surface.id()
    }

    /// A fresh Mach port (send right) to the surface for this frame's
    /// delivery. The transport MOVE_SENDs the clone; the surface keeps its
    /// own port internally so it stays shareable.
    pub fn port_for_frame(&self) -> OsMachPort {
        self.port.clone()
    }
}

/// Create a global RGBA IOSurface for a decoded `<img>` image. The surface is
/// padded to the Metal width constraint like a shared frame surface; the
/// graphics process keeps it alive and the embedder presents it directly on
/// the image layer's CoreAnimation sublayer.
pub(crate) fn create_image_surface(width: u32, height: u32) -> Option<CFRetained<IOSurfaceRef>> {
    let surface_width = padded_width(width);
    let properties = surface_properties(surface_width, height.max(1));
    // SAFETY: the properties dictionary is built with the exact keys and
    // value types IOSurfaceCreate expects.
    unsafe { IOSurfaceRef::new(&properties) }
}

/// Create one IOSurface-backed RGBA8 wgpu texture on the renderer's device.
/// The texture usage (STORAGE_BINDING | TEXTURE_BINDING | COPY_SRC) is what
/// Vello needs to render into it via compute; the Metal texture descriptor
/// must match the IOSurface's 'RGBA' format exactly.
pub fn create_shared_texture(
    renderer: &IosurfaceRenderer,
    width: u32,
    height: u32,
    texture_id: u64,
) -> Option<IosurfaceTexture> {
    let width = width.max(1);
    let height = height.max(1);
    // Metal rejects IOSurface-backed textures whose width is not a multiple
    // of 64; the surface is created at the padded width and the render only
    // fills the logical width's region.
    let surface_width = padded_width(width);
    let device = renderer.device();
    let raw_device = renderer.raw_metal_device()?;

    let properties = surface_properties(surface_width, height);
    // SAFETY: the properties dictionary is built with the exact keys and
    // value types IOSurfaceCreate expects.
    let Some(surface) = (unsafe { IOSurfaceRef::new(&properties) }) else {
        error!(
            "[iosurface] IOSurfaceCreate failed for {}x{}",
            width, height
        );
        return None;
    };
    let port = OsMachPort::from_name(surface.create_mach_port());
    // Metal texture from the IOSurface. The descriptor's format must match
    // the surface pixel format ('RGBA' ↔ RGBA8Unorm); the dimensions are the
    // padded surface dimensions.
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::RGBA8Unorm,
            surface_width as usize,
            height as usize,
            false,
        )
    };
    descriptor.setStorageMode(MTLStorageMode::Private);
    descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
    let Some(metal_texture) =
        raw_device.newTextureWithDescriptor_iosurface_plane(&descriptor, &surface, 0)
    else {
        error!(
            "[iosurface] newTextureWithDescriptor failed for {}x{} (surf {}x{})",
            width,
            height,
            surface.width(),
            surface.height()
        );
        return None;
    };
    let metal_texture: Retained<ProtocolObject<dyn MTLTexture>> =
        ProtocolObject::from_retained(metal_texture);

    // Import into wgpu on the same device the renderer composites with.
    let hal_texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            metal_texture,
            TextureFormat::Rgba8Unorm,
            objc2_metal::MTLTextureType::Type2D,
            1,
            1,
            wgpu::hal::CopyExtent {
                width: surface_width,
                height,
                depth: 1,
            },
        )
    };
    let texture = unsafe {
        device.create_texture_from_hal::<wgpu::hal::metal::Api>(
            hal_texture,
            &TextureDescriptor {
                label: Some("shared-iosurface"),
                size: Extent3d {
                    width: surface_width,
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
            },
        )
    };
    debug!(
        "[iosurface] created {}x{} (padded {}x{}) texture_id={} surface_id={}",
        width,
        height,
        surface_width,
        height,
        texture_id,
        surface.id()
    );
    Some(IosurfaceTexture {
        texture,
        texture_id,
        surface,
        port,
    })
}

/// Build the IOSurface creation properties dictionary: RGBA8, width, height.
#[allow(deprecated)]
fn surface_properties(width: u32, height: u32) -> Retained<CFDictionary> {
    let width_value = CFNumber::new_i64(i64::from(width));
    let height_value = CFNumber::new_i64(i64::from(height));
    let bytes_per_element_value = CFNumber::new_i64(4);
    // 'RGBA' as a 32-bit big-endian FourCC: 'R'=0x52, 'G'=0x47, 'B'=0x42,
    // 'A'=0x41 -> 0x52474241. CoreAnimation composites IOSurfaces by their
    // pixel format; an invalid FourCC makes the layer refuse the contents.
    let pixel_format_value = CFNumber::new_i64(0x52474241);
    let alloc_size_value = CFNumber::new_i64(i64::from(width * height * 4));
    // macOS 13+ keeps IOSurfaces process-local by default: other processes
    // (the embedder, the WindowServer compositing the layer contents) can
    // only look a surface up by its global ID when the creator opts in.
    let is_global_value = CFNumber::new_i64(1);

    // SAFETY: the keys and values are the exact CF objects IOSurfaceCreate
    // expects; the statics are valid for the process lifetime.
    let keys: [&CFString; 6] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
            kIOSurfaceAllocSize,
            kIOSurfaceIsGlobal,
        ]
    };
    let values: [&CFNumber; 6] = [
        &width_value,
        &height_value,
        &bytes_per_element_value,
        &pixel_format_value,
        &alloc_size_value,
        &is_global_value,
    ];
    let dictionary = CFDictionary::<CFString, CFNumber>::from_slices(&keys, &values);
    // SAFETY: CFDictionary<K, V> is a transparent wrapper over CFDictionaryRef;
    // the type parameters are phantom and the runtime content is CFString keys
    // with CFNumber values, which is what IOSurfaceCreate expects.
    unsafe { std::mem::transmute(dictionary) }
}
