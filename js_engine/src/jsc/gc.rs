//! JavaScriptCore garbage-collection integration built on `JSManagedValue`.
//!
//! Platform objects are represented to JavaScriptCore by an exported ObjC
//! holder (see `src/jsc_gc_wrapper.m`).  The holder owns the Rust platform
//! data and is the owner for every managed value the platform object holds,
//! so a managed reference lives exactly as long as the object (or the realm)
//! that owns it.  This module wraps the shim for Rust:
//!
//! - [`JscGcContext`] is the cached ObjC `JSContext` for an engine.
//! - [`JscGcOwner`] is a realm-lifetime owner used for the handful of roots
//!   the realm itself owns (Window, Document).
//! - [`JscManagedValue`] wraps one JS value.  With an owner it is retained
//!   while the owner is reachable from JS (the public-API replacement for the
//!   C API protect set); without one it is a weak reference that reports
//!   `None` once the value is collected.
//!
//! Both types release their ObjC reference on drop, so no separate protect
//! set has to be balanced by hand.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_void;
use std::rc::Rc;

use crate::jsc_sys::{JSContextRef, JSGlobalContextRef, JSObjectRef, JSValueRef};

use super::types::{JscObject, JscValue};

/// Opaque `FwJscContext` handle from the Objective-C shim.
#[repr(C)]
pub struct FwJscContextOpaque {
    _private: [u8; 0],
}

/// Opaque `FwJscGcOwner` handle from the Objective-C shim.
#[repr(C)]
pub struct FwJscGcOwnerOpaque {
    _private: [u8; 0],
}

/// Opaque `FwJscPlatformObject` handle from the Objective-C shim.
#[repr(C)]
pub struct FwJscPlatformObjectOpaque {
    _private: [u8; 0],
}

/// Opaque `FwJscManagedValue` handle from the Objective-C shim.
#[repr(C)]
pub struct FwJscManagedValueOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn fw_jsc_context_create(global: *mut JSGlobalContextRef) -> *mut FwJscContextOpaque;
    fn fw_jsc_context_release(context: *mut FwJscContextOpaque);
    fn fw_jsc_gc_owner_create(context: *mut FwJscContextOpaque) -> *mut FwJscGcOwnerOpaque;
    fn fw_jsc_gc_owner_release(owner: *mut FwJscGcOwnerOpaque);
    fn fw_jsc_platform_object_create(
        context: *mut FwJscContextOpaque,
        data: *mut c_void,
    ) -> *mut FwJscPlatformObjectOpaque;
    fn fw_jsc_platform_object_js_object(
        context: *mut FwJscContextOpaque,
        holder: *mut FwJscPlatformObjectOpaque,
    ) -> *mut JSObjectRef;
    fn fw_jsc_platform_object_release(holder: *mut FwJscPlatformObjectOpaque);
    fn fw_jsc_managed_value_create(
        context: *mut FwJscContextOpaque,
        value: *mut JSValueRef,
        owner: *mut c_void,
    ) -> *mut FwJscManagedValueOpaque;
    fn fw_jsc_managed_value_retain(
        managed: *mut FwJscManagedValueOpaque,
    ) -> *mut FwJscManagedValueOpaque;
    fn fw_jsc_managed_value_release(managed: *mut FwJscManagedValueOpaque);
    fn fw_jsc_managed_value_get(managed: *mut FwJscManagedValueOpaque) -> *mut JSValueRef;
}

/// One exported platform object: the Rust data it owns and the ObjC holder
/// that is its managed-reference owner.
struct PlatformEntry {
    data: *mut Box<dyn std::any::Any>,
    holder: *mut FwJscPlatformObjectOpaque,
}

thread_local! {
    /// Lookup table from JS object pointer to platform entry.  The holder owns
    /// the data and the holder's `dealloc` removes the entry, so this only
    /// mirrors lifetime; it exists so `with_object_any` never has to round-trip
    /// through the ObjC bridge (which would enumerate the object's properties).
    static PLATFORM_OBJECTS: RefCell<HashMap<usize, PlatformEntry>> =
        RefCell::new(HashMap::new());
}

/// Called by an exported platform-object holder when its JS object is
/// collected; drops the Rust data the holder owns and the lookup entry.
///
/// # Safety
///
/// Called by the Objective-C shim only, with pointers produced by
/// `create_platform_object` and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fw_jsc_platform_object_drop_data(
    data: *mut c_void,
    js_object: *mut c_void,
) {
    PLATFORM_OBJECTS.with(|objects| {
        objects.borrow_mut().remove(&(js_object as usize));
    });
    if data.is_null() {
        return;
    }
    // SAFETY: The pointer is a `Box<Box<dyn Any>>` leaked in
    // `create_platform_object`; this is its only owner.
    unsafe {
        drop(Box::from_raw(data as *mut Box<dyn std::any::Any>));
    }
}

