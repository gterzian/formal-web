//! Image decode and storage backends for the graphics process.
//!
//! The compositor assembles scenes and embeds one image per `<img>`.
//! How that image is presented is backend-specific:
//!
//! - **CPU / cross-platform:** the `image` crate decodes the encoded bytes to
//!   RGBA8 and stores them in `ImageData`.  Vello draws that image into the
//!   layer's own surface, exactly like any other scene content.
//! - **macOS fast path:** the image is decoded once into its own global
//!   IOSurface.  The compositor emits the image layer as a *direct surface*
//!   instead of rendering it; the embedder looks the surface up by ID and sets
//!   it as the contents of that layer's CoreAnimation sublayer.  The decoded
//!   texture is shared with the embedder zero-copy after the initial decode.
//!
//! Decoding compressed bytes necessarily produces pixels.  On macOS the
//! remaining copy is the decode into the IOSurface; from then on the image
//! reaches the embedder through the same zero-copy IOSurface path as a
//! composed frame.  The `ImageBackend` boundary is where a future ImageIO
//! decoder can replace the `image` crate without touching the compositor, the
//! renderer, or the embedder.

use ipc_messages::content::ImageIdentifier;
use ipc_messages::graphics::SurfacePayload;
use peniko::ImageData;
use vello::Renderer as VelloRenderer;

#[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
mod cpu;
#[cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
mod macos;

#[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
pub(crate) use cpu::CpuImageBackend as Backend;
#[cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
pub(crate) use macos::MacosImageBackend as Backend;

/// How one decoded image reaches the embedder. Exactly one of the two forms
/// is present, chosen by the image backend.
#[derive(Clone)]
pub struct StoredImage {
    /// CPU path: pixels Vello draws into the layer's rendered surface.
    pub image_data: Option<ImageData>,
    /// macOS path: an IOSurface the embedder presents directly on the image
    /// layer's CoreAnimation sublayer.
    pub surface: Option<ImageSurface>,
}

/// A decoded image's own shared surface, ready for the embedder to present.
#[derive(Clone)]
pub struct ImageSurface {
    /// The shared IOSurface identity and Mach port, using the same
    /// `SurfacePayload::SharedTexture` shape as a composed frame.
    pub payload: SurfacePayload,
}

/// A decoded image's presentation plus its logical dimensions.
pub(crate) type StoredImageResult = (StoredImage, (u32, u32));

/// Decode one `<img>` resource and return how it should be presented, plus
/// the decoded dimensions.  `Ok(None)` means the image was already stored
/// under `id`.
pub(crate) trait ImageBackend {
    fn new() -> Self
    where
        Self: Sized;

    fn store_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vello_renderer: &mut VelloRenderer,
        id: &ImageIdentifier,
        encoded: &[u8],
    ) -> Result<Option<StoredImageResult>, String>;

    /// Drop the decoded state and any shared surface for the given image
    /// identifiers (their document was unloaded).
    fn release_images(&mut self, image_ids: &[ImageIdentifier]);
}
