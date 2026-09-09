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

// XPC type constants for runtime type checks.
// Comparing xpc_get_type() result against these avoids API misuse (calling
// dictionary functions on connection/error objects).
unsafe extern "C" {
    static _xpc_type_connection: c_void;
    static _xpc_type_error: c_void;
    static _xpc_type_dictionary: c_void;
    static _xpc_type_shmem: c_void;
    static _xpc_type_endpoint: c_void;
}

pub type XpcListenerEventCallback = unsafe extern "C" fn(event: xpc_object_t, context: *mut c_void);
pub type XpcPeerMessageCallback =
    unsafe extern "C" fn(dictionary: xpc_object_t, context: *mut c_void);

unsafe extern "C" {
    #[cfg(target_os = "macos")]
    pub fn fw_xpc_create_listener(
        service_name: *const c_char,
        queue: dispatch_queue_t,
        callback: Option<XpcListenerEventCallback>,
        context: *mut c_void,
    ) -> xpc_connection_t;
    #[cfg(target_os = "macos")]
    pub fn fw_xpc_create_client(
        service_name: *const c_char,
        queue: dispatch_queue_t,
        callback: Option<XpcPeerMessageCallback>,
        context: *mut c_void,
    ) -> xpc_connection_t;
    #[cfg(target_os = "macos")]
    pub fn fw_xpc_create_connection(
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
    #[cfg(target_os = "macos")]
    pub fn fw_xpc_run_service(
        handler: Option<unsafe extern "C" fn(xpc_connection_t, *mut c_void)>,
        context: *mut c_void,
    );
    // dispatch_main is not declared — we use a custom xpc_main wrapper.
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
    // Real signature: `size_t xpc_shmem_map(xpc_object_t, void **region)` —
    // the mapped region is written to the out-param and the length returned.
    pub fn xpc_shmem_map(shmem: xpc_object_t, region: *mut *mut c_void) -> usize;
    pub fn xpc_shmem_get_length(shmem: xpc_object_t) -> usize;
    pub fn xpc_bool_create(value: bool) -> xpc_object_t;
    pub fn xpc_connection_create(
        name: *const c_char,
        targetq: dispatch_queue_t,
    ) -> xpc_connection_t;
    pub fn xpc_endpoint_create(connection: xpc_connection_t) -> xpc_object_t;
    pub fn xpc_connection_create_from_endpoint(
        endpoint: xpc_object_t,
        queue: dispatch_queue_t,
    ) -> xpc_connection_t;
    pub fn xpc_connection_set_peer_team_identity_requirement(
        connection: xpc_connection_t,
        signing_identifier: *const c_char,
    ) -> i32;
    pub fn xpc_connection_set_peer_entitlement_matches_value_requirement(
        connection: xpc_connection_t,
        entitlement: *const c_char,
        value: xpc_object_t,
    ) -> i32;
    pub fn dispatch_queue_create(label: *const c_char, attr: dispatch_queue_t) -> dispatch_queue_t;
    pub fn dispatch_retain(object: dispatch_queue_t) -> dispatch_queue_t;
    pub fn dispatch_release(object: dispatch_queue_t);
}

pub struct XpcObject {
    inner: xpc_object_t,
}
impl XpcObject {
    /// Wrap a raw retained XPC object.
    ///
    /// # Safety
    ///
    /// `inner` must be a valid XPC object reference that the caller owns;
    /// the wrapper releases it on drop.
    pub unsafe fn from_raw(inner: xpc_object_t) -> Self {
        XpcObject { inner }
    }
    pub fn as_raw(&self) -> xpc_object_t {
        self.inner
    }
    /// Create a bool object, e.g. as the expected value of a
    /// peer-entitlement requirement.
    pub fn new_bool(value: bool) -> Self {
        unsafe { XpcObject::from_raw(xpc_bool_create(value)) }
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
}
impl Default for XpcDictionary {
    fn default() -> Self {
        Self::new()
    }
}
impl XpcDictionary {
    /// Wrap an owned XPC dictionary object.
    ///
    /// # Safety
    ///
    /// `object` must be a valid dictionary object the caller owns; the
    /// wrapper releases it on drop.
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
    /// Set an arbitrary XPC object (e.g. an endpoint or shmem region) under
    /// `key`. `xpc_dictionary_set_value` retains the object.
    pub fn set_object(&mut self, key: &str, object: &XpcObject) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_value(self.object.as_raw(), ck.as_ptr(), object.as_raw());
        }
    }
    /// Get an arbitrary XPC object stored under `key` (retained for the
    /// caller).
    pub fn get_object(&self, key: &str) -> Option<XpcObject> {
        let ck = CString::new(key).unwrap();
        unsafe {
            let v = xpc_dictionary_get_value(self.object.as_raw(), ck.as_ptr());
            if v.is_null() {
                None
            } else {
                Some(XpcObject::from_raw(xpc_retain(v)))
            }
        }
    }
    /// Set an anonymous-endpoint object under `key`. Endpoints are
    /// transferable only inside XPC messages, never as byte blobs, so they
    /// travel as dictionary values.
    pub fn set_endpoint(&mut self, key: &str, endpoint: &XpcEndpoint) {
        let ck = CString::new(key).unwrap();
        unsafe {
            xpc_dictionary_set_value(self.object.as_raw(), ck.as_ptr(), endpoint.as_raw());
        }
    }
    /// Get an anonymous-endpoint object stored under `key`, if the value
    /// there is actually an XPC endpoint.
    pub fn get_endpoint(&self, key: &str) -> Option<XpcEndpoint> {
        let object = self.get_object(key)?;
        unsafe {
            if std::ptr::eq(
                xpc_get_type(object.as_raw()),
                &_xpc_type_endpoint as *const c_void,
            ) {
                Some(XpcEndpoint { object })
            } else {
                None
            }
        }
    }
    /// Get a shared-memory region stored under `key`, if the value there is
    /// actually an XPC shmem object.
    pub fn get_shmem(&self, key: &str) -> Option<XpcSharedMemory> {
        let object = self.get_object(key)?;
        unsafe {
            if std::ptr::eq(
                xpc_get_type(object.as_raw()),
                &_xpc_type_shmem as *const c_void,
            ) {
                XpcSharedMemory::map_object(object).ok()
            } else {
                None
            }
        }
    }
}

