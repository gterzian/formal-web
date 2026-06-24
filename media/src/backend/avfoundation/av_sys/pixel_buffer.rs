//! Safe wrappers around `CVPixelBuffer` lock/read/unlock.

use objc2::rc::Retained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVReturnSuccess,
};

use ipc_messages::media::{MediaPipelineId, VideoFrame};

/// Scoped lock on a CVPixelBuffer.  The buffer is unlocked on drop.
pub(crate) struct PixelBufferLock {
    // Holding the Retained keeps the buffer alive while locked.
    #[allow(dead_code)]
    buf: Retained<CVPixelBuffer>,
}

impl PixelBufferLock {
    /// Lock the pixel buffer for read-only access.
    ///
    /// Returns `None` if locking fails (e.g. the buffer is not in system
    /// memory).
    pub(crate) fn new(buf: &CVPixelBuffer) -> Option<Self> {
        let lock = CVPixelBufferLockFlags::ReadOnly;
        // SAFETY: buf is a valid CVPixelBuffer, and ReadOnly locking is
        // safe for read-only access from any thread.
        if unsafe { CVPixelBufferLockBaseAddress(buf, lock) } != kCVReturnSuccess {
            return None;
        }
        // Retain so the buffer stays alive while locked.
        // SAFETY: buf points to a valid Objective-C object, and we hold
        // a reference from the caller.
        let retained = unsafe { Retained::retain(buf as *const _ as *mut _) }?;
        Some(Self { buf: retained })
    }

    /// Width in pixels.
    pub(crate) fn width(&self) -> usize {
        CVPixelBufferGetWidth(&*self.buf)
    }

    /// Height in pixels.
    pub(crate) fn height(&self) -> usize {
        CVPixelBufferGetHeight(&*self.buf)
    }

    /// Bytes per row (may include padding beyond width × 4).
    pub(crate) fn bytes_per_row(&self) -> usize {
        CVPixelBufferGetBytesPerRow(&*self.buf)
    }

    /// Pointer to the first byte of pixel data.
    pub(crate) fn base_address(&self) -> *const u8 {
        CVPixelBufferGetBaseAddress(&*self.buf) as *const u8
    }
}

impl Drop for PixelBufferLock {
    fn drop(&mut self) {
        let lock = CVPixelBufferLockFlags::ReadOnly;
        // SAFETY: Matches the lock call above.
        unsafe { CVPixelBufferUnlockBaseAddress(&*self.buf, lock) };
    }
}

/// Convert a locked `CVPixelBuffer` to a tightly-packed `VideoFrame`.
///
/// The buffer is assumed to have 4 bytes per pixel (BGRA). Row padding
/// (bytes_per_row may be larger than width × 4) is stripped so the output
/// is tightly packed W×H×4.
pub(crate) fn pixel_buffer_to_frame(
    pipeline_id: MediaPipelineId,
    lock: &PixelBufferLock,
) -> Option<VideoFrame> {
    let width = lock.width();
    let height = lock.height();
    let bpr = lock.bytes_per_row();
    let base = lock.base_address();

    if width == 0 || height == 0 || base.is_null() {
        return None;
    }

    let row_bytes = width * 4;
    let mut data = Vec::with_capacity(height * row_bytes);
    for row in 0..height {
        // SAFETY: The pixel buffer is locked for the lifetime of `lock`,
        // so the memory at `base + row * bpr` is valid and accessible.
        let src = unsafe { std::slice::from_raw_parts(base.add(row * bpr), row_bytes) };
        data.extend_from_slice(src);
    }

    Some(VideoFrame {
        pipeline_id,
        width: width as u32,
        height: height as u32,
        data,
    })
}
