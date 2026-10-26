//! macOS image fast path: Image I/O decodes the encoded bytes and Core
//! Graphics draws the decoded image straight into the image's own global
//! IOSurface, so the decode has no intermediate pixel buffer and no row copy.
//! The surface is the only copy of the pixels: the compositor emits the image
//! layer as a direct surface, the embedder looks the surface up by ID, and the
//! image travels through the same zero-copy IOSurface delivery path as a
//! composed frame.
//!
//! The draw targets a premultiplied, sRGB bitmap context whose bytes are R,
//! G, B, A in memory: the order of the surface's 'RGBA' format and of the
//! `RGBA8Unorm` texture a GPU consumer would wrap it as.

use std::collections::HashMap;

use ipc_channel::platform::OsMachPort;
use ipc_messages::content::ImageIdentifier;
use ipc_messages::graphics::SurfacePayload;
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFRetained, CFString, CGPoint, CGRect, CGSize, kCFBooleanFalse,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBlendMode, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    kCGColorSpaceSRGB,
};
use objc2_image_io::{CGImageSource, kCGImageSourceShouldCache};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use vello::Renderer as VelloRenderer;

use crate::iosurface::create_image_surface;

use super::{ImageDecoder, ImageSurface, StoredImage, StoredImageResult};

pub(crate) struct MacosImageDecoder {
    /// The graphics process keeps each image surface alive for the webview's
    /// lifetime: the embedder looks it up by global IOSurfaceID, so releasing
    /// it here would destroy the surface under the CALayer.
    surfaces: HashMap<ImageIdentifier, CFRetained<IOSurfaceRef>>,
    next_texture_id: u64,
}

impl ImageDecoder for MacosImageDecoder {
    fn new() -> Self {
        Self {
            surfaces: HashMap::new(),
            next_texture_id: 1,
        }
    }

    fn store_image(
        &mut self,
        _: &wgpu::Device,
        _: &wgpu::Queue,
        _: &mut VelloRenderer,
        id: &ImageIdentifier,
        encoded: &[u8],
    ) -> Result<Option<StoredImageResult>, String> {
        if self.surfaces.contains_key(id) {
            return Ok(None);
        }
        let decoded = decode_into_surface(encoded)?;
        let texture_id = self.next_texture_id;
        self.next_texture_id += 1;
        let payload = SurfacePayload::SharedTexture {
            texture_id,
            surface_id: decoded.surface.id(),
            port: OsMachPort::from_name(decoded.surface.create_mach_port()),
        };
        self.surfaces.insert(id.clone(), decoded.surface);
        Ok(Some((
            StoredImage {
                image_data: None,
                surface: Some(ImageSurface { payload }),
            },
            (decoded.width, decoded.height),
        )))
    }

    fn release_images(&mut self, image_ids: &[ImageIdentifier]) {
        for image_id in image_ids {
            self.surfaces.remove(image_id);
        }
    }
}

/// One encoded image decoded into an RGBA8 surface of its own, with the
/// image's natural dimensions.
struct DecodedSurface {
    surface: CFRetained<IOSurfaceRef>,
    width: u32,
    height: u32,
}

/// Decode `encoded` into a new RGBA8 IOSurface: the image's pixels at natural
/// size, premultiplied, with the surface's width padding left clear so the
/// embedder can crop it with `contentsRect`.
fn decode_into_surface(encoded: &[u8]) -> Result<DecodedSurface, String> {
    let image = create_cg_image(encoded)?;
    let image_width = CGImage::width(Some(&image));
    let image_height = CGImage::height(Some(&image));
    let width = u32::try_from(image_width)
        .map_err(|error| format!("image width {image_width} is not addressable: {error}"))?;
    let height = u32::try_from(image_height)
        .map_err(|error| format!("image height {image_height} is not addressable: {error}"))?;
    let Some(surface) = create_image_surface(width, height) else {
        return Err(String::from("IOSurfaceCreate failed for image"));
    };
    draw_image_into_surface(&image, &surface, width, height)?;
    Ok(DecodedSurface {
        surface,
        width,
        height,
    })
}

