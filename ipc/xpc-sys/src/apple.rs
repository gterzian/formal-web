//! Apple-specific XPC FFI bindings and safe wrappers.
//! Only compiled when `target_vendor = "apple"`.

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::{Arc, Mutex};

pub enum xpc_object_t_private {}
pub type xpc_object_t = *mut xpc_object_t_private;
pub enum xpc_connection_t_private {}
pub type xpc_connection_t = *mut xpc_connection_t_private;
pub type dispatch_queue_t = *mut c_void;

pub type XpcListenerEventCallback = unsafe extern "C" fn(event: xpc_object_t, context: *mut c_void);
pub type XpcPeerMessageCallback =
    unsafe extern "C" fn(dictionary: xpc_object_t, context: *mut c_void);

unsafe extern "C" {
    pub fn fw_xpc_create_listener(
        service_name: *const c_char,
        queue: dispatch_queue_t,
        callback: Option<XpcListenerEventCallback>,
        context: *mut c_void,
    ) -> xpc_connection_t;
    pub fn fw_xpc_create_client(
        service_name: *const c_char,
        queue: dispatch_queue_t,
        callback: Option<XpcPeerMessageCallback>,
        context: *mut c_void,
    ) -> xpc_connection_t;
    pub fn fw_xpc_set_listener_handler(
        listener: xpc_connection_t,
        queue: dispatch_queue_t,
        callback: Option<XpcListenerEventCallback>,
        context: *mut c_void,
    );
    pub fn fw_xpc_set_peer_handler(
        peer: xpc_connection_t,
        queue: dispatch_queue_t,
        callback: Option<XpcPeerMessageCallback>,
        context: *mut c_void,
    );
    pub fn fw_xpc_peer_from_event(event: xpc_object_t) -> xpc_connection_t;
    pub fn fw_xpc_resume(connection: xpc_connection_t);
    pub fn fw_xpc_cancel(connection: xpc_connection_t);
}

unsafe extern "C" {
    pub fn xpc_retain(object: xpc_object_t) -> xpc_object_t;
    pub fn xpc_release(object: xpc_object_t);
    pub fn xpc_get_type(object: xpc_object_t) -> *const c_void;
    pub fn xpc_dictionary_create(
        keys: *mut *mut c_char,
        values: *mut xpc_object_t,
        count: usize,
    ) -> xpc_object_t;
    pub fn xpc_dictionary_get_string(dict: xpc_object_t, key: *const c_char) -> *const c_char;
    pub fn xpc_dictionary_set_string(dict: xpc_object_t, key: *const c_char, value: *const c_char);
    pub fn xpc_dictionary_get_int64(dict: xpc_object_t, key: *const c_char) -> i64;
    pub fn xpc_dictionary_set_int64(dict: xpc_object_t, key: *const c_char, value: i64);
    pub fn xpc_dictionary_get_uint64(dict: xpc_object_t, key: *const c_char) -> u64;
    pub fn xpc_dictionary_set_uint64(dict: xpc_object_t, key: *const c_char, value: u64);
    pub fn xpc_dictionary_get_bool(dict: xpc_object_t, key: *const c_char) -> bool;
    pub fn xpc_dictionary_set_bool(dict: xpc_object_t, key: *const c_char, value: bool);
    pub fn xpc_dictionary_get_data(
        dict: xpc_object_t,
        key: *const c_char,
        length: *mut usize,
    ) -> *const c_void;
    pub fn xpc_dictionary_set_data(
        dict: xpc_object_t,
        key: *const c_char,
        value: *const c_void,
        length: usize,
    );
    pub fn xpc_dictionary_get_value(dict: xpc_object_t, key: *const c_char) -> xpc_object_t;
    pub fn xpc_dictionary_set_value(dict: xpc_object_t, key: *const c_char, value: xpc_object_t);
    pub fn xpc_connection_send_message(connection: xpc_connection_t, message: xpc_object_t);
    pub fn xpc_shmem_create(region: *mut c_void, size: usize) -> xpc_object_t;
    pub fn xpc_shmem_map(shmem: xpc_object_t) -> *mut c_void;
    pub fn xpc_shmem_get_length(shmem: xpc_object_t) -> usize;
    pub fn dispatch_queue_create(label: *const c_char, attr: dispatch_queue_t) -> dispatch_queue_t;
    pub fn dispatch_retain(object: dispatch_queue_t) -> dispatch_queue_t;
    pub fn dispatch_release(object: dispatch_queue_t);
}

