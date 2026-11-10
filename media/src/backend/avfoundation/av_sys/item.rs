//! Safe wrappers around `AVPlayerItem` operations.

use objc2::rc::Retained;
use objc2_av_foundation::{AVPlayerItem, AVPlayerItemStatus};

use super::video_output::AvVideoOutput;

/// Safe handle to an AVPlayerItem.
pub(crate) struct AvPlayerItem {
    inner: Retained<AVPlayerItem>,
    /// The thread that created the item; every access asserts it.
    owner: std::thread::ThreadId,
}

// SAFETY: the item is created and used on one thread only (the one that pumps
// its run loop), and `inner()` asserts that thread at every access. The only
// operation allowed on another thread is the release performed when `inner`
// drops, which is an `objc_release` refcount decrement.
unsafe impl Send for AvPlayerItem {}

impl AvPlayerItem {
    pub(crate) fn new(inner: Retained<AVPlayerItem>) -> Self {
        Self {
            inner,
            owner: std::thread::current().id(),
        }
    }

    /// The item, after checking that this is the thread that owns it.
    fn inner(&self) -> &Retained<AVPlayerItem> {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "AVPlayerItem used from a thread other than the one that created it"
        );
        &self.inner
    }

    /// The item's playback status.
    pub(crate) fn status(&self) -> AVPlayerItemStatus {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().status() }
    }

    /// Current playback position in seconds.
    pub(crate) fn current_time_secs(&self) -> f64 {
        // SAFETY: `inner()` proved this is the creating thread; `CMTime` is
        // a plain value returned by the framework.
        let t = unsafe { self.inner().currentTime() };
        let secs = unsafe { t.seconds() };
        if secs.is_finite() && secs >= 0.0 {
            secs
        } else {
            0.0
        }
    }

    /// Duration in seconds, or 0.0 if not yet known.
    pub(crate) fn duration_secs(&self) -> f64 {
        // SAFETY: as for `currentTime`: the owning thread, a plain value back.
        let d = unsafe { self.inner().duration() };
        let secs = unsafe { d.seconds() };
        if secs.is_finite() && secs > 0.0 {
            secs
        } else {
            0.0
        }
    }

    /// Attach a video output to this item.
    pub(crate) fn add_output(&self, output: &AvVideoOutput) {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().addOutput(output.inner()) };
    }

    /// Remove a video output from this item.
    pub(crate) fn remove_output(&self, output: &AvVideoOutput) {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().removeOutput(output.inner()) };
    }
}
