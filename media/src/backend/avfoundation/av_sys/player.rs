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
    /// Create an AVPlayer on the graphics select-loop thread.
    ///
    /// SAFETY: the marker is fabricated because `AVPlayer` is annotated
    /// `MainThreadOnly` by objc2-av-foundation, but AVFoundation only
    /// requires that an `AVPlayer` and everything that touches it share one
    /// thread with a run loop: creation happens here on the select-loop
    /// thread and `AvfPipeline::sample` pumps that thread's `NSRunLoop`.
    pub(crate) fn new(url: &NSURL) -> Self {
        // SAFETY: see above; the caller guarantees it runs on the thread
        // that owns the pipeline and pumps its run loop.
        let marker = unsafe { MainThreadMarker::new_unchecked() };
        let inner = unsafe { AVPlayer::playerWithURL(url, marker) };
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

    /// Current playback rate (0 = paused/ended, 1 = playing).
    pub(crate) fn rate(&self) -> f32 {
        unsafe { self.inner.rate() }
    }

    /// Get the current AVPlayerItem, if any.
    pub(crate) fn current_item(&self) -> Option<AvPlayerItem> {
        let item = unsafe { self.inner.currentItem()? };
        Some(AvPlayerItem { inner: item })
    }
}