pub struct XpcObject {
    inner: xpc_object_t,
}
impl XpcObject {
    pub unsafe fn from_raw(inner: xpc_object_t) -> Self {
        XpcObject { inner }
    }
    pub fn as_raw(&self) -> xpc_object_t {
        self.inner
    }
    pub fn into_raw(self) -> xpc_object_t {
        let raw = self.inner;
        std::mem::forget(self);
        raw
    }
}
impl Drop for XpcObject {
    fn drop(&mut self) {
        unsafe { xpc_release(self.inner) }
    }
}
impl Clone for XpcObject {
    fn clone(&self) -> Self {
        unsafe {
            XpcObject {
                inner: xpc_retain(self.inner),
            }
        }
    }
}

pub struct XpcDictionary {
    object: XpcObject,
}
impl XpcDictionary {
    pub fn new() -> Self {
        unsafe {
            XpcDictionary {
                object: XpcObject::from_raw(xpc_dictionary_create(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                )),
            }
        }
    }
    pub unsafe fn from_object(object: XpcObject) -> Self {
        XpcDictionary { object }
    }
    pub fn as_raw(&self) -> xpc_object_t {
        self.object.as_raw()
    }
    pub fn into_object(self) -> XpcObject {
        self.object
    }
    pub fn set_string(&mut self, key: &str, value: &str) {
        let ck = CString::new(key).unwrap();
        let cv = CString::new(value).unwrap();
        unsafe {
            xpc_dictionary_set_string(self.object.as_raw(), ck.as_ptr(), cv.as_ptr());
        }
    }
    pub fn get_string(&self, key: &str) -> Option<String> {
        let ck = CString::new(key).unwrap();
        unsafe {
            let p = xpc_dictionary_get_string(self.object.as_raw(), ck.as_ptr());
            if p.is_null() {
                None
            } else {
                Some(CStr::from_ptr(p).to_string_lossy().into_owned())
            }
        }
    }
    pub fn set_int64(&mut self, key: &str, value: i64) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_int64(self.object.as_raw(), ck.as_ptr(), value);
        }
    }
    pub fn get_int64(&self, key: &str) -> Option<i64> {
        let ck = CString::new(key).unwrap();
        unsafe {
            if xpc_dictionary_get_value(self.object.as_raw(), ck.as_ptr()).is_null() {
                None
            } else {
                Some(xpc_dictionary_get_int64(self.object.as_raw(), ck.as_ptr()))
            }
        }
    }
    pub fn set_uint64(&mut self, key: &str, value: u64) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_uint64(self.object.as_raw(), ck.as_ptr(), value);
        }
    }
    pub fn get_uint64(&self, key: &str) -> Option<u64> {
        let ck = CString::new(key).unwrap();
        unsafe {
            if xpc_dictionary_get_value(self.object.as_raw(), ck.as_ptr()).is_null() {
                None
            } else {
                Some(xpc_dictionary_get_uint64(self.object.as_raw(), ck.as_ptr()))
            }
        }
    }
    pub fn set_bool(&mut self, key: &str, value: bool) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_bool(self.object.as_raw(), ck.as_ptr(), value);
        }
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        let ck = CString::new(key).unwrap();
        unsafe {
            if xpc_dictionary_get_value(self.object.as_raw(), ck.as_ptr()).is_null() {
                None
            } else {
                Some(xpc_dictionary_get_bool(self.object.as_raw(), ck.as_ptr()))
            }
        }
    }
    pub fn set_data(&mut self, key: &str, value: &[u8]) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_data(
                self.object.as_raw(),
                ck.as_ptr(),
                value.as_ptr() as *const c_void,
                value.len(),
            );
        }
    }
    pub fn get_data(&self, key: &str) -> Option<&[u8]> {
        let ck = CString::new(key).unwrap();
        unsafe {
            let mut l = 0;
            let p = xpc_dictionary_get_data(self.object.as_raw(), ck.as_ptr(), &mut l);
            if p.is_null() {
                None
            } else {
                Some(std::slice::from_raw_parts(p as *const u8, l))
            }
        }
    }
    pub fn set_shmem(&mut self, key: &str, shmem: &XpcSharedMemory) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_value(self.object.as_raw(), ck.as_ptr(), shmem.object.as_raw());
        }
    }
    pub fn set_dict(&mut self, key: &str, value: &XpcDictionary) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_value(self.object.as_raw(), ck.as_ptr(), value.object.as_raw());
        }
    }
    pub fn get_dict(&self, key: &str) -> Option<XpcDictionary> {
        let ck = CString::new(key).unwrap();
        unsafe {
            let v = xpc_dictionary_get_value(self.object.as_raw(), ck.as_ptr());
            if v.is_null() {
                None
            } else {
                Some(XpcDictionary {
                    object: XpcObject::from_raw(xpc_retain(v)),
                })
            }
        }
    }
}