// ── XpcEndpoint ────────────────────────────────────────────────────────────

/// An anonymous XPC endpoint: a transferable reference to an anonymous
/// listener that another process connects to with
/// `xpc_connection_create_from_endpoint`. Endpoints travel inside XPC
/// messages (as dictionary values), never as arbitrary byte blobs.
pub struct XpcEndpoint {
    object: XpcObject,
}
impl XpcEndpoint {
    /// Wrap an anonymous listener connection as an endpoint.
    pub fn from_connection(connection: &XpcConnection) -> Self {
        unsafe {
            XpcEndpoint {
                object: XpcObject::from_raw(xpc_endpoint_create(connection.as_raw())),
            }
        }
    }
    pub fn as_raw(&self) -> xpc_object_t {
        self.object.as_raw()
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
    /// Wrap an existing writable memory region as an XPC shared-memory
    /// object for transfer in a message. The caller keeps ownership of the
    /// region; releasing the object does not unmap it.
    ///
    /// # Safety
    ///
    /// `ptr` must point to `size` bytes of valid, page-backed memory that
    /// stays alive until the message carrying this object has been sent.
    pub unsafe fn wrap(ptr: *mut c_void, size: usize) -> Self {
        unsafe {
            XpcSharedMemory {
                object: XpcObject::from_raw(xpc_shmem_create(ptr, size)),
                ptr: ptr as *mut u8,
                size,
                needs_munmap: false,
            }
        }
    }
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
    /// Map an XPC shared-memory object into this process's address space.
    /// The returned region must be unmapped by the caller (the wrapper's
    /// `Drop` does this via `munmap`).
    ///
    /// # Safety
    ///
    /// `object` must be a valid XPC shmem object and must outlive the
    /// returned mapping.
    pub unsafe fn map_object(object: XpcObject) -> Result<Self, String> {
        let mut region: *mut c_void = ptr::null_mut();
        // xpc_shmem_map writes the mapped region into the out-param and
        // returns its length.
        let size = unsafe { xpc_shmem_map(object.as_raw(), &mut region) };
        if region.is_null() {
            return Err("xpc_shmem_map failed".into());
        }
        Ok(XpcSharedMemory {
            object,
            ptr: region as *mut u8,
            size,
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

// SAFETY: the raw pointer is only ever dereferenced while holding the
// `entries` mutex (or never — late mach-cancel callbacks bail out on the
// `alive` flag before touching it), and `XpcConnection` itself is already
// declared `Send + Sync` on the same grounds.
unsafe impl Send for ContextEntry {}
unsafe impl Sync for ContextEntry {}
unsafe impl Send for SharedContext {}
unsafe impl Sync for SharedContext {}

/// Shared context slot between the C callback and `XpcConnection::drop`.
/// The Mutex ensures exclusive access: while the callback holds the lock,
/// Drop cannot free the context, and vice versa.
///
/// The `alive` flag is a separate heap allocation that is NEVER freed.
/// This is necessary because the C wrapper's Block captures the context
/// pointer by value, and mach cancel events on macOS 26+ can fire after
/// Drop has freed the SharedContext's entries. By checking the alive flag
/// first (which survives Drop), callbacks can safely bail out.
struct SharedContext {
    entries: Arc<Mutex<Option<ContextEntry>>>,
    alive: Box<std::sync::atomic::AtomicBool>,
}

impl SharedContext {
    fn new(ptr: *mut c_void, cleanup: unsafe fn(*mut c_void)) -> Self {
        SharedContext {
            entries: Arc::new(Mutex::new(Some(ContextEntry { ptr, cleanup }))),
            alive: Box::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    fn invalidate(&self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Release);
    }

    fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Acquire)
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
/// Checks the XPC type before calling dictionary functions to avoid
/// API misuse on macOS 26+ where calling dict functions on non-dict
/// objects triggers _xpc_api_misuse / SIGTRAP.
unsafe extern "C" fn xpc_peer_callback(object: xpc_object_t, context: *mut c_void) {
    // Check pointer validity BEFORE accessing shared context.
    // On macOS 26+, mach cancel events deliver invalid small-integer pointers.
    // The shared context may have been freed by XpcConnection::drop by the
    // time this callback fires, so avoid touching context for invalid objects.
    if object.is_null() || (object as usize) < 0x100_0000 {
        return;
    }

    let shared = unsafe { &*(context as *const SharedContext) };
    if !shared.is_alive() {
        return;
    }
    let mut guard = match shared.entries.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let handler_ptr = match guard.as_ref().map(|e| e.ptr) {
        Some(p) => p,
        None => return,
    };
    let handler = unsafe { &*(handler_ptr as *mut Box<dyn Fn(XpcMessageEvent) + Send>) };

    let object_type = unsafe { xpc_get_type(object) };
    let is_dictionary =
        unsafe { std::ptr::eq(object_type, &_xpc_type_dictionary as *const c_void) };

    if is_dictionary {
        let dict = unsafe { XpcObject::from_raw(xpc_retain(object)) };
        handler(XpcMessageEvent::Message(XpcDictionary { object: dict }));
    } else {
        // Non-dictionary: error or invalidation.
        let error_key = CString::new("XPCErrorDescription").unwrap();
        // Only call dict getters on error objects (not on connection/other types).
        let error_str = if unsafe { std::ptr::eq(object_type, &_xpc_type_error as *const c_void) } {
            unsafe { xpc_dictionary_get_string(object, error_key.as_ptr()) }
        } else {
            std::ptr::null()
        };
        if !error_str.is_null() {
            let msg = unsafe { CStr::from_ptr(error_str) }
                .to_string_lossy()
                .into_owned();
            if msg.contains("invalidated") || msg.contains("Interrupted") {
                handler(XpcMessageEvent::Invalidated);
                if let Some(entry) = guard.take() {
                    unsafe {
                        (entry.cleanup)(entry.ptr);
                    }
                }
            } else {
                handler(XpcMessageEvent::Error(msg));
            }
        } else {
            // Non-dictionary, non-error: treat as invalidation.
            handler(XpcMessageEvent::Invalidated);
        }
    }
}

/// Internal callback for listener events.
/// Checks XPC type before calling dict functions — on macOS 26+ the event
/// is a direct XPC_TYPE_CONNECTION (peer), not a dictionary.
unsafe extern "C" fn xpc_listener_callback(event: xpc_object_t, context: *mut c_void) {
    // Check pointer validity BEFORE accessing shared context.
    if event.is_null() || (event as usize) < 0x100_0000 {
        return;
    }

    let shared = unsafe { &*(context as *const SharedContext) };
    if !shared.is_alive() {
        return;
    }
    let guard = match shared.entries.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let handler_ptr = match guard.as_ref().map(|e| e.ptr) {
        Some(p) => p,
        None => return,
    };
    let handler = unsafe { &*(handler_ptr as *mut Box<dyn Fn(XpcListenerEvent) + Send>) };

    let event_type = unsafe { xpc_get_type(event) };

    if unsafe { std::ptr::eq(event_type, &_xpc_type_connection as *const c_void) } {
        // New peer connection.
        let peer_inner = unsafe { fw_xpc_peer_from_event(event) };
        let peer_queue = create_queue("com.formal-web.xpc-peer");
        handler(XpcListenerEvent::NewPeer(XpcConnection {
            inner: peer_inner,
            _queue: peer_queue,
            context: Arc::new(Mutex::new(None)),
        }));
    } else if unsafe { std::ptr::eq(event_type, &_xpc_type_error as *const c_void) } {
        // Error / invalidation.
        let error_key = CString::new("XPCErrorDescription").unwrap();
        let error_str = unsafe { xpc_dictionary_get_string(event, error_key.as_ptr()) };
        let msg = if !error_str.is_null() {
            unsafe { CStr::from_ptr(error_str) }
                .to_string_lossy()
                .into_owned()
        } else {
            String::from("listener error")
        };
        handler(XpcListenerEvent::Error(msg));
    } else if unsafe { std::ptr::eq(event_type, &_xpc_type_dictionary as *const c_void) } {
        // Dictionary (first message) — pass as peer via fw_xpc_peer_from_event
        // which casts to xpc_connection_t.
        let peer_inner = unsafe { fw_xpc_peer_from_event(event) };
        let peer_queue = create_queue("com.formal-web.xpc-peer");
        handler(XpcListenerEvent::NewPeer(XpcConnection {
            inner: peer_inner,
            _queue: peer_queue,
            context: Arc::new(Mutex::new(None)),
        }));
    } else {
        handler(XpcListenerEvent::Error(String::from("unknown event type")));
    }
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
impl XpcConnection {
    /// Get the raw `xpc_connection_t` pointer.
    pub fn as_raw(&self) -> xpc_connection_t {
        self.inner
    }
}

impl Drop for XpcConnection {
    fn drop(&mut self) {
        unsafe {
            // Cancel first — after this no more callbacks will fire.
            fw_xpc_cancel(self.inner);
        }
        // Signal to any pending callbacks that the connection is dead,
        // then clean up the handler. The SharedContext allocation itself
        // is intentionally LEAKED so that late-arriving mach cancel
        // events (macOS 26+) can still check the alive flag without
        // accessing freed memory.
        if let Ok(mut guard) = self.context.lock()
            && let Some(entry) = guard.take()
        {
            unsafe {
                let shared = &*(entry.ptr as *const SharedContext);
                shared.invalidate();
                // Free the inner double-boxed closure.
                if let Ok(mut inner_guard) = shared.entries.lock()
                    && let Some(inner_entry) = inner_guard.take()
                {
                    (inner_entry.cleanup)(inner_entry.ptr);
                }
                // Leak the outer Box<SharedContext> — it must stay alive
                // for late callbacks that check is_alive().
                // entry.cleanup is NOT called — it would free the
                // Box<SharedContext>, which we need to keep.
            }
        }
        unsafe {
            xpc_release(self.inner as xpc_object_t);
        }
    }
}

impl XpcConnection {
    // ── connect (launchd Mach service, macOS only) ───────────────────────

    #[cfg(target_os = "macos")]
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

    // ── connect_embedded (embedded XPC service, macOS only) ─────────────

    #[cfg(target_os = "macos")]
    pub fn connect_embedded<F: Fn(XpcMessageEvent) + Send + 'static>(
        service_name: &str,
        handler: F,
    ) -> Self {
        let c_name = CString::new(service_name).unwrap();
        let queue = create_queue(&format!("com.formal-web.xpc-embedded.{}", service_name));
        let trait_obj: Box<dyn Fn(XpcMessageEvent) + Send> = Box::new(handler);
        let closure_ptr = Box::into_raw(Box::new(trait_obj)) as *mut c_void;

        let shared = SharedContext::new(closure_ptr, cleanup_msg_handler);
        let c_context = Box::into_raw(Box::new(shared)) as *mut c_void;

        let inner = unsafe {
            fw_xpc_create_connection(
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

    // ── listen (macOS only) ─────────────────────────────────────────────

    #[cfg(target_os = "macos")]
    pub fn listen<F: Fn(XpcListenerEvent) + Send + 'static>(
        service_name: &str,
        handler: F,
    ) -> Self {
        log::trace!("xpc: listen() creating listener for {}", service_name);
        let c_name = CString::new(service_name).unwrap();
        let queue = create_queue(&format!("com.formal-web.xpc-listener.{}", service_name));
        // Double-indirection: Box the closure as a trait object (fat pointer),
        // then box the fat pointer so C sees a thin pointer.
        let trait_obj: Box<dyn Fn(XpcListenerEvent) + Send> = Box::new(handler);
        let closure_ptr = Box::into_raw(Box::new(trait_obj)) as *mut c_void;
        log::trace!("xpc: listen() closure_ptr={:p}", closure_ptr);

        let shared = SharedContext::new(closure_ptr, cleanup_listener_handler);
        let c_context = Box::into_raw(Box::new(shared)) as *mut c_void;
        log::trace!("xpc: listen() c_context={:p}", c_context);

        log::trace!("xpc: listen() calling fw_xpc_create_listener");
        let inner = unsafe {
            fw_xpc_create_listener(
                c_name.as_ptr(),
                queue.inner,
                Some(xpc_listener_callback as XpcListenerEventCallback),
                c_context,
            )
        };
        log::trace!("xpc: listen() listener={:?}", inner);
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

    /// Create an XpcConnection from a raw `xpc_connection_t` without taking
    /// ownership of the listener setup.  Used with anonymous connections created
    /// via `xpc_connection_create(NULL, queue)`.
    ///
    /// # Safety
    ///
    /// `inner` must be a valid retained `xpc_connection_t`.  The caller must
    /// set up an event handler via `set_listener_handler` or `set_message_handler`
    /// before resuming.
    pub unsafe fn from_raw(inner: xpc_connection_t, queue: XpcQueue) -> Self {
        XpcConnection {
            inner,
            _queue: queue,
            context: Arc::new(Mutex::new(None)),
        }
    }

    /// Set a listener-style event handler on an existing connection.
    /// Used for anonymous listener connections created with `from_raw`.
    pub fn set_listener_handler<F: Fn(XpcListenerEvent) + Send + 'static>(&self, handler: F) {
        let queue = create_queue("com.formal-web.xpc-anon-listener");
        let trait_obj: Box<dyn Fn(XpcListenerEvent) + Send> = Box::new(handler);
        let closure_ptr = Box::into_raw(Box::new(trait_obj)) as *mut c_void;

        let shared = SharedContext::new(closure_ptr, cleanup_listener_handler);
        let c_context = Box::into_raw(Box::new(shared)) as *mut c_void;

        unsafe {
            fw_xpc_set_listener_handler(
                self.inner,
                queue.inner,
                Some(xpc_listener_callback as XpcListenerEventCallback),
                c_context,
            );
        }

        // Store the context so Drop cleans it up.
        let mut guard = self.context.lock().unwrap();
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

    // ── peer code-signing requirements ────────────────────────────────────
    //
    // Require the peer on the other end of this connection to satisfy a
    // code-signing/entitlement check before any message is accepted. Only
    // one of the `xpc_connection_set_peer_*_requirement` family may be set
    // per connection.

    /// Require the peer to be signed with the same team identifier as the
    /// current process and, when `signing_identifier` is given, to carry
    /// that signing identifier.
    ///
    /// Returns `Ok(())` when the requirement was installed; a non-zero
    /// result means the requirement was invalid or already set.
    pub fn require_team_identity(&self, signing_identifier: Option<&str>) -> Result<(), i32> {
        let c_identifier = signing_identifier.map(|id| CString::new(id).unwrap());
        let result = unsafe {
            xpc_connection_set_peer_team_identity_requirement(
                self.inner,
                c_identifier
                    .as_ref()
                    .map_or(ptr::null(), |cstring| cstring.as_ptr()),
            )
        };
        if result == 0 { Ok(()) } else { Err(result) }
    }

    /// Require the peer to hold the given entitlement with the given value
    /// (e.g. `com.apple.developer.web-browser-engine.webcontent` = `true`).
    ///
    /// Returns `Ok(())` when the requirement was installed; a non-zero
    /// result means the requirement was invalid or already set.
    pub fn require_entitlement_value(
        &self,
        entitlement: &str,
        expected: &XpcObject,
    ) -> Result<(), i32> {
        let c_entitlement = CString::new(entitlement).unwrap();
        let result = unsafe {
            xpc_connection_set_peer_entitlement_matches_value_requirement(
                self.inner,
                c_entitlement.as_ptr(),
                expected.as_raw(),
            )
        };
        if result == 0 { Ok(()) } else { Err(result) }
    }
}

// ── XpcQueue ────────────────────────────────────────────────────────────────

pub struct XpcQueue {
    pub inner: dispatch_queue_t,
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
pub fn create_queue(label: &str) -> XpcQueue {
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
