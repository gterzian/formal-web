//! Raw Objective-C URLSession FFI declarations. Crate-private: the public
//! surface of this crate is the safe [`crate::UrlSession`] API.

use std::os::raw::{c_char, c_int, c_void};

pub enum FwUrlSession_private {}
pub type FwUrlSession = *mut FwUrlSession_private;

/// Completion callback invoked exactly once when a data task finishes.
/// All pointer arguments are only valid for the duration of the call;
/// the caller must copy what it needs. `error` is NULL on success.
pub type FwUrlSessionCompletion = unsafe extern "C" fn(
    context: *mut c_void,
    status_code: c_int,
    final_url: *const c_char,
    content_type: *const c_char,
    body: *const u8,
    body_length: usize,
    error: *const c_char,
);

unsafe extern "C" {
    /// Create a session with no shared cache. Returns NULL on failure.
    pub fn fw_url_session_create() -> FwUrlSession;

    /// Release a session. NULL-safe.
    pub fn fw_url_session_release(session: FwUrlSession);

    /// Start a data task on the session. `header_names` and `header_values`
    /// are parallel arrays of `header_count` NUL-terminated strings,
    /// appended to the request in order; entries are only read for the
    /// duration of the call. Returns 0 when the task was started (the
    /// completion callback will be invoked later, on a background queue),
    /// non-zero when the task could not be started (the completion callback
    /// is not invoked).
    pub fn fw_url_session_fetch(
        session: FwUrlSession,
        method: *const c_char,
        url: *const c_char,
        header_names: *const *const c_char,
        header_values: *const *const c_char,
        header_count: usize,
        body: *const u8,
        body_length: usize,
        context: *mut c_void,
        completion: Option<FwUrlSessionCompletion>,
    ) -> c_int;
}
