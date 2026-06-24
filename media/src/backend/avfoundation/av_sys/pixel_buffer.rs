//! Safe wrappers around `CVPixelBuffer` lock/read/unlock.

use objc2::rc::Retained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVReturnSuccess,
};

use ipc_messages::media::{MediaPipelineId, VideoFrame};

/// Scoped lock on a CVPixelBuffer.  The buffer is unlocked on drop.
///
/// Takes ownership of a `Retained<CVPixelBuffer>` (from
/// `copyPixelBufferForItemTime:`) and holds it locked for the lifetime of
/// this lock.
pub(crate) struct PixelBufferLock {
    _buf: Retained<CVPixelBuffer>,
}

impl PixelBufferLock {
    /// Lock a CVPixelBuffer for read-only access.
    pub(crate) fn new(buf: Retained<CVPixelBuffer>) -> Option<Self> {
        let lock = CVPixelBufferLockFlags::ReadOnly;
        // SAFETY: buf is valid and the lock is read-only.
        if unsafe { CVPixelBufferLockBaseAddress(&buf, lock) } != kCVReturnSuccess {
            return None;
        }
        Some(Self { _buf: buf })
    }

    pub(crate) fn width(&self) -> usize {
        CVPixelBufferGetWidth(&self._buf)
    }

    pub(crate) fn height(&self) -> usize {
        CVPixelBufferGetHeight(&self._buf)
    }

    pub(crate) fn bytes_per_row(&self) -> usize {
        CVPixelBufferGetBytesPerRow(&self._buf)
    }

    pub(crate) fn base_address(&self) -> *const u8 {
        CVPixelBufferGetBaseAddress(&self._buf) as *const u8
    }
}

impl Drop for PixelBufferLock {
    fn drop(&mut self) {
        let lock = CVPixelBufferLockFlags::ReadOnly;
        // SAFETY: Matches the lock call above.
        unsafe { CVPixelBufferUnlockBaseAddress(&self._buf, lock) };
    }
}

/// Convert a locked CVPixelBuffer to a tightly-packed VideoFrame.
///
/// The buffer is assumed to have 4 bytes per pixel (BGRA).
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
        let src = unsafe { std::slice::from_raw_parts(base.add(row * bpr), row_bytes) };
        // The pixel buffer is BGRA but the compositor expects RGBA.
        // Swap byte 0 (B) with byte 2 (R) in each 4-byte pixel.
        for chunk in src.chunks_exact(4) {
            data.push(chunk[2]); // R
            data.push(chunk[1]); // G
            data.push(chunk[0]); // B
            data.push(chunk[3]); // A
        }
    }

    Some(VideoFrame {
        pipeline_id,
        width: width as u32,
        height: height as u32,
        data,
    })
}
