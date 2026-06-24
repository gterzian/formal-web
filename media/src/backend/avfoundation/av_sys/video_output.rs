//! Safe wrappers around `AVPlayerItemVideoOutput`.

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_av_foundation::AVPlayerItemVideoOutput;
use objc2_core_media::CMTime;
use objc2_core_video::CVPixelBuffer;

/// Safe handle to an AVPlayerItemVideoOutput.
pub(crate) struct AvVideoOutput {
    pub(crate) inner: Retained<AVPlayerItemVideoOutput>,
}

impl AvVideoOutput {
    /// Create a new video output with default pixel buffer attributes.
    pub(crate) fn new() -> Self {
        let inner = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                None,
            )
        };
        Self { inner }
    }

    /// Suppress system video rendering — the consumer handles display.
    pub(crate) fn suppress_rendering(&self) {
        unsafe { self.inner.setSuppressesPlayerRendering(true) };
    }

    /// Convert a host-time value (seconds in the CoreVideo time base) to the
    /// equivalent item timeline time.
    pub(crate) fn item_time_for_host_time(&self, host_secs: f64) -> CMTime {
        unsafe { self.inner.itemTimeForHostTime(host_secs) }
    }

    /// Check whether a new pixel buffer is available at the given item time.
    pub(crate) fn has_new_pixel_buffer(&self, time: CMTime) -> bool {
        unsafe { self.inner.hasNewPixelBufferForItemTime(time) }
    }

    /// Copy the pixel buffer for the given item time, if available.
    pub(crate) fn copy_pixel_buffer(&self, time: CMTime) -> Option<Retained<CVPixelBuffer>> {
        unsafe {
            self.inner
                .copyPixelBufferForItemTime_itemTimeForDisplay(time, std::ptr::null_mut())
        }
    }
}
