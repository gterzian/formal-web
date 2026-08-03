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
//!
//! Platform objects can additionally get a **per-object owner**: a fresh
//! `NSObject` exported as a property of the object's reflector (`JSValue
//! setValue:forProperty:`), reachable from the JS runtime exactly while
//! the reflector is.  [`JscGcOwner`] is the handle type for both the
//! per-context anchor and per-object owners; `JscManagedValue::new`
//! takes one so a cell's edges can be pointed at either.

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

        /// Set a named property on the wrapped JS object to an exported
        /// Objective-C object.  This creates a `JSAPIWrapperObject` on the
        /// object (the property key is a plain string), making the object
        /// reachable from the JS runtime while the JS object is.  Used to
        /// export the per-object GC owner on a platform object's reflector.
        #[unsafe(method(setValue:forProperty:))]
        fn set_value(&self, value: &NSObject, property: &NSObject);
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

/// Name of the property holding the per-object GC owner on each platform
/// object's reflector (see `JscGcOwner::exported_on`).  Visible to JS like
/// the anchor; kept obscure to avoid colliding with real properties.
pub(crate) const PLATFORM_GC_OWNER_PROPERTY: &str = "formalWebGcOwner";

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

// ── GC owner handle ───────────────────────────────────────────────────────

/// An owner for managed-reference edges: an Objective-C object exported
/// to the JS runtime (a `JSAPIWrapperObject`), plus the virtual machine
/// the edges are registered with.
///
/// Two kinds exist:
///
/// - the per-context **anchor** ([`JscGcOwner::anchor`]), exported on the
///   global object, always reachable;
/// - a **per-object owner** ([`JscGcOwner::exported_on`]), exported as a
///   property of a platform object's reflector, reachable exactly while
///   the reflector is.
#[derive(Clone)]
pub(crate) struct JscGcOwner {
    /// The owner object; edges are scanned by the GC while this is
    /// reachable from the JS runtime.
    object: Retained<NSObject>,
    /// The virtual machine the edges are registered with.
    vm: Retained<JSVirtualMachine>,
    /// The context wrapper; kept alive so the owner's export stays valid.
    _context: Retained<JSContext>,
}

impl JscGcOwner {
    /// The per-context anchor: a plain `NSObject` exported to the global
    /// object, reachable for the whole context lifetime.
    pub(crate) fn anchor(ctx: *mut JSContextRef) -> Self {
        let (vm, anchor, js_context) = gc_anchor(ctx);
        Self {
            object: anchor,
            vm,
            _context: js_context,
        }
    }

    /// Create a fresh `NSObject`, export it as `property` on the given JS
    /// object (the reflector), and return it as an owner handle.
    pub(crate) fn exported_on(
        ctx: *mut JSContextRef,
        object_ref: *mut JSValueRef,
        property: &str,
    ) -> Self {
        objc2::rc::autoreleasepool(|_pool| {
            let js_context =
                JSContext::context_with_js_global_context_ref(ctx as *mut JSGlobalContextRef);
            let vm = js_context.virtual_machine();
            let owner = NSObject::new();
            let name = NSString::from_str(property);
            let js_value = JSValue::value_with_js_value_ref(object_ref, &js_context);
            js_value.set_value(&owner, &name);
            Self {
                object: owner,
                vm,
                _context: js_context,
            }
        })
    }
}

// ── RAII managed reference ────────────────────────────────────────────────

/// A JS value kept alive through JSC's managed-reference mechanism.
///
/// Created with [`JscManagedValue::new`]; the value is wrapped in a
/// `JSManagedValue` and registered with the context's `JSVirtualMachine`
/// via `addManagedReference:withOwner:`, using the passed [`JscGcOwner`]
/// (per-context anchor, or a per-object owner exported on a reflector).
/// When the last clone is dropped the edge is removed
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
    /// The owner the edge is registered with (and the VM, needed to tear
    /// the edge down on drop).
    owner: JscGcOwner,
    /// The C-API value pointer, for callers that need the raw JSValueRef.
    value_ref: *mut JSValueRef,
}

impl JscManagedValue {
    /// Protect `value` in `ctx` through the managed-reference mechanism,
    /// with the edge reported under `owner`.
    ///
    /// Returns `None` for primitive values (not GC-managed).
    pub(crate) fn new(
        ctx: *mut JSContextRef,
        value: *mut JSValueRef,
        owner: &JscGcOwner,
    ) -> Option<Self> {
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
            let js_context =
                JSContext::context_with_js_global_context_ref(ctx as *mut JSGlobalContextRef);
            let js_value = JSValue::value_with_js_value_ref(value, &js_context);
            let managed = JSManagedValue::managed_value_with_value(&js_value);
            owner.vm.add_managed_reference(&managed, &owner.object);
            Some(JscManagedValue {
                managed,
                owner: owner.clone(),
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
        // owner stays reachable, so without this the value would never
        // be collected.
        self.owner
            .vm
            .remove_managed_reference(&self.managed, &self.owner.object);
    }
}