/// The cached ObjC `JSContext` wrapper for an engine's global context.
#[derive(Clone)]
pub struct JscGcContext(Rc<JscGcContextInner>);

struct JscGcContextInner {
    raw: *mut FwJscContextOpaque,
    global: *mut JSGlobalContextRef,
}

impl Drop for JscGcContextInner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `raw` was returned by `fw_jsc_context_create` and is
            // released exactly once, here.
            unsafe { fw_jsc_context_release(self.raw) };
        }
    }
}

impl JscGcContext {
    /// Create the cached context wrapper for `global`.
    ///
    /// # Safety
    ///
    /// `global` must be a live `JSGlobalContextRef` that outlives the context
    /// wrapper and every object created through it.
    pub unsafe fn new(global: *mut JSGlobalContextRef) -> Option<Self> {
        if global.is_null() {
            return None;
        }
        // SAFETY: The caller guarantees `global` is live.
        let raw = unsafe { fw_jsc_context_create(global) };
        if raw.is_null() {
            return None;
        }
        Some(Self(Rc::new(JscGcContextInner { raw, global })))
    }

    fn as_raw(&self) -> *mut FwJscContextOpaque {
        self.0.raw
    }

    /// The `JSGlobalContextRef` this context wraps.
    pub fn global(&self) -> *mut JSGlobalContextRef {
        self.0.global
    }

    /// Export a new platform object owning `data`.
    ///
    /// The returned object is the holder's bridge-created JS object; the
    /// holder frees `data` when that JS object is collected.  Returns `None`
    /// when the object cannot be created.
    pub fn create_platform_object(
        &self,
        data: Box<dyn std::any::Any + 'static>,
    ) -> Option<JscObject> {
        // Keep the `Box<dyn Any>` behind one more `Box` so the shim can hold a
        // thin pointer; `fw_jsc_platform_object_drop_data` reconstructs it.
        let data_pointer = Box::into_raw(Box::new(data));
        // SAFETY: `self` is a live context; the shim takes ownership of
        // `data_pointer` on success.
        let holder =
            unsafe { fw_jsc_platform_object_create(self.as_raw(), data_pointer as *mut c_void) };
        if holder.is_null() {
            // Reclaim the data if the shim could not take it.
            // SAFETY: The pointer has not been handed to the shim.
            unsafe { drop(Box::from_raw(data_pointer)) };
            return None;
        }
        // SAFETY: `holder` is a live handle.
        let raw = unsafe { fw_jsc_platform_object_js_object(self.as_raw(), holder) };
        if raw.is_null() {
            // SAFETY: `holder` came from `fw_jsc_platform_object_create`.
            unsafe { fw_jsc_platform_object_release(holder) };
            return None;
        }
        PLATFORM_OBJECTS.with(|objects| {
            objects.borrow_mut().insert(
                raw as usize,
                PlatformEntry {
                    data: data_pointer,
                    holder,
                },
            );
        });
        // The JS wrapper owns the holder from here on; drop the +1 handle.
        // SAFETY: `holder` came from `fw_jsc_platform_object_create`.
        unsafe { fw_jsc_platform_object_release(holder) };
        Some(JscObject {
            raw,
            ctx: self.global() as *mut JSContextRef,
        })
    }

    /// The Rust platform data owned by the holder of `object`, if `object` is
    /// an exported platform object.
    pub fn platform_object_data(&self, object: &JscObject) -> Option<&dyn std::any::Any> {
        let data = PLATFORM_OBJECTS.with(|objects| {
            objects
                .borrow()
                .get(&(object.raw as usize))
                .map(|entry| entry.data)
        });
        let data = data?;
        // SAFETY: The holder owns the data and keeps it alive while the JS
        // object (and therefore this borrow) is alive.
        Some(unsafe { &**data })
    }

    /// A raw pointer to the Rust platform data owned by the holder of
    /// `object`, for callers that need exclusive access.
    pub fn platform_object_data_raw(
        &self,
        object: &JscObject,
    ) -> Option<*mut dyn std::any::Any> {
        let data = PLATFORM_OBJECTS.with(|objects| {
            objects
                .borrow()
                .get(&(object.raw as usize))
                .map(|entry| entry.data)
        });
        let data = data?;
        // SAFETY: The holder owns the data and keeps it alive while the JS
        // object is alive.
        Some(unsafe { &mut **data } as *mut dyn std::any::Any)
    }

    /// The holder of the exported platform object `object`, as a raw owner
    /// pointer, or null.
    fn platform_object_owner(&self, object: &JscObject) -> *mut c_void {
        PLATFORM_OBJECTS.with(|objects| {
            objects
                .borrow()
                .get(&(object.raw as usize))
                .map(|entry| entry.holder as *mut c_void)
                .unwrap_or(std::ptr::null_mut())
        })
    }
}

