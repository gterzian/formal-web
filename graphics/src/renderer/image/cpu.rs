//! CPU / cross-platform image decoder: decoded RGBA8 pixels live in
//! `ImageData`; Vello copies them into its atlas when a scene first draws them.

use std::collections::HashMap;
use std::io::Cursor;

use image::{ImageFormat as CrateImageFormat, ImageReader};
use ipc_messages::content::ImageIdentifier;
use ipc_messages::graphics::{DecodedImage, ImageFormat};
use peniko::{Blob, ImageAlphaType, ImageData, ImageFormat as PenikoImageFormat};
use vello::Renderer as VelloRenderer;

use super::{ImageDecoder, StoredImage, StoredImageResult};

pub(crate) struct CpuImageDecoder {
    images: HashMap<ImageIdentifier, (ImageData, DecodedImage)>,
}

impl ImageDecoder for CpuImageDecoder {
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
    ) -> Result<StoredImageResult, String> {
        if let Some((image, decoded)) = self.images.get(id) {
            return Ok((
                StoredImage {
                    image_data: Some(image.clone()),
                    surface: None,
                },
                *decoded,
            ));
        }
        let reader = ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .map_err(|error| format!("failed to sniff image format: {error}"))?;
        let format = reader
            .format()
            .map(report_format)
            .unwrap_or(ImageFormat::Other);
        let rgba = reader
            .decode()
            .map_err(|error| format!("failed to decode image: {error}"))?
            .into_rgba8();
        let (width, height) = rgba.dimensions();
        let decoded = DecodedImage {
            width,
            height,
            format,
        };
        let image = ImageData {
            data: Blob::from(rgba.into_raw()),
            format: PenikoImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        };
        self.images.insert(id.clone(), (image.clone(), decoded));
        Ok((
            StoredImage {
                image_data: Some(image),
                surface: None,
            },
            decoded,
        ))
    }

    fn release_images(&mut self, image_ids: &[ImageIdentifier]) {
        for image_id in image_ids {
            self.images.remove(image_id);
        }
    }
}

/// The wire format the content process checks for the accepted set.
fn report_format(format: CrateImageFormat) -> ImageFormat {
    match format {
        CrateImageFormat::Png => ImageFormat::Png,
        CrateImageFormat::Jpeg => ImageFormat::Jpeg,
        CrateImageFormat::Gif => ImageFormat::Gif,
        CrateImageFormat::WebP => ImageFormat::WebP,
        _ => ImageFormat::Other,
    }
}
