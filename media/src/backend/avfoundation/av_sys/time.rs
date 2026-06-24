//! Safe wrappers for CoreVideo host-time and CoreMedia CMTime utilities.
//!
//! `CVGetCurrentHostTime` returns Mach absolute time in ticks.
//! `CVGetHostClockFrequency` returns ticks per second.
//! Dividing gives seconds in the CoreVideo time base, suitable for
//! `AVPlayerItemOutput::itemTimeForHostTime:`.

use objc2_core_video::{CVGetCurrentHostTime, CVGetHostClockFrequency};

/// Preferred timescale for CMTime values created in this backend.
pub(crate) const CMTIME_SCALE: i32 = 600;

/// Current host time in seconds (CoreVideo time base).
///
/// This is the correct time base to pass to
/// `AVPlayerItemVideoOutput::itemTimeForHostTime:`.
pub(crate) fn host_time_seconds() -> f64 {
    let ticks = CVGetCurrentHostTime();
    let freq = CVGetHostClockFrequency();
    ticks as f64 / freq
}
