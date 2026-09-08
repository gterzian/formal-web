//! Safe Apple URLSession client for formal-web.
//!
//! Wraps the Objective-C URLSession surface (`src/url_session_wrapper.m`) in
//! a safe API: one session with no shared cache, per fetch completion
//! callbacks. Fetch completions fire on a background queue, at any time
//! after [`UrlSession::fetch`] returns. Only compiled on Apple targets.

#![allow(non_camel_case_types, non_snake_case)]

mod ffi;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};

/// The callback invoked with the outcome of a fetch.
pub type FetchCompletion = Box<dyn FnOnce(Result<FetchResponse, String>) + Send>;

/// <https://fetch.spec.whatwg.org/#concept-response>
pub struct FetchResponse {
    pub status: u16,
    pub final_url: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// One NSURLSession with no shared cache.
pub struct UrlSession {
    handle: ffi::FwUrlSession,
}

impl UrlSession {
    /// Create a session with no shared cache.
    pub fn new() -> Result<Self, String> {
        // SAFETY: `fw_url_session_create` returns an owned handle (NULL on
        // failure); the handle is released exactly once in `Drop`.
        let handle = unsafe { ffi::fw_url_session_create() };
        if handle.is_null() {
            return Err(String::from("failed to create NSURLSession"));
        }
        Ok(UrlSession { handle })
    }

    /// Start a fetch on this session. The completion handler is invoked
    /// exactly once, on a background queue, when the task finishes; it may
    /// fire after this method returns.
    ///
    /// Note: no task handle is exposed, so in-flight fetches cannot be
    /// cancelled; abort-on-drop semantics are not implemented.
    pub fn fetch(
        &self,
        method: &str,
        url: &str,
        header_list: &[(String, String)],
        body: Option<&[u8]>,
        completion: impl FnOnce(Result<FetchResponse, String>) + Send + 'static,
    ) -> Result<(), String> {
        let method_c = CString::new(method).map_err(|error| format!("invalid method: {error}"))?;
        let url_c = CString::new(url).map_err(|error| format!("invalid URL: {error}"))?;

        // <https://fetch.spec.whatwg.org/#concept-request-header-list>
        // The C wrapper copies the header strings into the request during
        // the call, so the owned strings and the pointer arrays they back
        // only have to outlive it.
        let mut header_names = Vec::with_capacity(header_list.len());
        let mut header_values = Vec::with_capacity(header_list.len());
        for (name, value) in header_list {
            header_names.push(
                CString::new(name.as_str())
                    .map_err(|error| format!("invalid header name: {error}"))?,
            );
            header_values.push(
                CString::new(value.as_str())
                    .map_err(|error| format!("invalid header value: {error}"))?,
            );
        }
        let header_name_pointers: Vec<*const c_char> =
            header_names.iter().map(|name| name.as_ptr()).collect();
        let header_value_pointers: Vec<*const c_char> =
            header_values.iter().map(|value| value.as_ptr()).collect();

        // Double box: the outer box is a thin pointer, so it can be passed
        // through the C callback as a `void *` context and recovered as a
        // `FetchCompletion` in the trampoline.
        let callback: Box<FetchCompletion> = Box::new(Box::new(completion));
        let context = Box::into_raw(callback) as *mut c_void;

        // SAFETY: the C strings are NUL-terminated and live for the duration
        // of the call (the C wrapper copies them), the header arrays hold
        // `header_list.len()` such pointers each, the body pointer and
        // length are consistent, `context` points at the boxed callback
        // which the trampoline reclaims exactly once, and `completion` is
        // invoked exactly once — or not at all when the task cannot be
        // started, in which case the box is reclaimed below.
        let result = unsafe {
            ffi::fw_url_session_fetch(
                self.handle,
                method_c.as_ptr(),
                url_c.as_ptr(),
                header_name_pointers.as_ptr(),
                header_value_pointers.as_ptr(),
                header_list.len(),
                body.map_or(std::ptr::null(), |bytes| bytes.as_ptr()),
                body.map_or(0, |bytes| bytes.len()),
                context,
                Some(completion_trampoline),
            )
        };
        if result != 0 {
            // The task could not be started; the completion callback was not
            // invoked, so reclaim the callback box.
            // SAFETY: `context` is the `Box::into_raw` result above, only
            // reclaimed here because the C side did not take ownership.
            let callback = unsafe { Box::from_raw(context as *mut FetchCompletion) };
            drop(callback);
            return Err(String::from("failed to start URLSession data task"));
        }
        Ok(())
    }
}

impl Drop for UrlSession {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this session and released once;
        // `fw_url_session_release` is NULL-safe.
        unsafe { ffi::fw_url_session_release(self.handle) };
    }
}

unsafe extern "C" fn completion_trampoline(
    context: *mut c_void,
    status_code: std::os::raw::c_int,
    final_url: *const std::os::raw::c_char,
    content_type: *const std::os::raw::c_char,
    body: *const u8,
    body_length: usize,
    error: *const std::os::raw::c_char,
) {
    // SAFETY: `context` is the `Box::into_raw` result from `UrlSession::fetch`
    // and is reclaimed exactly once per fetch (the C wrapper invokes the
    // completion exactly once, or never for a task that could not start).
    let callback = unsafe { Box::from_raw(context as *mut FetchCompletion) };
    let result = if !error.is_null() {
        // SAFETY: `error` is a NUL-terminated C string valid for the call.
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        Err(message)
    } else {
        // SAFETY: the pointer arguments are only valid for the duration of
        // the call; they are copied out here before the trampoline returns.
        let final_url = if final_url.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(final_url) }
                .to_string_lossy()
                .into_owned()
        };
        let content_type = if content_type.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(content_type) }
                .to_string_lossy()
                .into_owned()
        };
        let body = if body.is_null() || body_length == 0 {
            Vec::new()
        } else {
            // SAFETY: `body` points at `body_length` initialized bytes
            // valid for the duration of the call.
            unsafe { std::slice::from_raw_parts(body, body_length) }.to_vec()
        };
        Ok(FetchResponse {
            status: status_code as u16,
            final_url,
            content_type,
            body,
        })
    };
    callback(result);
}
