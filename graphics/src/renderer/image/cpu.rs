//! CPU / cross-platform image backend: decoded RGBA8 pixels live in
//! `ImageData`; Vello copies them into its atlas when a scene first draws them.

use std::collections::HashMap;

use ipc_messages::content::ImageIdentifier;
use peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use vello::Renderer as VelloRenderer;

use super::{ImageBackend, StoredImage, StoredImageResult};

pub(crate) struct CpuImageBackend {
    images: HashMap<ImageIdentifier, (ImageData, (u32, u32))>,
}

impl ImageBackend for CpuImageBackend {
    fn new() -> Self {
        Self {
            images: HashMap::new(),
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
        if self.images.contains_key(id) {
            return Ok(None);
        }
        let decoded = image::load_from_memory(encoded)
            .map_err(|error| format!("failed to decode image: {error}"))?
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        let image = ImageData {
            data: Blob::from(decoded.into_raw()),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        };
        self.images
            .insert(id.clone(), (image.clone(), (width, height)));
        Ok(Some((
            StoredImage {
                image_data: Some(image),
                surface: None,
            },
            (width, height),
        )))
    }

    fn release_images(&mut self, image_ids: &[ImageIdentifier]) {
        for image_id in image_ids {
            self.images.remove(image_id);
        }
    }
}
