//! macOS image fast path: Image I/O decodes the encoded bytes and Core
//! Graphics draws the decoded image straight into the image's own global
//! IOSurface. This module holds no intermediate pixel buffer and runs no row
//! copy: the draw reads the `CGImage` and writes the surface, and the `CGImage`
//! is dropped when the decode returns, so the surface is the pixel buffer this
//! process keeps. The compositor emits the image layer as a direct surface, the
//! embedder looks the surface up by ID, and the image travels through the same
//! zero-copy IOSurface delivery path as a composed frame.
//!
//! The draw targets a premultiplied, sRGB bitmap context whose bytes are R,
//! G, B, A in memory: the order of the surface's 'RGBA' format and of the
//! `RGBA8Unorm` texture a GPU consumer would wrap it as. Rendering into an
//! sRGB context means an embedded ICC profile is converted to sRGB, which the
//! cross-platform `image`-crate decoder does not do (see `graphics/README.md`).

use std::collections::HashMap;
use std::ptr::write_bytes;

use ipc_channel::platform::OsMachPort;
use ipc_messages::content::ImageIdentifier;
use ipc_messages::graphics::{DecodedImage, ImageFormat, SurfacePayload};
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

/// One image's shared surface, the texture id it was registered under, the
/// Mach port it is shared on, and the report the decoder hands back for it.
struct StoredSurface {
    surface: CFRetained<IOSurfaceRef>,
    texture_id: u64,
    port: OsMachPort,
    decoded: DecodedImage,
}

pub(crate) struct MacosImageDecoder {
    /// The graphics process keeps each image surface alive for the webview's
    /// lifetime: the embedder looks it up by global IOSurfaceID, so releasing
    /// it here would destroy the surface under the CALayer.
    surfaces: HashMap<ImageIdentifier, StoredSurface>,
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
    ) -> Result<StoredImageResult, String> {
        if let Some(stored) = self.surfaces.get(id) {
            return Ok((present_stored_surface(stored), stored.decoded));
        }
        let decoded = decode_into_surface(encoded)?;
        let texture_id = self.next_texture_id;
        self.next_texture_id += 1;
        let stored = StoredSurface {
            port: OsMachPort::from_name(decoded.surface.create_mach_port()),
            surface: decoded.surface,
            texture_id,
            decoded: decoded.decoded,
        };
        let presentation = present_stored_surface(&stored);
        let report = stored.decoded;
        self.surfaces.insert(id.clone(), stored);
        Ok((presentation, report))
    }

    fn release_images(&mut self, image_ids: &[ImageIdentifier]) {
        for image_id in image_ids {
            self.surfaces.remove(image_id);
        }
    }
}

/// How one stored surface is presented to the embedder: the shared surface
/// identity and Mach port, using the same payload shape as a composed frame.
fn present_stored_surface(stored: &StoredSurface) -> StoredImage {
    StoredImage {
        image_data: None,
        surface: Some(ImageSurface {
            payload: SurfacePayload::SharedTexture {
                texture_id: stored.texture_id,
                surface_id: stored.surface.id(),
                // A clone of the surface's send right, so re-registering an
                // already-decoded image does not mint a port each time.
                port: stored.port.clone(),
            },
        }),
    }
}

/// One encoded image decoded into an RGBA8 surface of its own, with the report
/// the decoder hands back for it.
struct DecodedSurface {
    surface: CFRetained<IOSurfaceRef>,
    decoded: DecodedImage,
}

/// Decode `encoded` into a new RGBA8 IOSurface: the image's pixels at natural
/// size, premultiplied, with the surface's width padding left clear so the
/// embedder can crop it with `contentsRect`.
fn decode_into_surface(encoded: &[u8]) -> Result<DecodedSurface, String> {
    let (image, format) = create_cg_image(encoded)?;
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
        decoded: DecodedImage {
            width,
            height,
            format,
        },
    })
}

