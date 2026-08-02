//! ObjC (JavaScriptCore ObjC API) managed-reference bindings for GC
//! integration.
//!
//! The C API's `JSValueProtect`/`JSValueUnprotect` keep a JS value alive
//! with an opaque reference count that JSC's garbage collector cannot
//! reason about.  The ObjC API provides the proper integration, as
//! exercised by WebKit's own `testObjectiveCAPI.mm`:
//!
//! - [`JSManagedValue`] wraps a `JSValue` and retains it conditionally:
//!   the wrapped value stays alive as long as the managed value is
//!   reachable through the JS object graph or through a managed reference
//!   reported via `addManagedReference:withOwner:`.
//! - [`JSVirtualMachine::add_managed_reference`] /
//!   `remove_managed_reference` report an external (Rust) object-graph
//!   edge to the garbage collector.  The edge is scanned while the
//!   *owner* is reachable from within the JavaScript runtime — i.e. while
//!   the owner is an Objective-C object exported to JS (a
//!   `JSAPIWrapperObject`); `removeManagedReference` tears the edge down
//!   so the GC can collect the value.
//!
//! Each context gets one **anchor** object (a plain `NSObject` exported
//! to the JS global object).  All managed values for that context report
//! the anchor as their owner, so their values stay alive while the
//! anchor — and therefore the context — lives, and become collectable as
//! soon as `removeManagedReference` is called.
//!
//! The anchor is attached to the context's `JSContext` wrapper via an
//! Objective-C associated object.  `contextWithJSGlobalContextRef:` is
//! guaranteed to return the *same* wrapper for a given context (the VM
//! caches wrappers), so the anchor can be found from a bare
//! `JSContextRef` without any global registry.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{extern_class, extern_methods};
use objc2_foundation::NSString;

use crate::jsc_sys::{
    JSContextRef, JSGlobalContextRef, JSType, JSValueGetType, JSValueRef,
    OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject,
};

// ── ObjC class declarations (JavaScriptCore framework) ────────────────────

extern_class!(
    /// Objective-C wrapper around a `JSGlobalContextRef`.
    #[unsafe(super(NSObject))]
    pub struct JSContext;
);

extern_class!(
    /// A JavaScript virtual machine (object space / execution resources).
    #[unsafe(super(NSObject))]
    pub struct JSVirtualMachine;
);

extern_class!(
    /// Objective-C wrapper around a `JSValueRef`.
    #[unsafe(super(NSObject))]
    pub struct JSValue;
);

extern_class!(
    /// Conditionally-retained wrapper around a `JSValue`.
    #[unsafe(super(NSObject))]
    pub struct JSManagedValue;
);

impl JSContext {
    extern_methods!(
        /// Bridge: wrap an existing C-API context in its ObjC counterpart.
        /// The VM caches wrappers, so repeated calls return the same object.
        #[unsafe(method(contextWithJSGlobalContextRef:))]
        #[unsafe(method_family = none)]
        fn context_with_js_global_context_ref(ctx: *mut JSGlobalContextRef) -> Retained<JSContext>;

        /// The virtual machine this context belongs to.
        #[unsafe(method(virtualMachine))]
        #[unsafe(method_family = none)]
        fn virtual_machine(&self) -> Retained<JSVirtualMachine>;

        /// Export an Objective-C object to the global object (creates a
        /// `JSAPIWrapperObject`, making the object reachable from the JS
        /// runtime while the property lives).
        #[unsafe(method(setObject:forKeyedSubscript:))]
        fn set_object(&self, object: &NSObject, key: &NSObject);
    );
}

impl JSVirtualMachine {
    extern_methods!(
        /// Report an external object-graph edge: `object` is kept alive
        /// while `owner` is alive and reachable from the JS runtime.
        #[unsafe(method(addManagedReference:withOwner:))]
        fn add_managed_reference(&self, object: &NSObject, owner: &NSObject);

        /// Remove a previously reported edge; the GC may then collect the
        /// object.
        #[unsafe(method(removeManagedReference:withOwner:))]
        fn remove_managed_reference(&self, object: &NSObject, owner: &NSObject);
    );
}

impl JSValue {
    extern_methods!(
        /// Bridge: wrap an existing C-API value in its ObjC counterpart.
        #[unsafe(method(valueWithJSValueRef:inContext:))]
        #[unsafe(method_family = none)]
        fn value_with_js_value_ref(
            value: *mut JSValueRef,
            context: &JSContext,
        ) -> Retained<JSValue>;
    );
}

impl JSManagedValue {
    extern_methods!(
        /// Wrap a JS value in a conditionally-retained managed value.
        #[unsafe(method(managedValueWithValue:))]
        #[unsafe(method_family = none)]
        fn managed_value_with_value(value: &JSValue) -> Retained<JSManagedValue>;
    );
}

// ── Per-context anchor ────────────────────────────────────────────────────

/// Associated-object key for the anchor stored on the `JSContext` wrapper.
static ANCHOR_ASSOC_KEY: u8 = 0;

/// Name of the exported global property holding the anchor.
const ANCHOR_GLOBAL_NAME: &str = "formalWebGcAnchor";