/// Decode the encoded bytes into a `CGImage`, taking the first frame of an
/// animated format. `kCGImageSourceShouldCache` is false because the surface
/// is the stored copy: Image I/O has no decoded image it needs to keep.
fn create_cg_image(encoded: &[u8]) -> Result<CFRetained<CGImage>, String> {
    let data = CFData::from_bytes(encoded);
    // SAFETY: `kCGImageSourceShouldCache` and `kCFBooleanFalse` are framework
    // constants valid for the process lifetime.
    let should_cache_key: &CFString = unsafe { kCGImageSourceShouldCache };
    let Some(should_not_cache) = (unsafe { kCFBooleanFalse }) else {
        return Err(String::from("kCFBooleanFalse is null"));
    };
    let options: CFRetained<CFDictionary<CFString, CFBoolean>> =
        CFDictionary::from_slices(&[should_cache_key], &[should_not_cache]);
    let image_options: &CFDictionary = options.as_ref();
    // SAFETY: `data` is a CFData holding the encoded image, and the options
    // dictionary's only key is Image I/O's own `kCGImageSourceShouldCache`
    // with a CFBoolean value, the type that key takes.
    let Some(source) = (unsafe { CGImageSource::with_data(&data, None) }) else {
        return Err(String::from("CGImageSourceCreateWithData returned nil"));
    };
    // SAFETY: the options dictionary has the same shape as above; index 0 is
    // the first frame of any image.
    let Some(image) = (unsafe { source.image_at_index(0, Some(image_options)) }) else {
        return Err(String::from(
            "CGImageSourceCreateImageAtIndex returned nil for image",
        ));
    };
    Ok(image)
}

/// Draw `image` into `surface`, locking the surface for the duration of the
/// draw and unlocking it before the pixels are handed to the embedder.
fn draw_image_into_surface(
    image: &CGImage,
    surface: &IOSurfaceRef,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // SAFETY: `kCGColorSpaceSRGB` is a framework constant valid for the
    // process lifetime.
    let named_space: &CFString = unsafe { kCGColorSpaceSRGB };
    let Some(color_space) = CGColorSpace::with_name(Some(named_space)) else {
        return Err(String::from(
            "CGColorSpaceCreateWithName(sRGB) returned nil",
        ));
    };
    let mut seed = 0u32;
    // SAFETY: `seed` is a valid out-parameter, and the surface is not
    // accessed again until it is unlocked below.
    let lock_result = unsafe { surface.lock(IOSurfaceLockOptions(0), &mut seed) };
    if lock_result != 0 {
        return Err(format!("IOSurfaceLock failed: {lock_result}"));
    }
    let draw_result = draw_into_locked_surface(image, surface, width, height, &color_space);
    // SAFETY: the surface was locked above with the same options, and
    // `draw_into_locked_surface` dropped the bitmap context it created over
    // the surface's pages.
    let unlock_result = unsafe { surface.unlock(IOSurfaceLockOptions(0), &mut seed) };
    if unlock_result != 0 {
        return Err(format!("IOSurfaceUnlock failed: {unlock_result}"));
    }
    draw_result
}