// ── XpcSharedMemory ─────────────────────────────────────────────────────────

pub struct XpcSharedMemory {
    pub(crate) object: XpcObject,
    ptr: *mut u8,
    size: usize,
    // Tracks ownership of the mmap region:
    // - `false` for `allocate()`:   XPC owns the mapping (xpc_release unmaps it).
    // - `true`  for `map_object()`: caller must munmap.
    needs_munmap: bool,
}
impl XpcSharedMemory {
    pub fn allocate(size: usize) -> Result<Self, String> {
        unsafe {
            let p = libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANON | libc::MAP_SHARED,
                -1,
                0,
            );
            if p == libc::MAP_FAILED {
                return Err("mmap failed".into());
            }
            let s = xpc_shmem_create(p, size);
            if s.is_null() {
                libc::munmap(p, size);
                return Err("xpc_shmem_create failed".into());
            }
            Ok(XpcSharedMemory {
                object: XpcObject::from_raw(s),
                ptr: p as *mut u8,
                size,
                needs_munmap: false,
            })
        }
    }
    pub unsafe fn map_object(object: XpcObject) -> Result<Self, String> {
        let p = unsafe { xpc_shmem_map(object.as_raw()) };
        if p.is_null() {
            return Err("xpc_shmem_map failed".into());
        }
        let s = unsafe { xpc_shmem_get_length(object.as_raw()) };
        Ok(XpcSharedMemory {
            object,
            ptr: p as *mut u8,
            size: s,
            needs_munmap: true,
        })
    }
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }
    pub fn size(&self) -> usize {
        self.size
    }
}
impl Drop for XpcSharedMemory {
    fn drop(&mut self) {
        if self.needs_munmap {
            unsafe {
                libc::munmap(self.ptr as *mut libc::c_void, self.size);
            }
        }
    }
}

// ── Shared context for callback closures ────────────────────────────────────

/// A type-erased closure pointer and its cleanup function.
struct ContextEntry {
    ptr: *mut c_void,
    cleanup: unsafe fn(*mut c_void),
}

/// Shared context slot between the C callback and `XpcConnection::drop`.
/// The Mutex ensures exclusive access: while the callback holds the lock,
/// Drop cannot free the context, and vice versa.
struct SharedContext(Arc<Mutex<Option<ContextEntry>>>);

impl SharedContext {
    fn new(ptr: *mut c_void, cleanup: unsafe fn(*mut c_void)) -> Self {
        SharedContext(Arc::new(Mutex::new(Some(ContextEntry { ptr, cleanup }))))
    }
}

unsafe fn cleanup_msg_handler(ptr: *mut c_void) {
    unsafe {
        let _ = Box::from_raw(ptr as *mut Box<dyn Fn(XpcMessageEvent) + Send>);
    }
}

unsafe fn cleanup_listener_handler(ptr: *mut c_void) {
    unsafe {
        let _ = Box::from_raw(ptr as *mut Box<dyn Fn(XpcListenerEvent) + Send>);
    }
}

/// Internal callback shared by `connect()` and `set_message_handler()`.
/// The XPC error description is parsed from the event object to detect
/// invalidation. The context cleanup runs while the lock is held to prevent
/// races with `Drop`.
unsafe extern "C" fn xpc_peer_callback(object: xpc_object_t, context: *mut c_void) {
    let shared = unsafe { &*(context as *const SharedContext) };
    let mut guard = match shared.0.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let handler_ptr = match guard.as_ref().map(|e| e.ptr) {
        Some(p) => p,
        None => return, // Context already freed
    };
    let handler = unsafe { &*(handler_ptr as *mut Box<dyn Fn(XpcMessageEvent) + Send>) };

    let error_key = CString::new("XPCErrorDescription").unwrap();
    let error_str = unsafe { xpc_dictionary_get_string(object, error_key.as_ptr()) };
    if !error_str.is_null() {
        let msg = unsafe { CStr::from_ptr(error_str) }
            .to_string_lossy()
            .into_owned();
        if msg.contains("invalidated") || msg.contains("Interrupted") {
            handler(XpcMessageEvent::Invalidated);
            // Free the context while still holding the lock.
            if let Some(entry) = guard.take() {
                unsafe {
                    (entry.cleanup)(entry.ptr);
                }
            }
        } else {
            handler(XpcMessageEvent::Error(msg));
        }
        return;
    }
    let dict = unsafe { XpcObject::from_raw(xpc_retain(object)) };
    handler(XpcMessageEvent::Message(XpcDictionary { object: dict }));
    // guard dropped — lock released, Drop can now safely clean up
}

