//! Cross-process GPU surface sharing via IOSurface (macOS).
//! Allocates IOSurfaces and wraps them as wgpu::Texture objects
//! that can be shared between processes without CPU round-trips.

use log::debug;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::msg_send;
use objc2_core_foundation::{CFDictionary, CFNumber, CFType, CFRetained};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceWidth, kIOSurfaceHeight,
    kIOSurfaceBytesPerRow, kIOSurfaceBytesPerElement, kIOSurfacePixelFormat,
};
use objc2_metal::{MTLTextureDescriptor, MTLTextureType, MTLPixelFormat,
                  MTLStorageMode, MTLTextureUsage};
use wgpu::{Device, TextureDescriptor, TextureDimension, TextureFormat,
           TextureUsages, Extent3d};
use wgpu::hal::{self, metal::Api as MetalApi};

/// Big-endian FourCC for 'RGBA': R(0x52), G(0x47), B(0x42), A(0x41).
const IOSURFACE_PIXEL_FMT_RGBA: i32 = 0x52474241;

/// Allocate an IOSurface with RGBA8 pixel format.
pub fn allocate_iosurface(width: u32, height: u32) -> Option<(CFRetained<IOSurfaceRef>, u32)> {
    let w = width as isize;
    let h = height as isize;
    let bpr = (width * 4) as isize;

    let w_val = CFNumber::new_isize(w);
    let h_val = CFNumber::new_isize(h);
    let bpe_val = CFNumber::new_isize(4);
    let bpr_val = CFNumber::new_isize(bpr);
    let pix_val = CFNumber::new_i32(IOSURFACE_PIXEL_FMT_RGBA);

    let keys: [&CFType; 5] = [
        unsafe { &kIOSurfaceWidth },
        unsafe { &kIOSurfaceHeight },
        unsafe { &kIOSurfaceBytesPerElement },
        unsafe { &kIOSurfaceBytesPerRow },
        unsafe { &kIOSurfacePixelFormat },
    ];
    let values: [&CFType; 5] = [
        w_val.as_ref(), h_val.as_ref(), bpe_val.as_ref(),
        bpr_val.as_ref(), pix_val.as_ref(),
    ];

    let dict = CFDictionary::<CFType, CFType>::from_slices(&keys, &values);
    let surface = unsafe { IOSurfaceRef::new((&*dict).as_ref())? };
    let iosurface_id = surface.id();
    debug!("[iosurface] created id={} ({}x{})", iosurface_id, width, height);
    Some((surface, iosurface_id))
}

/// Import an existing IOSurface as a wgpu::Texture on the caller's device.
///
/// The MTLTexture is created via the system default Metal device
/// (single-GPU assumption — on Apple Silicon this is the same
/// physical GPU as wgpu's device). The resulting MTLTexture is
/// wrapped via wgpu_hal::metal::Device::texture_from_raw (an
/// associated function, no receiver needed) and then imported
/// into wgpu via Device::create_texture_from_hal.
pub fn import_iosurface_as_wgpu_texture(
    device: &Device,
    iosurface: &IOSurfaceRef,
    width: u32,
    height: u32,
) -> Option<wgpu::Texture> {
    unsafe extern "C" {
        fn MTLCreateSystemDefaultDevice() -> *mut std::ffi::c_void;
    }
    let raw_device = unsafe { MTLCreateSystemDefaultDevice() };
    if raw_device.is_null() {
        log::error!("[iosurface] MTLCreateSystemDefaultDevice failed");
        return None;
    }
    let mtl_device: Retained<ProtocolObject<dyn objc2_metal::MTLDevice>> = unsafe {
        Retained::from_raw(
            raw_device as *mut ProtocolObject<dyn objc2_metal::MTLDevice>,
        ).unwrap_or_else(|| unreachable!())
    };

    let descriptor = MTLTextureDescriptor::new();
    descriptor.setTextureType(MTLTextureType::Type2D);
    unsafe { descriptor.setWidth(width as _) };
    unsafe { descriptor.setHeight(height as _) };
    unsafe { descriptor.setMipmapLevelCount(1) };
    descriptor.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
    descriptor.setStorageMode(MTLStorageMode::Private);

    let mtl_texture: Option<Retained<ProtocolObject<dyn objc2_metal::MTLTexture>>> = unsafe {
        msg_send![&mtl_device, newTextureWithDescriptor: &*descriptor,
                  iosurface: iosurface, plane: 0u64]
    };
    let Some(mtl_texture) = mtl_texture else {
        log::error!("[iosurface] newTextureWithDescriptor:iosurface:plane: failed");
        return None;
    };

    // texture_from_raw is an associated function on wgpu_hal::metal::Device —
    // no Device receiver needed, it just constructs the HAL wrapper.
    let hal_texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            mtl_texture,
            TextureFormat::Rgba8Unorm,
            MTLTextureType::Type2D,
            1,  // array_layers
            1,  // mip_levels
            hal::CopyExtent { width, height, depth: 1 },
        )
    };
    let texture = unsafe {
        device.create_texture_from_hal::<MetalApi>(
            hal_texture,
            &TextureDescriptor {
                label: None,
                size: Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_DST,
                view_formats: &[],
            },
        )
    };
    debug!("[iosurface] imported as wgpu::Texture {}x{}", width, height);
    Some(texture)
}