/// A realm-lifetime owner for managed values.
///
/// The owner is bridged into the JS runtime so JavaScriptCore tracks the
/// managed references registered against it.  Used for the handful of
/// references the realm itself roots (Window, Document); platform objects use
/// their own holders as owners.
pub struct JscGcOwner {
    raw: *mut FwJscGcOwnerOpaque,
    context: JscGcContext,
}

impl Drop for JscGcOwner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `raw` was returned by `fw_jsc_gc_owner_create` and is
            // released exactly once, here.
            unsafe { fw_jsc_gc_owner_release(self.raw) };
        }
    }
}

impl JscGcOwner {
    /// Create a realm owner.
    pub fn new(context: JscGcContext) -> Option<Self> {
        // SAFETY: The context is live for the owner's lifetime.
        let raw = unsafe { fw_jsc_gc_owner_create(context.as_raw()) };
        if raw.is_null() {
            return None;
        }
        Some(Self { raw, context })
    }

    fn as_raw(&self) -> *mut FwJscGcOwnerOpaque {
        self.raw
    }

    /// The context this owner belongs to.
    pub fn context(&self) -> &JscGcContext {
        &self.context
    }
}

/// A JS value held outside the JS heap.
///
/// With an owner the value is retained while the owner is reachable from JS;
/// without one it is a weak reference.  Cloning retains the ObjC managed
/// value; the last clone releases it.
pub struct JscManagedValue {
    raw: *mut FwJscManagedValueOpaque,
    ctx: *mut JSContextRef,
}

impl Clone for JscManagedValue {
    fn clone(&self) -> Self {
        if self.raw.is_null() {
            return Self::empty();
        }
        // SAFETY: `raw` is a live retained handle owned by `self`.
        let raw = unsafe { fw_jsc_managed_value_retain(self.raw) };
        Self {
            raw,
            ctx: self.ctx,
        }
    }
}

impl JscManagedValue {
    /// Wrap `value`, retained while `owner` is reachable from JS.
    pub fn new_owned(context: &JscGcContext, value: &JscValue, owner: Owner<'_>) -> Option<Self> {
        let owner = match owner {
            Owner::Realm(owner) => owner.as_raw() as *mut c_void,
            Owner::PlatformObject(object) => context.platform_object_owner(&object),
        };
        // SAFETY: The context and value are live; the shim returns a retained
        // handle or NULL.
        let raw = unsafe { fw_jsc_managed_value_create(context.as_raw(), value.raw, owner) };
        if raw.is_null() {
            return None;
        }
        Some(Self {
            raw,
            ctx: context.global() as *mut JSContextRef,
        })
    }

    /// A weak reference to `value` (no owner).
    pub fn new_weak(context: &JscGcContext, value: &JscValue) -> Option<Self> {
        // SAFETY: The context and value are live.
        let raw =
            unsafe { fw_jsc_managed_value_create(context.as_raw(), value.raw, std::ptr::null_mut()) };
        if raw.is_null() {
            return None;
        }
        Some(Self {
            raw,
            ctx: context.global() as *mut JSContextRef,
        })
    }

    /// An empty managed value that refers to nothing.  `get` always returns
    /// `None`.  Used when a weak reference cannot be created.
    pub fn empty() -> Self {
        Self {
            raw: std::ptr::null_mut(),
            ctx: std::ptr::null_mut(),
        }
    }

    /// The current value, or `None` if it has been collected.
    ///
    /// For a weak managed value the returned value is only guaranteed alive
    /// until the next JavaScript execution.
    pub fn get(&self) -> Option<JscValue> {
        if self.raw.is_null() {
            return None;
        }
        // SAFETY: `raw` is a live handle owned by this value.
        let raw_value = unsafe { fw_jsc_managed_value_get(self.raw) };
        if raw_value.is_null() {
            None
        } else {
            Some(JscValue {
                raw: raw_value,
                ctx: self.ctx,
            })
        }
    }

    /// The current value as an object, or `None` if collected or not an
    /// object.
    pub fn get_object(&self) -> Option<JscObject> {
        self.get().and_then(|value| value.as_object())
    }

    /// Whether the value is still alive.
    pub fn is_alive(&self) -> bool {
        self.get().is_some()
    }
}

impl Drop for JscManagedValue {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `raw` was returned by `fw_jsc_managed_value_create` or
            // `fw_jsc_managed_value_retain` and each retained handle is
            // released exactly once.
            unsafe { fw_jsc_managed_value_release(self.raw) };
        }
    }
}

/// The owner a managed value is registered against.
pub enum Owner<'a> {
    /// A realm-lifetime owner.
    Realm(&'a JscGcOwner),
    /// An exported platform object (its holder owns the reference).
    PlatformObject(JscObject),
}

// `JscManagedValue` is neither `Send` nor `Sync`: the content process is
// single-threaded and JavaScriptCore's ObjC API is thread-affine.  The raw
// pointer fields make that the default.
