//! JavaScriptCore garbage-collection integration built on `JSManagedValue`.
//!
//! The JSC backend holds JS values in Rust structures (`GcCell` contents,
//! platform-object fields, reflectors).  A bare `JSValueRef` in Rust is not a
//! GC root, so JSC cannot see it.  This module wraps such values in an ObjC
//! `JSManagedValue` (through the `jsc_gc_wrapper` C shim), which is the public
//! API for holding a JS value from outside the JS heap:
//!
//! - [`JscGcOwner`] is a per-realm owner object bridged into the JS runtime.
//!   Managed values registered against it are retained while the owner lives.
//! - [`JscManagedValue`] wraps one JS value.  Created with an owner it is a
//!   strong, GC-visible root (the public-API replacement for the C API
//!   protect set); created without one it is a weak reference that reports
//!   `None` once the value is collected.
//!
//! Both types release their ObjC reference on drop, so no separate protect
//! set has to be balanced by hand.

use std::rc::Rc;

use crate::jsc_sys::{JSContextRef, JSGlobalContextRef, JSValueRef};

use super::types::{JscObject, JscValue};

/// Opaque `FwJscGcOwner` handle from the Objective-C shim.
#[repr(C)]
pub struct FwJscGcOwnerOpaque {
    _private: [u8; 0],
}

/// Opaque `FwJscManagedValue` handle from the Objective-C shim.
#[repr(C)]
pub struct FwJscManagedValueOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn fw_jsc_gc_owner_create(global: *mut JSGlobalContextRef) -> *mut FwJscGcOwnerOpaque;
    fn fw_jsc_gc_owner_release(owner: *mut FwJscGcOwnerOpaque);
    fn fw_jsc_managed_value_create(
        global: *mut JSGlobalContextRef,
        value: *mut JSValueRef,
        owner: *mut FwJscGcOwnerOpaque,
    ) -> *mut FwJscManagedValueOpaque;
    fn fw_jsc_managed_value_retain(
        managed: *mut FwJscManagedValueOpaque,
    ) -> *mut FwJscManagedValueOpaque;
    fn fw_jsc_managed_value_release(managed: *mut FwJscManagedValueOpaque);
    fn fw_jsc_managed_value_get(managed: *mut FwJscManagedValueOpaque) -> *mut JSValueRef;
}

/// Per-realm owner object for conditionally-retained managed values.
///
/// The owner is bridged into the JS runtime so JavaScriptCore tracks the
/// managed references registered against it.  Cloning shares the same owner;
/// the underlying ObjC object is released when the last clone is dropped.
#[derive(Clone)]
pub struct JscGcOwner(Rc<JscGcOwnerInner>);

struct JscGcOwnerInner {
    raw: *mut FwJscGcOwnerOpaque,
}

impl Drop for JscGcOwnerInner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `raw` was returned by `fw_jsc_gc_owner_create` and is
            // released exactly once, here.
            unsafe { fw_jsc_gc_owner_release(self.raw) };
        }
    }
}

impl JscGcOwner {
    /// Create an owner for `global`.  Failure is fatal to the caller: an
    /// engine cannot root values without an owner.
    ///
    /// # Safety
    ///
    /// `global` must be a live `JSGlobalContextRef` that outlives the owner
    /// and every managed value registered against it.
    pub unsafe fn new(global: *mut JSGlobalContextRef) -> Option<Self> {
        if global.is_null() {
            return None;
        }
        // SAFETY: The caller guarantees `global` is a live context; the shim
        // returns a retained handle or NULL.
        let raw = unsafe { fw_jsc_gc_owner_create(global) };
        if raw.is_null() {
            return None;
        }
        Some(Self(Rc::new(JscGcOwnerInner { raw })))
    }

    fn as_raw(&self) -> *mut FwJscGcOwnerOpaque {
        self.0.raw
    }
}

/// A JS value held outside the JS heap.
///
/// With an owner the value is retained while the managed value lives (a GC
/// root); without one it is a weak reference.  Cloning retains the ObjC
/// managed value; the last clone releases it.
pub struct JscManagedValue {
    raw: *mut FwJscManagedValueOpaque,
    ctx: *mut JSContextRef,
    owner: Option<JscGcOwner>,
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
            owner: self.owner.clone(),
        }
    }
}

impl JscManagedValue {
    /// Wrap `value`.  `owner` controls the retention semantics: `Some` makes
    /// the value a strong root while the owner lives, `None` makes it weak.
    ///
    /// # Safety
    ///
    /// `global` must be a live `JSGlobalContextRef` and `value` must be a
    /// value belonging to it.  Both must outlive the returned managed value.
    pub unsafe fn new(
        global: *mut JSGlobalContextRef,
        value: &JscValue,
        owner: Option<&JscGcOwner>,
    ) -> Option<Self> {
        if global.is_null() {
            return None;
        }
        let owner_ptr = owner
            .map(JscGcOwner::as_raw)
            .unwrap_or(std::ptr::null_mut());
        // SAFETY: The caller guarantees `global` and `value` are live; the
        // shim returns a retained handle or NULL.
        let raw = unsafe { fw_jsc_managed_value_create(global, value.raw, owner_ptr) };
        if raw.is_null() {
            return None;
        }
        Some(Self {
            raw,
            ctx: global as *mut JSContextRef,
            owner: owner.cloned(),
        })
    }

    /// A weak reference to `value` (no owner).
    ///
    /// # Safety
    ///
    /// `global` must be a live `JSGlobalContextRef` and `value` must be a
    /// value belonging to it.  Both must outlive the returned managed value.
    pub unsafe fn new_weak(global: *mut JSGlobalContextRef, value: &JscValue) -> Option<Self> {
        // SAFETY: Forwarded to the caller's guarantee.
        unsafe { Self::new(global, value, None) }
    }

    /// An empty managed value that refers to nothing.  `get` always returns
    /// `None`.  Used when a weak reference cannot be created.
    pub fn empty() -> Self {
        Self {
            raw: std::ptr::null_mut(),
            ctx: std::ptr::null_mut(),
            owner: None,
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

// `JscManagedValue` is neither `Send` nor `Sync`: the content process is
// single-threaded and JavaScriptCore's ObjC API is thread-affine.  The raw
// pointer fields make that the default.
