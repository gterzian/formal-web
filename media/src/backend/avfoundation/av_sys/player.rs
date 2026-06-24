//! Safe wrappers around `AVPlayer` operations.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_av_foundation::AVPlayer;
use objc2_foundation::NSURL;

use super::item::AvPlayerItem;
use super::time::CMTIME_SCALE;

/// Safe handle to an AVPlayer.
///
/// SAFETY: All AVPlayer access is serialized through the command queue,
/// matching AVFoundation's requirement that all operations on an AVPlayer
/// happen on a single thread.
pub(crate) struct AvPlayer {
    inner: Retained<AVPlayer>,
}

// SAFETY: The original code sends Retained<AVPlayer> across threads.
// All AVPlayer methods are called through &mut self, which is safe.
unsafe impl Send for AvPlayer {}

impl AvPlayer {
    /// Create an AVPlayer (uses new_unchecked internally).
    ///
    /// # Safety
    ///
    /// The caller must ensure all AVPlayer access is serialized on this
    /// thread (e.g. via a command queue).
    pub(crate) unsafe fn new(url: &NSURL) -> Self {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let inner = unsafe { AVPlayer::playerWithURL(url, mtm) };
        Self { inner }
    }

    /// Start or resume playback.
    pub(crate) fn play(&mut self) {
        unsafe { self.inner.play() };
    }

    /// Pause playback.
    pub(crate) fn pause(&mut self) {
        unsafe { self.inner.pause() };
    }

    /// Seek to an absolute time in seconds.
    pub(crate) fn seek(&mut self, secs: f64) {
        let time = unsafe { objc2_core_media::CMTime::with_seconds(secs, CMTIME_SCALE) };
        unsafe { self.inner.seekToTime(time) };
    }

    /// Replace the current item with `None` (teardown).
    pub(crate) fn clear_item(&mut self) {
        unsafe { self.inner.replaceCurrentItemWithPlayerItem(None) };
    }

    /// Get the current AVPlayerItem, if any.
    pub(crate) fn current_item(&self) -> Option<AvPlayerItem> {
        let item = unsafe { self.inner.currentItem()? };
        Some(AvPlayerItem { inner: item })
    }
}