/// Look up (creating if needed) the per-context GC anchor: a plain
/// `NSObject` exported to the context's global object so the garbage
/// collector treats it as reachable from the JS runtime.
///
/// Returns the virtual machine and the anchor (as the owner for managed
/// references).
fn gc_anchor(
    ctx: *mut JSContextRef,
) -> (
    Retained<JSVirtualMachine>,
    Retained<NSObject>,
    Retained<JSContext>,
) {
    let js_context = JSContext::context_with_js_global_context_ref(ctx as *mut JSGlobalContextRef);
    let vm = js_context.virtual_machine();

    let key_ptr = &ANCHOR_ASSOC_KEY as *const u8 as *const c_void;
    let js_context_ptr = &*js_context as *const JSContext as *const c_void;
    let existing = unsafe { objc_getAssociatedObject(js_context_ptr as *mut c_void, key_ptr) };
    if !existing.is_null() {
        // SAFETY: The association policy is RETAIN_NONATOMIC, so the
        // object is alive as long as the JSContext wrapper is; retain it
        // to share ownership.
        let anchor = unsafe { Retained::retain(existing as *mut NSObject) }
            .expect("associated anchor must be a valid NSObject");
        return (vm, anchor, js_context);
    }

    let anchor = NSObject::new();
    let key = NSString::from_str(ANCHOR_GLOBAL_NAME);
    // Export the anchor to the global object, making it reachable from
    // the JS runtime for as long as the context lives.
    js_context.set_object(&anchor, &key);

    unsafe {
        objc_setAssociatedObject(
            js_context_ptr as *mut c_void,
            key_ptr,
            &*anchor as *const NSObject as *mut c_void,
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }

    (vm, anchor, js_context)
}

// ── RAII managed reference ────────────────────────────────────────────────

/// A JS value kept alive through JSC's managed-reference mechanism.
///
/// Created with [`JscManagedValue::new`]; the value is wrapped in a
/// `JSManagedValue` and registered with the context's `JSVirtualMachine`
/// via `addManagedReference:withOwner:`, using the per-context exported
/// anchor as the owner (reachable from the JS runtime for the whole
/// context lifetime).  When the last clone is dropped the edge is removed
/// (`removeManagedReference:withOwner:`) and the GC may collect the value.
///
/// Only GC-managed heap values (objects, symbols, bigints) are wrapped;
/// primitives (undefined, null, boolean, number, string) are stack
/// allocated and need no protection — `new` returns `None` for them.
#[derive(Clone)]
pub(crate) struct JscManagedValue {
    /// The conditionally-retained value; the external object graph keeps
    /// this (and therefore the wrapped JS value) alive.
    managed: Retained<JSManagedValue>,
    /// The virtual machine, needed to tear the edge down on drop.
    vm: Retained<JSVirtualMachine>,
    /// Anchor owner — the per-context exported `NSObject`.  Reachable
    /// from the JS runtime while the context lives, so the edge is always
    /// scanned while this value exists.
    owner: Retained<NSObject>,
    /// The context wrapper; kept alive so the anchor's export (and the
    /// edge) stays valid for this value's lifetime.
    _context: Retained<JSContext>,
    /// The C-API value pointer, for callers that need the raw JSValueRef.
    value_ref: *mut JSValueRef,
}

impl JscManagedValue {
    /// Protect `value` in `ctx` through the managed-reference mechanism.
    ///
    /// Returns `None` for primitive values (not GC-managed).
    pub(crate) fn new(ctx: *mut JSContextRef, value: *mut JSValueRef) -> Option<Self> {
        if ctx.is_null() || value.is_null() {
            return None;
        }
        // Only heap-allocated values need protection.
        let js_type = unsafe { JSValueGetType(ctx, value) };
        if !matches!(
            js_type,
            JSType::kJSTypeObject | JSType::kJSTypeSymbol | JSType::kJSTypeBigInt
        ) {
            return None;
        }
        // JSC's ObjC API autoreleases the wrapper objects it creates.
        // Rust has no autorelease pool, so without an explicit one the
        // JSValue/JSManagedValue wrappers would leak and keep the wrapped
        // value alive forever.  Drain the pool right after creation: the
        // JSManagedValue retains its JSValue strongly (JSManagedValue.h
        // declares `value` as `strong`), and every object we keep is
        // explicitly retained (Retained), so nothing we need is released.
        objc2::rc::autoreleasepool(|_pool| {
            let (vm, anchor, js_context) = gc_anchor(ctx);
            let js_value = JSValue::value_with_js_value_ref(value, &js_context);
            let managed = JSManagedValue::managed_value_with_value(&js_value);
            vm.add_managed_reference(&managed, &anchor);
            Some(JscManagedValue {
                managed,
                vm,
                owner: anchor,
                _context: js_context,
                value_ref: value,
            })
        })
    }

    /// The raw C-API value reference kept alive by this managed value.
    pub(crate) fn value_ref(&self) -> *mut JSValueRef {
        self.value_ref
    }
}

impl Drop for JscManagedValue {
    fn drop(&mut self) {
        // The runtime scans the edge until it is explicitly removed; the
        // anchor owner stays reachable, so without this the value would
        // never be collected.
        self.vm.remove_managed_reference(&self.managed, &self.owner);
    }
}
