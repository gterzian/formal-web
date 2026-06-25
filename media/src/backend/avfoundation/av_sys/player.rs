//! Safe wrappers around `AVPlayer` operations.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_av_foundation::AVPlayer;
use objc2_foundation::NSURL;

use super::item::AvPlayerItem;
use super::time::CMTIME_SCALE;

/// Safe handle to an AVPlayer.
pub(crate) struct AvPlayer {
    inner: Retained<AVPlayer>,
}

// SAFETY: All AVPlayer access is single-threaded (the select-loop thread).
unsafe impl Send for AvPlayer {}

impl AvPlayer {
    /// Create an AVPlayer on the main thread (safe, with explicit marker).
    pub(crate) unsafe fn new_on_main(url: &NSURL, mtm: MainThreadMarker) -> Self {
        let inner = unsafe { AVPlayer::playerWithURL(url, mtm) };
        Self { inner }
    }

    /// Start or resume playback.
    pub(crate) fn play(&self) {
        unsafe { self.inner.play() };
    }

    /// Pause playback.
    pub(crate) fn pause(&self) {
        unsafe { self.inner.pause() };
    }

    /// Seek to an absolute time in seconds.
    pub(crate) fn seek(&self, secs: f64) {
        let time = unsafe { objc2_core_media::CMTime::with_seconds(secs, CMTIME_SCALE) };
        unsafe { self.inner.seekToTime(time) };
    }

    /// Replace the current item with `None` (teardown).
    pub(crate) fn clear_item(&self) {
        unsafe { self.inner.replaceCurrentItemWithPlayerItem(None) };
    }

    /// Get the current AVPlayerItem, if any.
    pub(crate) fn current_item(&self) -> Option<AvPlayerItem> {
        let item = unsafe { self.inner.currentItem()? };
        Some(AvPlayerItem { inner: item })
    }
}
