//! Safe NSURL creation from a Rust string.

use std::ffi::CString;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSString, NSURL};

/// Create an `NSURL` from a UTF-8 string.
///
/// Returns `None` if the URL string contains a null byte or if
/// `NSURL::initWithString` fails (e.g. malformed URL).
pub(crate) fn url_from_string(url: &str) -> Option<Retained<NSURL>> {
    let c_string = CString::new(url).ok()?;
    let ptr = std::ptr::NonNull::new(c_string.as_ptr() as *mut _)?;
    let ns_string: Retained<NSString> = unsafe { NSString::stringWithUTF8String(ptr)? };
    NSURL::initWithString(NSURL::alloc(), &ns_string)
}
