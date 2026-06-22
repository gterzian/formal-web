//! Apple-specific XPC FFI bindings and safe wrappers.
//!
//! This file contains the actual XPC implementation used on macOS.
//! It is only compiled when `target_vendor = "apple"`.

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

// ── Opaque XPC types ────────────────────────────────────────────────────────

pub enum xpc_object_t_private {}
pub type xpc_object_t = *mut xpc_object_t_private;

pub enum xpc_connection_t_private {}
pub type xpc_connection_t = *mut xpc_connection_t_private;

pub type dispatch_queue_t = *mut c_void;

// ── Callback types matching our C wrapper ───────────────────────────────────

pub type XpcListenerEventCallback =
    unsafe extern "C" fn(event: xpc_object_t, context: *mut c_void);

pub type XpcPeerMessageCallback =
    unsafe extern "C" fn(dictionary: xpc_object_t, context: *mut c_void);

// ── FFI imports from xpc_wrapper.c ─────────────────────────────────────────

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

// ── Direct XPC FFI ──────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn xpc_retain(object: xpc_object_t) -> xpc_object_t;
    pub fn xpc_release(object: xpc_object_t);

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

// ── Safe Rust wrappers ──────────────────────────────────────────────────────

pub struct XpcObject { inner: xpc_object_t }

impl XpcObject {
    pub unsafe fn from_raw(inner: xpc_object_t) -> Self { XpcObject { inner } }
    pub fn as_raw(&self) -> xpc_object_t { self.inner }
    pub fn into_raw(self) -> xpc_object_t { let raw = self.inner; std::mem::forget(self); raw }
}

impl Drop for XpcObject { fn drop(&mut self) { unsafe { xpc_release(self.inner) } } }
impl Clone for XpcObject {
    fn clone(&self) -> Self { unsafe { XpcObject { inner: xpc_retain(self.inner) } } }
}

pub struct XpcDictionary { object: XpcObject }

impl XpcDictionary {
    pub fn new() -> Self {
        unsafe { let dict = xpc_dictionary_create(ptr::null_mut(), ptr::null_mut(), 0); XpcDictionary { object: XpcObject::from_raw(dict) } }
    }
    pub unsafe fn from_object(object: XpcObject) -> Self { XpcDictionary { object } }
    pub fn as_raw(&self) -> xpc_object_t { self.object.as_raw() }
    pub fn into_object(self) -> XpcObject { self.object }

