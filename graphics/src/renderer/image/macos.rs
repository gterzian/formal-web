//! macOS image fast path: decode once into the image's own global IOSurface.
//! The compositor emits the image layer as a direct surface, and the embedder
//! looks the surface up by ID and presents it on that layer's CoreAnimation
//! sublayer. After the decode, the image travels through the same zero-copy
//! IOSurface delivery path as a composed frame.

use std::collections::HashMap;

use ipc_channel::platform::OsMachPort;
use ipc_messages::content::ImageIdentifier;
use ipc_messages::graphics::SurfacePayload;
use objc2_core_foundation::CFRetained;
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use vello::Renderer as VelloRenderer;

use crate::iosurface::create_image_surface;

use super::{ImageBackend, ImageSurface, StoredImage, StoredImageResult};

pub(crate) struct MacosImageBackend {
    /// The graphics process keeps each image surface alive for the webview's
    /// lifetime: the embedder looks it up by global IOSurfaceID, so releasing
    /// it here would destroy the surface under the CALayer.
    surfaces: HashMap<ImageIdentifier, CFRetained<IOSurfaceRef>>,
    next_texture_id: u64,
}

impl ImageBackend for MacosImageBackend {
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
        let decoded = image::load_from_memory(encoded)
            .map_err(|error| format!("failed to decode image: {error}"))?
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        let Some(surface) = create_image_surface(width, height) else {
            return Err(String::from("IOSurfaceCreate failed for image"));
        };

        let mut seed = 0u32;
        // SAFETY: `seed` is a valid pointer; the surface is owned by this
        // function and not otherwise accessed.
        let lock_result = unsafe { surface.lock(IOSurfaceLockOptions(0), &mut seed) };
        if lock_result != 0 {
            return Err(format!("IOSurfaceLock failed: {lock_result}"));
        }
        let bytes_per_row = surface.bytes_per_row();
        let source_row_bytes = width as usize * 4;
        let total_bytes = bytes_per_row
            .checked_mul(height as usize)
            .ok_or_else(|| String::from("image surface size overflow"))?;
        assert!(source_row_bytes <= bytes_per_row);
        // SAFETY: the surface is locked and its base address points to
        // `bytes_per_row * height` writable bytes. Clearing the whole surface
        // first also zeroes the width padding the embedder crops with
        // `contentsRect`.
        let base = surface.base_address().as_ptr().cast::<u8>();
        unsafe {
            std::ptr::write_bytes(base, 0, total_bytes);
        }
        let source = decoded.as_raw();
        for row in 0..height as usize {
            let source_offset = row * source_row_bytes;
            let destination_offset = row * bytes_per_row;
            assert!(source_offset + source_row_bytes <= source.len());
            assert!(destination_offset + source_row_bytes <= total_bytes);
            // SAFETY: both ranges are in bounds (asserted above); the
            // surfaces do not overlap because `base` is the surface mapping
            // and `source` is the decoded image buffer.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    source.as_ptr().add(source_offset),
                    base.add(destination_offset),
                    source_row_bytes,
                );
            }
        }
        // SAFETY: the surface was locked above with the same options.
        let unlock_result = unsafe { surface.unlock(IOSurfaceLockOptions(0), &mut seed) };
        if unlock_result != 0 {
            return Err(format!("IOSurfaceUnlock failed: {unlock_result}"));
        }

        let texture_id = self.next_texture_id;
        self.next_texture_id += 1;
        let payload = SurfacePayload::SharedTexture {
            texture_id,
            surface_id: surface.id(),
            port: OsMachPort::from_name(surface.create_mach_port()),
        };
        self.surfaces.insert(id.clone(), surface);
        Ok(Some((
            StoredImage {
                image_data: None,
                surface: Some(ImageSurface { payload }),
            },
            (width, height),
        )))
    }

    fn release_images(&mut self, image_ids: &[ImageIdentifier]) {
        for image_id in image_ids {
            self.surfaces.remove(image_id);
        }
    }
}
