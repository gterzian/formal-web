//! Safe wrappers around `AVPlayerItem` operations.

use objc2::rc::Retained;
use objc2_av_foundation::{AVPlayerItem, AVPlayerItemStatus};

use super::video_output::AvVideoOutput;

/// Safe handle to an AVPlayerItem.
pub(crate) struct AvPlayerItem {
    pub(crate) inner: Retained<AVPlayerItem>,
}

unsafe impl Send for AvPlayerItem {}

impl AvPlayerItem {
    /// The item's playback status.
    pub(crate) fn status(&self) -> AVPlayerItemStatus {
        unsafe { self.inner.status() }
    }

    /// Duration in seconds, or 0.0 if not yet known.
    pub(crate) fn duration_secs(&self) -> f64 {
        let d = unsafe { self.inner.duration() };
        let secs = unsafe { d.seconds() };
        if secs.is_finite() && secs > 0.0 {
            secs
        } else {
            0.0
        }
    }

    /// Attach a video output to this item.
    pub(crate) fn add_output(&self, output: &AvVideoOutput) {
        unsafe { self.inner.addOutput(&output.inner) };
    }

    /// Remove a video output from this item.
    pub(crate) fn remove_output(&self, output: &AvVideoOutput) {
        unsafe { self.inner.removeOutput(&output.inner) };
    }
}