/// Internal callback for listener events.
unsafe extern "C" fn xpc_listener_callback(event: xpc_object_t, context: *mut c_void) {
    let shared = unsafe { &*(context as *const SharedContext) };
    let guard = match shared.0.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let handler_ptr = match guard.as_ref().map(|e| e.ptr) {
        Some(p) => p,
        None => return,
    };
    let handler = unsafe { &*(handler_ptr as *mut Box<dyn Fn(XpcListenerEvent) + Send>) };

    let error_key = CString::new("XPCErrorDescription").unwrap();
    let error_str = unsafe { xpc_dictionary_get_string(event, error_key.as_ptr()) };
    if !error_str.is_null() {
        let msg = unsafe { CStr::from_ptr(error_str) }
            .to_string_lossy()
            .into_owned();
        handler(XpcListenerEvent::Error(msg));
        return;
    }
    let peer_inner = unsafe { fw_xpc_peer_from_event(event) };
    let peer_queue = create_queue("com.formal-web.xpc-peer");
    handler(XpcListenerEvent::NewPeer(XpcConnection {
        inner: peer_inner,
        _queue: peer_queue,
        context: Arc::new(Mutex::new(None)),
    }));
    // guard dropped
}

// ── XpcConnection ───────────────────────────────────────────────────────────

pub struct XpcConnection {
    inner: xpc_connection_t,
    _queue: XpcQueue,
    /// The C callback context slot. Shared via Arc so that clones (which share
    /// the same underlying XPC connection via xpc_retain) also share the
    /// cleanup lifecycle.
    context: Arc<Mutex<Option<ContextEntry>>>,
}
unsafe impl Send for XpcConnection {}
unsafe impl Sync for XpcConnection {}
impl Clone for XpcConnection {
    fn clone(&self) -> Self {
        XpcConnection {
            inner: unsafe { xpc_retain(self.inner as xpc_object_t) as xpc_connection_t },
            _queue: self._queue.clone(),
            context: self.context.clone(),
        }
    }
}
impl Drop for XpcConnection {
    fn drop(&mut self) {
        unsafe {
            // Cancel first — after this no more callbacks will fire.
            fw_xpc_cancel(self.inner);
        }
        // Try to clean up the context. If the invalidation callback already
        // freed it (while holding the lock), `take()` returns None.
        if let Ok(mut guard) = self.context.lock() {
            if let Some(entry) = guard.take() {
                unsafe {
                    (entry.cleanup)(entry.ptr);
                }
            }
        }
        unsafe {
            xpc_release(self.inner as xpc_object_t);
        }
    }
}

impl XpcConnection {
    // ── connect ──────────────────────────────────────────────────────────