/// Write `image` into the pages of a locked `surface`: clear the width
/// padding, then draw the image into the logical width and height.
fn draw_into_locked_surface(
    image: &CGImage,
    surface: &IOSurfaceRef,
    width: u32,
    height: u32,
    color_space: &CGColorSpace,
) -> Result<(), String> {
    let bytes_per_row = surface.bytes_per_row();
    let total_bytes = bytes_per_row
        .checked_mul(height as usize)
        .ok_or_else(|| String::from("image surface size overflow"))?;
    let row_bytes = width as usize * 4;
    assert!(row_bytes <= bytes_per_row);
    assert!(total_bytes <= surface.alloc_size());
    // SAFETY: the surface is locked, so its base address is a writable
    // mapping of at least `alloc_size` bytes covering `bytes_per_row *
    // height` (asserted above).
    let base = surface.base_address().as_ptr().cast::<u8>();
    // The draw below replaces every pixel of the logical width, so only the
    // width padding needs clearing: the embedder crops it with `contentsRect`,
    // and it must not hold whatever the surface's pages held before.
    let padding_bytes = bytes_per_row - row_bytes;
    if padding_bytes > 0 {
        for row in 0..height as usize {
            // SAFETY: this row's padding starts `row_bytes` into the row and
            // is `padding_bytes` long, both inside the mapping (asserted
            // above).
            unsafe {
                std::ptr::write_bytes(base.add(row * bytes_per_row + row_bytes), 0, padding_bytes);
            }
        }
    }
    // SAFETY: `base` points at `bytes_per_row * height` writable bytes of the
    // locked surface, and a row holds at least `width * 4` bytes (asserted
    // above). 8 bits per component with `premultipliedLast` and the default
    // byte order stores R, G, B, A in memory, the order of the surface's
    // 'RGBA' format.
    let Some(context) = (unsafe {
        CGBitmapContextCreate(
            base.cast(),
            width as usize,
            height as usize,
            8,
            bytes_per_row,
            Some(color_space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }) else {
        return Err(String::from(
            "CGBitmapContextCreate returned nil for image surface",
        ));
    };
    // `Copy` replaces the destination instead of compositing the source over
    // it: a translucent source pixel must not blend with what the surface's
    // pages held before, and no pass over the logical pixels is needed to
    // prepare the destination for them.
    CGContext::set_blend_mode(Some(&context), CGBlendMode::Copy);
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(f64::from(width), f64::from(height)),
        ),
        Some(image),
    );
    // The bitmap context writes through `base`; it is dropped before the
    // caller unlocks the surface.
    drop(context);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{ImageFormat, ImageReader, RgbaImage};

    use super::*;

    /// Pixels that differ per column and per row, so a channel-order, row-
    /// order, or stride mistake shows up as a mismatch.
    fn gradient_image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |column, row| {
            image::Rgba([
                (column * 29) as u8,
                (row * 61) as u8,
                ((column + row) * 13) as u8,
                255,
            ])
        })
    }

    fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut cursor = Cursor::new(&mut bytes);
        // JPEG carries no alpha channel; its encoder takes the RGB form.
        let written = match format {
            ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image.clone())
                .into_rgb8()
                .write_to(&mut cursor, format),
            _ => image.write_to(&mut cursor, format),
        };
        written.expect("encode fixture");
        bytes
    }

    /// The dimensions the content process's header probe reads, before it
    /// sends the bytes here.
    fn probe_dimensions(encoded: &[u8]) -> (u32, u32) {
        ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .expect("guess fixture format")
            .into_dimensions()
            .expect("probe fixture dimensions")
    }

    /// Run `write` over the surface's whole mapping, `bytes_per_row` per row,
    /// while the surface is locked.
    fn with_locked_mapping(
        surface: &IOSurfaceRef,
        height: u32,
        write: impl FnOnce(&mut [u8], usize),
    ) {
        let mut seed = 0u32;
        // SAFETY: `seed` is a valid out-parameter, and the surface is not
        // accessed again until it is unlocked below.
        let lock_result = unsafe { surface.lock(IOSurfaceLockOptions(0), &mut seed) };
        assert_eq!(lock_result, 0, "IOSurfaceLock failed: {lock_result}");
        let bytes_per_row = surface.bytes_per_row();
        let len = bytes_per_row * height as usize;
        assert!(len <= surface.alloc_size());
        // SAFETY: the surface is locked, so its base address is a writable
        // mapping of at least `alloc_size` bytes covering `len`.
        let mapping = unsafe {
            std::slice::from_raw_parts_mut(surface.base_address().as_ptr().cast::<u8>(), len)
        };
        write(mapping, bytes_per_row);
        // SAFETY: the surface was locked above with the same options.
        let unlock_result = unsafe { surface.unlock(IOSurfaceLockOptions(0), &mut seed) };
        assert_eq!(unlock_result, 0, "IOSurfaceUnlock failed: {unlock_result}");
    }

    /// Read the surface's logical region back as tightly packed RGBA rows.
    fn read_surface_rgba(surface: &IOSurfaceRef, width: u32, height: u32) -> Vec<u8> {
        let row_bytes = width as usize * 4;
        let mut pixels = vec![0u8; row_bytes * height as usize];
        with_locked_mapping(surface, height, |mapping, bytes_per_row| {
            assert!(row_bytes <= bytes_per_row);
            for row in 0..height as usize {
                let source = row * bytes_per_row;
                let destination = row * row_bytes;
                pixels[destination..destination + row_bytes]
                    .copy_from_slice(&mapping[source..source + row_bytes]);
            }
        });
        pixels
    }

    /// Premultiply one channel of a straight-alpha pixel the way the
    /// premultiplied bitmap context stores it.
    fn premultiply(channel: u8, alpha: u8) -> u8 {
        u8::try_from((u16::from(channel) * u16::from(alpha) + 127) / 255)
            .expect("a premultiplied channel fits in u8")
    }

    /// Decode `encoded` through both decoders and compare the pixels and the
    /// dimensions. `tolerance` is the per-channel difference the decoders may
    /// differ by (JPEG decoders round differently; PNG must match exactly).
    fn assert_decoders_agree(encoded: &[u8], tolerance: u8) {
        let expected = image::load_from_memory(encoded)
            .expect("image crate decode")
            .into_rgba8();
        let decoded = decode_into_surface(encoded).expect("Image I/O decode");

        let dimensions = (decoded.width, decoded.height);
        assert_eq!(dimensions, expected.dimensions(), "decoder dimensions");
        assert_eq!(dimensions, probe_dimensions(encoded), "probed dimensions");

        let actual = read_surface_rgba(&decoded.surface, decoded.width, decoded.height);
        let expected_pixels = expected.as_raw();
        assert_eq!(actual.len(), expected_pixels.len());
        for (index, (actual, straight)) in actual.iter().zip(expected_pixels).enumerate() {
            let pixel = index / 4;
            let channel = index % 4;
            let alpha = expected_pixels[pixel * 4 + 3];
            // The image crate decodes straight alpha; the surface holds what
            // the premultiplied draw wrote.
            let expected_channel = if channel == 3 {
                alpha
            } else {
                premultiply(*straight, alpha)
            };
            assert!(
                actual.abs_diff(expected_channel) <= tolerance,
                "pixel {pixel} channel {channel}: Image I/O {actual}, image crate {expected_channel}"
            );
        }
    }

    #[test]
    fn png_decodes_like_the_image_crate() {
        let encoded = encode(&gradient_image(9, 5), ImageFormat::Png);
        assert_decoders_agree(&encoded, 0);
    }

    #[test]
    fn jpeg_decodes_like_the_image_crate() {
        let encoded = encode(&gradient_image(16, 8), ImageFormat::Jpeg);
        assert_decoders_agree(&encoded, 4);
    }

    #[test]
    fn translucent_png_is_premultiplied() {
        let mut fixture = RgbaImage::from_pixel(4, 2, image::Rgba([200, 100, 50, 128]));
        fixture.put_pixel(0, 0, image::Rgba([200, 100, 50, 255]));
        let encoded = encode(&fixture, ImageFormat::Png);
        assert_decoders_agree(&encoded, 0);
    }

    #[test]
    fn corrupt_bytes_are_rejected() {
        assert!(
            decode_into_surface(b"\x89PNG\r\n\x1a\nnot an image").is_err(),
            "bytes no decoder recognizes must fail"
        );
    }

    #[test]
    fn draw_replaces_the_surface_instead_of_blending() {
        let fixture = RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 128]));
        let encoded = encode(&fixture, ImageFormat::Png);
        let image = create_cg_image(&encoded).expect("Image I/O decode");
        let (width, height) = (2u32, 2u32);
        let surface = create_image_surface(width, height).expect("IOSurfaceCreate");
        // Fill the surface with a value a blend would bleed through.
        with_locked_mapping(&surface, height, |mapping, _| mapping.fill(0xab));
        draw_image_into_surface(&image, &surface, width, height).expect("draw");
        let expected = [
            premultiply(200, 128),
            premultiply(100, 128),
            premultiply(50, 128),
            128,
        ];
        assert_eq!(
            read_surface_rgba(&surface, width, height),
            expected.repeat(4)
        );
    }

    #[test]
    fn width_padding_is_cleared() {
        // 9 logical pixels inside a 64-pixel padded surface.
        let encoded = encode(&gradient_image(9, 3), ImageFormat::Png);
        let decoded = decode_into_surface(&encoded).expect("Image I/O decode");
        with_locked_mapping(
            &decoded.surface,
            decoded.height,
            |mapping, bytes_per_row| {
                let written = decoded.width as usize * 4;
                for row in 0..decoded.height as usize {
                    let padding =
                        &mapping[row * bytes_per_row + written..(row + 1) * bytes_per_row];
                    assert!(
                        padding.iter().all(|byte| *byte == 0),
                        "row {row} width padding is not clear"
                    );
                }
            },
        );
    }
}
