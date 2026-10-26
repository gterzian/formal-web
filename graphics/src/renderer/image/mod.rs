//! Image decode and storage for the graphics process. The compositor embeds
//! one image per `<img>`; how the decoded pixels are presented is chosen at
//! compile time:
//!
//! - **CPU / cross-platform** (`cpu.rs`, the slow path): the `image` crate
//!   decodes the encoded bytes to RGBA8 and stores them in `ImageData`.
//!   Vello draws that image into the layer's own surface, exactly like any
//!   other scene content.
//! - **macOS fast path** (`macos.rs`): Image I/O decodes the bytes and Core
//!   Graphics draws them once into the image's own global IOSurface.  The
//!   compositor emits the image layer as a *direct surface* instead of
//!   rendering it; the embedder looks the surface up by ID and sets it as the
//!   contents of that layer's CoreAnimation sublayer.  The decoded texture
//!   reaches the embedder zero-copy.
//!
//! `ImageDecoder` is the single interface both paths implement; the `Decoder`
//! alias names the one this build compiles.

use ipc_messages::content::ImageIdentifier;
use ipc_messages::graphics::{DecodedImage, SurfacePayload};
use peniko::ImageData;
use vello::Renderer as VelloRenderer;

#[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
mod cpu;
#[cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
mod macos;

#[cfg(any(not(target_os = "macos"), feature = "cpu_readback"))]
pub(crate) use cpu::CpuImageDecoder as Decoder;
#[cfg(all(target_os = "macos", not(feature = "cpu_readback")))]
pub(crate) use macos::MacosImageDecoder as Decoder;

/// How one decoded image reaches the embedder. Exactly one of the two forms
/// is present, chosen by the image decoder.
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

/// A decoded image's presentation plus the dimensions and format the decoder
/// reports.
pub(crate) type StoredImageResult = (StoredImage, DecodedImage);

/// Decode one `<img>` resource into this decoder's storage and report how the
/// decoded image is presented to the embedder, its dimensions, and the format
/// it was decoded from. Registering an image that is already stored returns
/// the stored image again, so the report is always available.
pub(crate) trait ImageDecoder {
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
    ) -> Result<StoredImageResult, String>;

    /// Drop the decoded state and any shared surface for the given image
    /// identifiers (their document was unloaded).
    fn release_images(&mut self, image_ids: &[ImageIdentifier]);
}