    pub fn connect<F: Fn(XpcMessageEvent) + Send + 'static>(
        service_name: &str,
        handler: F,
    ) -> Self {
        let c_name = CString::new(service_name).unwrap();
        let queue = create_queue(&format!("com.formal-web.xpc-client.{}", service_name));
        // Double-indirection: Box the closure as a trait object (fat pointer),
        // then box the fat pointer so C sees a thin pointer.
        let trait_obj: Box<dyn Fn(XpcMessageEvent) + Send> = Box::new(handler);
        let closure_ptr = Box::into_raw(Box::new(trait_obj)) as *mut c_void;

        let shared = SharedContext::new(closure_ptr, cleanup_msg_handler);
        let c_context = Box::into_raw(Box::new(shared)) as *mut c_void;

        let inner = unsafe {
            fw_xpc_create_client(
                c_name.as_ptr(),
                queue.inner,
                Some(xpc_peer_callback as XpcPeerMessageCallback),
                c_context,
            )
        };
        XpcConnection {
            inner,
            _queue: queue,
            context: Arc::new(Mutex::new(Some(ContextEntry {
                ptr: c_context,
                cleanup: |p| {
                    let _ = unsafe { Box::from_raw(p as *mut SharedContext) };
                },
            }))),
        }
    }

    // ── listen ───────────────────────────────────────────────────────────

    pub fn listen<F: Fn(XpcListenerEvent) + Send + 'static>(
        service_name: &str,
        handler: F,
    ) -> Self {
        let c_name = CString::new(service_name).unwrap();
        let queue = create_queue(&format!("com.formal-web.xpc-listener.{}", service_name));
        // Double-indirection: Box the closure as a trait object (fat pointer),
        // then box the fat pointer so C sees a thin pointer.
        let trait_obj: Box<dyn Fn(XpcListenerEvent) + Send> = Box::new(handler);
        let closure_ptr = Box::into_raw(Box::new(trait_obj)) as *mut c_void;

        let shared = SharedContext::new(closure_ptr, cleanup_listener_handler);
        let c_context = Box::into_raw(Box::new(shared)) as *mut c_void;

        let inner = unsafe {
            fw_xpc_create_listener(
                c_name.as_ptr(),
                queue.inner,
                Some(xpc_listener_callback as XpcListenerEventCallback),
                c_context,
            )
        };
        XpcConnection {
            inner,
            _queue: queue,
            context: Arc::new(Mutex::new(Some(ContextEntry {
                ptr: c_context,
                cleanup: |p| {
                    let _ = unsafe { Box::from_raw(p as *mut SharedContext) };
                },
            }))),
        }
    }

    // ── set_message_handler ──────────────────────────────────────────────

    pub fn set_message_handler<F: Fn(XpcMessageEvent) + Send + 'static>(&self, handler: F) {
        let queue = create_queue("com.formal-web.xpc-peer-msg");
        // Double-indirection.
        let trait_obj: Box<dyn Fn(XpcMessageEvent) + Send> = Box::new(handler);
        let closure_ptr = Box::into_raw(Box::new(trait_obj)) as *mut c_void;

        let shared = SharedContext::new(closure_ptr, cleanup_msg_handler);
        let c_context = Box::into_raw(Box::new(shared)) as *mut c_void;

        unsafe {
            fw_xpc_set_peer_handler(
                self.inner,
                queue.inner,
                Some(xpc_peer_callback as XpcPeerMessageCallback),
                c_context,
            );
        }

        // Replace the old context with the new one.
        let mut guard = self.context.lock().unwrap();
        if let Some(old) = guard.take() {
            unsafe {
                (old.cleanup)(old.ptr);
            }
        }
        *guard = Some(ContextEntry {
            ptr: c_context,
            cleanup: |p| {
                let _ = unsafe { Box::from_raw(p as *mut SharedContext) };
            },
        });
    }

    // ── lifecycle ────────────────────────────────────────────────────────

    pub fn resume(&self) {
        unsafe { fw_xpc_resume(self.inner) }
    }
    pub fn send_message(&self, message: &XpcDictionary) {
        unsafe {
            xpc_connection_send_message(self.inner, message.as_raw());
        }
    }
    pub fn cancel(&self) {
        unsafe { fw_xpc_cancel(self.inner) }
    }
}

// ── XpcQueue ────────────────────────────────────────────────────────────────

struct XpcQueue {
    inner: dispatch_queue_t,
}
unsafe impl Send for XpcQueue {}
unsafe impl Sync for XpcQueue {}
impl Clone for XpcQueue {
    fn clone(&self) -> Self {
        XpcQueue {
            inner: unsafe { dispatch_retain(self.inner) },
        }
    }
}
impl Drop for XpcQueue {
    fn drop(&mut self) {
        unsafe { dispatch_release(self.inner) }
    }
}
fn create_queue(label: &str) -> XpcQueue {
    unsafe {
        let c = CString::new(label).unwrap();
        XpcQueue {
            inner: dispatch_queue_create(c.as_ptr(), ptr::null_mut()),
        }
    }
}

// ── Event types ─────────────────────────────────────────────────────────────

pub enum XpcListenerEvent {
    NewPeer(XpcConnection),
    Error(String),
}
pub enum XpcMessageEvent {
    Message(XpcDictionary),
    Invalidated,
    Error(String),
}