/// Decode the encoded bytes into a `CGImage` and the format its type
/// identifier names, taking the first frame of an animated format.
/// `kCGImageSourceShouldCache` is false because the surface is the stored copy:
/// Image I/O has no decoded image it needs to keep.
fn create_cg_image(encoded: &[u8]) -> Result<(CFRetained<CGImage>, ImageFormat), String> {
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
    // SAFETY: `data` is a CFData holding the encoded image for the lifetime
    // of the call; no options are passed, so Image I/O opens the source with
    // its defaults.
    let Some(source) = (unsafe { CGImageSource::with_data(&data, None) }) else {
        return Err(String::from("CGImageSourceCreateWithData returned nil"));
    };
    // SAFETY: `CGImageSourceGetType` returns a +1 type identifier for a source
    // this function owns; a source whose data Image I/O did not recognize
    // reports none.
    let format = (unsafe { source.r#type() })
        .map(|identifier| report_format(&identifier.to_string()))
        .unwrap_or(ImageFormat::Other);
    // SAFETY: the options dictionary's only key is Image I/O's own
    // `kCGImageSourceShouldCache` with a CFBoolean value, the type that key
    // takes; index 0 is the first frame of any image.
    let Some(image) = (unsafe { source.image_at_index(0, Some(image_options)) }) else {
        return Err(String::from(
            "CGImageSourceCreateImageAtIndex returned nil for image",
        ));
    };
    Ok((image, format))
}

/// The wire format for an Image I/O type identifier (a UTI).
fn report_format(identifier: &str) -> ImageFormat {
    match identifier {
        "public.png" => ImageFormat::Png,
        "public.jpeg" => ImageFormat::Jpeg,
        "com.compuserve.gif" => ImageFormat::Gif,
        "org.webmproject.webp" => ImageFormat::WebP,
        _ => ImageFormat::Other,
    }
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
                write_bytes(base.add(row * bytes_per_row + row_bytes), 0, padding_bytes);
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
    use std::slice::from_raw_parts_mut;

    use image::{DynamicImage, ImageFormat, ImageReader, Rgba, RgbaImage, load_from_memory};
    use ipc_messages::graphics::ImageFormat as ReportFormat;
    use objc2_core_foundation::CFArray;

    use super::*;

    /// Pixels that differ per column and per row, so a channel-order, row-
    /// order, or stride mistake shows up as a mismatch.
    fn gradient_image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |column, row| {
            Rgba([
                (column * 29) as u8,
                (row * 61) as u8,
                ((column + row) * 13) as u8,
                255,
            ])
        })
    }

    /// Sharply different colours every 8 columns and rows, so decoder rounding
    /// that a smooth gradient hides shows up as a mismatch. The `image` crate's
    /// JPEG encoder does not subsample chroma, so this is not a 4:2:0 fixture;
    /// neither JPEG fixture covers a photo's chroma upsampling.
    fn blocky_image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |column, row| {
            let block = ((column / 8) * 7 + (row / 8) * 13) % 251;
            Rgba([
                (block * 3) as u8,
                (block * 5 + 40) as u8,
                (block * 11 + 90) as u8,
                255,
            ])
        })
    }

    fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut cursor = Cursor::new(&mut bytes);
        // JPEG carries no alpha channel; its encoder takes the RGB form.
        let written = match format {
            ImageFormat::Jpeg => DynamicImage::ImageRgba8(image.clone())
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
        let mapping =
            unsafe { from_raw_parts_mut(surface.base_address().as_ptr().cast::<u8>(), len) };
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

    /// Per-channel difference between the decoders on the JPEG fixtures this
    /// module encodes. Both measure a worst difference of 2; the extra room
    /// covers rounding that varies between macOS versions, while a
    /// channel-order or colour-space mistake still exceeds it.
    const JPEG_TOLERANCE: u8 = 4;

    /// Per-channel difference on the 4:2:0 fixture, which measures 4: the
    /// decoders upsample its shared chroma samples differently, which the
    /// 4:2:2 fixtures cannot show.
    const SUBSAMPLED_JPEG_TOLERANCE: u8 = 8;

    /// The type identifiers Image I/O reports for this OS, as strings.
    fn supported_type_identifiers() -> Vec<String> {
        // SAFETY: `CGImageSourceCopyTypeIdentifiers` returns a +1 array of UTI
        // strings, or null, which the wrapper turns into an empty array.
        let identifiers: CFRetained<CFArray> = unsafe { CGImageSource::type_identifiers() };
        // SAFETY: Image I/O documents every element of that array as a
        // `CFString` UTI, and `CFArray<CFString>` has the same layout as the
        // opaque `CFArray` it arrives as.
        let identifiers: &CFArray<CFString> =
            unsafe { &*(identifiers.as_opaque() as *const CFArray).cast::<CFArray<CFString>>() };
        identifiers
            .iter()
            .map(|identifier| identifier.to_string())
            .collect()
    }

    /// Decode `encoded` through both decoders and compare the pixels and the
    /// dimensions. `tolerance` is the per-channel difference allowed between
    /// them; PNG and GIF must match exactly, JPEG rounds differently.
    fn assert_decoders_agree(encoded: &[u8], tolerance: u8, expected_format: ReportFormat) {
        let expected = load_from_memory(encoded)
            .expect("image crate decode")
            .into_rgba8();
        let decoded = decode_into_surface(encoded).expect("Image I/O decode");

        let dimensions = (decoded.decoded.width, decoded.decoded.height);
        assert_eq!(dimensions, expected.dimensions(), "decoder dimensions");
        assert_eq!(dimensions, probe_dimensions(encoded), "probed dimensions");
        assert_eq!(decoded.decoded.format, expected_format, "reported format");

        let actual = read_surface_rgba(
            &decoded.surface,
            decoded.decoded.width,
            decoded.decoded.height,
        );
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
        assert_decoders_agree(&encoded, 0, ReportFormat::Png);
    }

    #[test]
    fn jpeg_decodes_like_the_image_crate() {
        let encoded = encode(&gradient_image(16, 8), ImageFormat::Jpeg);
        assert_decoders_agree(&encoded, JPEG_TOLERANCE, ReportFormat::Jpeg);
    }

    #[test]
    fn blocky_jpeg_decodes_like_the_image_crate() {
        let encoded = encode(&blocky_image(64, 48), ImageFormat::Jpeg);
        assert_decoders_agree(&encoded, JPEG_TOLERANCE, ReportFormat::Jpeg);
    }

    #[test]
    fn subsampled_jpeg_decodes_like_the_image_crate() {
        // `graphics/tests/fixtures/photo-like-420.jpg` is a checked-in 48x32
        // photo-like fixture (smooth shading plus one hard colour edge) whose
        // SOF0 sampling factors are 0x22/0x11/0x11: 4:2:0, the layout photos
        // use, where the decoders' chroma upsampling can differ. The bytes are
        // checked in because the `image` crate's JPEG encoder is fixed at
        // 4:2:2 and cannot produce one; they were written by
        // `sips -s format jpeg` from a PNG of the same shading, so any 4:2:0
        // encoder can regenerate the fixture.
        let encoded = include_bytes!("../../../tests/fixtures/photo-like-420.jpg");
        assert_decoders_agree(encoded, SUBSAMPLED_JPEG_TOLERANCE, ReportFormat::Jpeg);
    }

    #[test]
    fn gif_decodes_like_the_image_crate() {
        let encoded = encode(&gradient_image(9, 5), ImageFormat::Gif);
        assert_decoders_agree(&encoded, 0, ReportFormat::Gif);
    }

    #[test]
    fn webp_decodes_like_the_image_crate() {
        // Lossless, so the two decoders must agree exactly.
        let encoded = encode(&gradient_image(16, 8), ImageFormat::WebP);
        assert_decoders_agree(&encoded, 0, ReportFormat::WebP);
    }

    #[test]
    fn image_io_reports_the_formats_content_accepts() {
        // No fallback decoder exists, so a format the running OS's Image I/O
        // cannot read fails the registration on that OS. WebP is the one
        // Apple's platforms gate (WebKit checks for it before accepting).
        let identifiers = supported_type_identifiers();
        for format in ["gif", "webp"] {
            assert!(
                identifiers
                    .iter()
                    .any(|identifier| identifier.to_ascii_lowercase().contains(format)),
                "Image I/O on this OS does not list {format}: {identifiers:?}"
            );
        }
    }

    #[test]
    fn translucent_png_is_premultiplied() {
        let mut fixture = RgbaImage::from_pixel(4, 2, Rgba([200, 100, 50, 128]));
        fixture.put_pixel(0, 0, Rgba([200, 100, 50, 255]));
        let encoded = encode(&fixture, ImageFormat::Png);
        assert_decoders_agree(&encoded, 0, ReportFormat::Png);
    }

    #[test]
    fn corrupt_bytes_are_rejected() {
        assert!(
            decode_into_surface(b"\x89PNG\r\n\x1a\nnot an image").is_err(),
            "bytes no decoder recognizes must fail"
        );
    }

    #[test]
    fn draw_replaces_pixels_and_clears_the_padding() {
        let fixture = RgbaImage::from_pixel(2, 2, Rgba([200, 100, 50, 128]));
        let encoded = encode(&fixture, ImageFormat::Png);
        let (image, _) = create_cg_image(&encoded).expect("Image I/O decode");
        // 2 logical pixels inside a 64-pixel padded surface.
        let (width, height) = (2u32, 2u32);
        let surface = create_image_surface(width, height).expect("IOSurfaceCreate");
        // The pre-fill serves both checks below: source-over blending would
        // bleed it through the image, and a missing padding clear would leave
        // it in the padded columns.
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
            expected.repeat(4),
            "the draw must replace the destination, not blend over it"
        );
        let written = width as usize * 4;
        with_locked_mapping(&surface, height, |mapping, bytes_per_row| {
            for row in 0..height as usize {
                let padding = &mapping[row * bytes_per_row + written..(row + 1) * bytes_per_row];
                assert!(
                    padding.iter().all(|byte| *byte == 0),
                    "row {row} width padding is not clear"
                );
            }
        });
    }
}
