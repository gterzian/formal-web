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
    /// The thread that created the player; every access asserts it.
    owner: std::thread::ThreadId,
}

// SAFETY: `AVPlayer` is created and used on one thread only (the one that
// pumps its run loop); `inner()` asserts that thread at every access. The
// only operation allowed on another thread is the release performed when
// `inner` drops, which is an `objc_release` refcount decrement.
unsafe impl Send for AvPlayer {}

impl AvPlayer {
    /// The player, after checking that this is the thread that owns it.
    ///
    /// AVFoundation requires one thread with a run loop to own the player and
    /// everything that touches it; the check turns a cross-thread use (which
    /// would move ObjC access and the eventual release) into a panic.
    fn inner(&self) -> &Retained<AVPlayer> {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "AVPlayer used from a thread other than the one that created it"
        );
        &self.inner
    }

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
        Self {
            inner,
            owner: std::thread::current().id(),
        }
    }

    /// Start or resume playback.
    pub(crate) fn play(&self) {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().play() };
    }

    /// Pause playback.
    pub(crate) fn pause(&self) {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().pause() };
    }

    /// Seek to an absolute time in seconds.
    pub(crate) fn seek(&self, secs: f64) {
        // SAFETY: `CMTime::with_seconds` is a plain initializer; the scale is
        // the fixed host constant below.
        let time = unsafe { objc2_core_media::CMTime::with_seconds(secs, CMTIME_SCALE) };
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().seekToTime(time) };
    }

    /// Replace the current item with `None` (teardown).
    pub(crate) fn clear_item(&self) {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().replaceCurrentItemWithPlayerItem(None) };
    }

    /// Current playback rate (0 = paused/ended, 1 = playing).
    pub(crate) fn rate(&self) -> f32 {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        unsafe { self.inner().rate() }
    }

    /// Get the current AVPlayerItem, if any.
    pub(crate) fn current_item(&self) -> Option<AvPlayerItem> {
        // SAFETY: `inner()` proved this is the creating thread and the receiver is a retained ObjC instance.
        let item = unsafe { self.inner().currentItem()? };
        Some(AvPlayerItem::new(item))
    }
}