    pub fn set_string(&mut self, key: &str, value: &str) {
        let c_key = CString::new(key).unwrap();
        let c_value = CString::new(value).unwrap();
        unsafe { xpc_dictionary_set_string(self.object.as_raw(), c_key.as_ptr(), c_value.as_ptr()); }
    }
    pub fn get_string(&self, key: &str) -> Option<String> {
        let c_key = CString::new(key).unwrap();
        unsafe {
            let ptr = xpc_dictionary_get_string(self.object.as_raw(), c_key.as_ptr());
            if ptr.is_null() { None } else { Some(CStr::from_ptr(ptr).to_string_lossy().into_owned()) }
        }
    }
    pub fn set_int64(&mut self, key: &str, value: i64) {
        let c_key = CString::new(key).unwrap();
        unsafe { xpc_dictionary_set_int64(self.object.as_raw(), c_key.as_ptr(), value); }
    }
    pub fn get_int64(&self, key: &str) -> Option<i64> {
        let c_key = CString::new(key).unwrap();
        unsafe {
            if xpc_dictionary_get_value(self.object.as_raw(), c_key.as_ptr()).is_null() { None }
            else { Some(xpc_dictionary_get_int64(self.object.as_raw(), c_key.as_ptr())) }
        }
    }
    pub fn set_uint64(&mut self, key: &str, value: u64) {
        let c_key = CString::new(key).unwrap();
        unsafe { xpc_dictionary_set_uint64(self.object.as_raw(), c_key.as_ptr(), value); }
    }
    pub fn get_uint64(&self, key: &str) -> Option<u64> {
        let c_key = CString::new(key).unwrap();
        unsafe {
            if xpc_dictionary_get_value(self.object.as_raw(), c_key.as_ptr()).is_null() { None }
            else { Some(xpc_dictionary_get_uint64(self.object.as_raw(), c_key.as_ptr())) }
        }
    }
    pub fn set_bool(&mut self, key: &str, value: bool) {
        let c_key = CString::new(key).unwrap();
        unsafe { xpc_dictionary_set_bool(self.object.as_raw(), c_key.as_ptr(), value); }
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        let c_key = CString::new(key).unwrap();
        unsafe {
            if xpc_dictionary_get_value(self.object.as_raw(), c_key.as_ptr()).is_null() { None }
            else { Some(xpc_dictionary_get_bool(self.object.as_raw(), c_key.as_ptr())) }
        }
    }
    pub fn set_data(&mut self, key: &str, value: &[u8]) {
        let c_key = CString::new(key).unwrap();
        unsafe { xpc_dictionary_set_data(self.object.as_raw(), c_key.as_ptr(), value.as_ptr() as *const c_void, value.len()); }
    }
    pub fn get_data(&self, key: &str) -> Option<&[u8]> {
        let c_key = CString::new(key).unwrap();
        unsafe {
            let mut length: usize = 0;
            let ptr = xpc_dictionary_get_data(self.object.as_raw(), c_key.as_ptr(), &mut length);
            if ptr.is_null() { None } else { Some(std::slice::from_raw_parts(ptr as *const u8, length)) }
        }
    }
    pub fn set_shmem(&mut self, key: &str, shmem: &XpcSharedMemory) {
        let c_key = CString::new(key).unwrap();
        unsafe { xpc_dictionary_set_value(self.object.as_raw(), c_key.as_ptr(), shmem.object.as_raw()); }
    }
    pub fn set_dict(&mut self, key: &str, value: &XpcDictionary) {
        let c_key = CString::new(key).unwrap();
        unsafe { xpc_dictionary_set_value(self.object.as_raw(), c_key.as_ptr(), value.object.as_raw()); }
    }
    pub fn get_dict(&self, key: &str) -> Option<XpcDictionary> {
        let c_key = CString::new(key).unwrap();
        unsafe {
            let value = xpc_dictionary_get_value(self.object.as_raw(), c_key.as_ptr());
            if value.is_null() { None }
            else { Some(XpcDictionary { object: XpcObject::from_raw(xpc_retain(value)) }) }
        }
    }
}

pub struct XpcSharedMemory {
    pub(crate) object: XpcObject,
    ptr: *mut u8,
    size: usize,
}

impl XpcSharedMemory {
    pub fn allocate(size: usize) -> Result<Self, String> {
        unsafe {
            let ptr = libc::mmap(ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_ANON | libc::MAP_SHARED, -1, 0);
            if ptr == libc::MAP_FAILED { return Err("mmap failed".into()); }
            let shmem_obj = xpc_shmem_create(ptr, size);
            if shmem_obj.is_null() { libc::munmap(ptr, size); return Err("xpc_shmem_create failed".into()); }
            Ok(XpcSharedMemory { object: XpcObject::from_raw(shmem_obj), ptr: ptr as *mut u8, size })
        }
    }
    pub unsafe fn map_object(object: XpcObject) -> Result<Self, String> {
        let ptr = unsafe { xpc_shmem_map(object.as_raw()) };
        if ptr.is_null() { return Err("xpc_shmem_map failed".into()); }
        let size = unsafe { xpc_shmem_get_length(object.as_raw()) };
        Ok(XpcSharedMemory { object, ptr: ptr as *mut u8, size })
    }
    pub fn as_slice(&self) -> &[u8] { unsafe { std::slice::from_raw_parts(self.ptr, self.size) } }
    pub fn as_mut_slice(&mut self) -> &mut [u8] { unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) } }
    pub fn size(&self) -> usize { self.size }
}

impl Drop for XpcSharedMemory { fn drop(&mut self) {} }

pub struct XpcConnection { inner: xpc_connection_t, _queue: XpcQueue }

// XPC connections and dispatch queues are thread-safe — they are designed
// to be used from any dispatch queue/thread.
unsafe impl Send for XpcConnection {}
unsafe impl Sync for XpcConnection {}

impl Clone for XpcConnection {
    fn clone(&self) -> Self {
        XpcConnection {
            inner: unsafe { xpc_retain(self.inner as xpc_object_t) as xpc_connection_t },
            _queue: XpcQueue { inner: unsafe { dispatch_retain(self._queue.inner) } },
        }
    }
}

struct XpcQueue { inner: dispatch_queue_t }
unsafe impl Send for XpcQueue {}
unsafe impl Sync for XpcQueue {}

impl Clone for XpcQueue {
    fn clone(&self) -> Self {
        XpcQueue { inner: unsafe { dispatch_retain(self.inner) } }
    }
}
impl Drop for XpcQueue { fn drop(&mut self) { unsafe { dispatch_release(self.inner) } } }

fn create_queue(label: &str) -> XpcQueue {
    unsafe { let c_label = CString::new(label).unwrap(); XpcQueue { inner: dispatch_queue_create(c_label.as_ptr(), ptr::null_mut()) } }
}

pub enum XpcListenerEvent { NewPeer(XpcConnection), Error(String) }
pub enum XpcMessageEvent { Message(XpcDictionary), Invalidated, Error(String) }

impl XpcConnection {
    pub fn connect<F>(service_name: &str, handler: F) -> Self
    where F: Fn(XpcMessageEvent) + Send + 'static {
        let c_name = CString::new(service_name).unwrap();
        let queue = create_queue(&format!("com.formal-web.xpc-client.{}", service_name));
        let context = Box::into_raw(Box::new(handler));
        unsafe extern "C" fn client_callback(dict: xpc_object_t, context: *mut c_void) {
            let handler = unsafe { &*(context as *mut Box<dyn Fn(XpcMessageEvent) + Send>) };
            let dict_obj = unsafe { XpcObject::from_raw(xpc_retain(dict)) };
            handler(XpcMessageEvent::Message(XpcDictionary { object: dict_obj }));
        }
        let inner = unsafe {
            fw_xpc_create_client(c_name.as_ptr(), queue.inner,
                Some(client_callback as XpcPeerMessageCallback), context as *mut c_void)
        };
        XpcConnection { inner, _queue: queue }
    }

    pub fn listen<F>(service_name: &str, handler: F) -> Self
    where F: Fn(XpcListenerEvent) + Send + 'static {
        let c_name = CString::new(service_name).unwrap();
        let queue = create_queue(&format!("com.formal-web.xpc-listener.{}", service_name));
        let context = Box::into_raw(Box::new(handler));
        unsafe extern "C" fn listener_callback(event: xpc_object_t, context: *mut c_void) {
            let handler = unsafe { &*(context as *mut Box<dyn Fn(XpcListenerEvent) + Send>) };
            let xpc_error_key = CString::new("XPCErrorDescription").unwrap();
            let error_str = unsafe { xpc_dictionary_get_string(event, xpc_error_key.as_ptr()) };
            if !error_str.is_null() {
                let msg = unsafe { CStr::from_ptr(error_str) }.to_string_lossy().into_owned();
                handler(XpcListenerEvent::Error(msg));
                return;
            }
            let peer_inner = unsafe { fw_xpc_peer_from_event(event) };
            let peer_queue = create_queue("com.formal-web.xpc-peer");
            handler(XpcListenerEvent::NewPeer(XpcConnection { inner: peer_inner, _queue: peer_queue }));
        }
        let inner = unsafe {
            fw_xpc_create_listener(c_name.as_ptr(), queue.inner,
                Some(listener_callback as XpcListenerEventCallback), context as *mut c_void)
        };
        XpcConnection { inner, _queue: queue }
    }

    pub fn set_message_handler<F>(&self, handler: F)
    where F: Fn(XpcMessageEvent) + Send + 'static {
        let queue = create_queue("com.formal-web.xpc-peer-msg");
        let context = Box::into_raw(Box::new(handler));
        unsafe extern "C" fn peer_callback(dict: xpc_object_t, context: *mut c_void) {
            let handler = unsafe { &*(context as *mut Box<dyn Fn(XpcMessageEvent) + Send>) };
            let dict_obj = unsafe { XpcObject::from_raw(xpc_retain(dict)) };
            handler(XpcMessageEvent::Message(XpcDictionary { object: dict_obj }));
        }
        unsafe {
            fw_xpc_set_peer_handler(self.inner, queue.inner,
                Some(peer_callback as XpcPeerMessageCallback), context as *mut c_void);
        }
    }

    pub fn resume(&self) { unsafe { fw_xpc_resume(self.inner) } }
    pub fn send_message(&self, message: &XpcDictionary) { unsafe { xpc_connection_send_message(self.inner, message.as_raw()); } }
    pub fn cancel(&self) { unsafe { fw_xpc_cancel(self.inner) } }
}
