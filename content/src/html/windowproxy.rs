//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
//!
//! The WindowProxy is an exotic object that wraps a Window ordinary object,
//! indirecting most operations through to the wrapped object.
//! Each browsing context has an associated WindowProxy object. When the
//! browsing context is navigated, the Window object wrapped by the
//! browsing context's associated WindowProxy object is changed.
//!
//! The WindowProxy implements custom internal methods per the HTML spec.
//! For same-origin access most operations delegate to the inner Window
//! object.  Cross-origin operations use a restricted property set.

use boa_engine::{
    Context, JsData, JsResult, JsValue,
    js_string,
    object::{
        InternalMethodPropertyContext, InternalObjectMethods,
        JsObject, JsPrototype,
    },
    property::{PropertyDescriptor, PropertyKey},
};
use boa_gc::{Finalize, Trace};

// ── WindowProxy struct ──

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Each browsing context has an associated WindowProxy object.
/// The [[Window]] internal slot points to the current Window ordinary
/// object for that browsing context's active document.
#[derive(Trace, Finalize)]
pub struct WindowProxy {
    /// <https://html.spec.whatwg.org/#windowproxy-[[window]]>
    window: JsObject,
}

impl WindowProxy {
    /// Create a new WindowProxy wrapping the given Window JsObject.
    pub(crate) fn new(window: JsObject) -> Self {
        Self { window }
    }

    /// Returns the wrapped Window JsObject handle.
    /// Used to extract the inner Window when bindings receive a
    /// WindowProxy as `this`, and for navigation-time Window swapping.
    pub(crate) fn window_handle(&self) -> &JsObject {
        &self.window
    }
}

// ── Exotic internal methods ──

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// The WindowProxy's internal methods vtable overrides 10 of the 14
/// ordinary internal methods to implement the WindowProxy exotic
/// behavior per HTML §7.2.3.
impl JsData for WindowProxy {
    fn internal_methods(&self) -> &'static InternalObjectMethods {
        static METHODS: InternalObjectMethods = InternalObjectMethods::ordinary()
            .get_prototype_of(window_proxy_get_prototype_of)
            .set_prototype_of(window_proxy_set_prototype_of)
            .is_extensible(window_proxy_is_extensible)
            .prevent_extensions(window_proxy_prevent_extensions)
            .get_own_property(window_proxy_get_own_property)
            .define_own_property(window_proxy_define_own_property)
            .get(window_proxy_get)
            .set(window_proxy_set)
            .delete(window_proxy_delete)
            .own_property_keys(window_proxy_own_property_keys)
            // __has_property__, __try_get__: ordinary — works because
            //   ordinary_has_property calls [[GetOwnProperty]] which we
            //   override to delegate to the Window.
            // __call__, __construct__: ordinary — WindowProxy is neither
            //   callable nor constructable.
            .build();
        &METHODS
    }
}

/// Helper: extract the wrapped Window from the WindowProxy receiver.
fn unwrap_window(obj: &JsObject) -> JsObject {
    // The obj IS the WindowProxy JsObject.
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    obj.downcast_ref::<WindowProxy>()
        .map(|proxy| proxy.window.clone())
        .expect("WindowProxy exotic method called on non-WindowProxy object")
}

// ── [[GetPrototypeOf]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-getprototypeof>
fn window_proxy_get_prototype_of(
    obj: &JsObject,
    _context: &mut Context,
) -> JsResult<JsPrototype> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let window = unwrap_window(obj);

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true, then return !
    //          OrdinaryGetPrototypeOf(W)."
    // Note: Same-origin is always true (single origin per content process)
    // until multi-origin support is added.
    // Step 3: "Return null."
    Ok(window.prototype())
}

// ── [[SetPrototypeOf]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-setprototypeof>
fn window_proxy_set_prototype_of(
    obj: &JsObject,
    val: JsPrototype,
    _context: &mut Context,
) -> JsResult<bool> {
    // Step 1: "Return ! SetImmutablePrototype(this, V)."
    //
    // SetImmutablePrototype ( O, V )
    // https://tc39.es/ecma262/#sec-set-immutable-prototype
    // 1. Let current be ? O.[[GetPrototypeOf]]().
    //    Use prototype() which reads the stored [[Prototype]] field.
    //    For WindowProxy this was set to Window.prototype during
    //    construction, matching what [[GetPrototypeOf]] returns.
    let current = obj.prototype();

    // 2. If SameValue(V, current) is true, return true.
    // 3. Return false.
    Ok(val == current)
}

// ── [[IsExtensible]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-isextensible>
fn window_proxy_is_extensible(
    _obj: &JsObject,
    _context: &mut Context,
) -> JsResult<bool> {
    // Step 1: "Return true."
    Ok(true)
}

// ── [[PreventExtensions]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-preventextensions>
fn window_proxy_prevent_extensions(
    _obj: &JsObject,
    _context: &mut Context,
) -> JsResult<bool> {
    // Step 1: "Return false."
    Ok(false)
}

// ── [[GetOwnProperty]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-getownproperty>
fn window_proxy_get_own_property(
    obj: &JsObject,
    key: &PropertyKey,
    context: &mut InternalMethodPropertyContext<'_>,
) -> JsResult<Option<PropertyDescriptor>> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let window = unwrap_window(obj);

    // Step 2: "If P is an array index property name:"
    // <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>
    let is_array_index = key_to_u32(key).is_some();
    if is_array_index {
        // Step 2.1-2.x: child navigable lookup.
        // Note: Child navigable support is not yet implemented
        // (no iframe child tracking in the content process).
        // Return undefined for now (same-origin fallthrough).
        //
        // Step 2.5.1: "If IsPlatformObjectSameOrigin(W) is true, then
        //              return undefined."
        return Ok(None);
    }

    // Step 3: "If IsPlatformObjectSameOrigin(W) is true, then return !
    //          OrdinaryGetOwnProperty(W, P)."
    window.get_own_property_descriptor(key, context)
}

// ── [[DefineOwnProperty]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-defineownproperty>
fn window_proxy_define_own_property(
    obj: &JsObject,
    key: &PropertyKey,
    desc: PropertyDescriptor,
    context: &mut InternalMethodPropertyContext<'_>,
) -> JsResult<bool> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let window = unwrap_window(obj);

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true:"
    // Note: Same-origin is always true (single origin per content process).

    // Step 2.1: "If P is an array index property name, return false."
    if key_to_u32(key).is_some() {
        return Ok(false);
    }

    // Step 2.2: "Return ? OrdinaryDefineOwnProperty(W, P, Desc)."
    window.define_own_property(key, desc, context)
}

// ── [[Get]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-get>
fn window_proxy_get(
    obj: &JsObject,
    key: &PropertyKey,
    receiver: JsValue,
    context: &mut InternalMethodPropertyContext<'_>,
) -> JsResult<JsValue> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let _window = unwrap_window(obj);

    // Step 2: "Check if an access between two browsing contexts should be
    //          reported..."
    // Note: Same-origin check — access reporting is not yet implemented.

    // Step 3: "If IsPlatformObjectSameOrigin(W) is true, then return ?
    //          OrdinaryGet(this, P, Receiver)."
    //
    // NOTE: Cannot delegate through the vtable (obj.__get__) because that
    // dispatches back to window_proxy_get — infinite loop.  Instead we
    // implement OrdinaryGet manually: call [[GetOwnProperty]], walk the
    // prototype chain if not found, then return the value or call the
    // accessor getter.

    // OrdinaryGet ( O, P, Receiver )
    // https://tc39.es/ecma262/#sec-ordinaryget

    // 1. Assert: IsPropertyKey(P) is true.
    // 2. Let desc be ? O.[[GetOwnProperty]](P).
    match obj.get_own_property_descriptor(key, context)? {
        None => {
            // 2a. Let parent be ? O.[[GetPrototypeOf]]().
            if let Some(parent) = obj.prototype() {
                // 2c. Return ? parent.[[Get]](P, Receiver).
                parent.get_with_receiver(key, receiver, context)
            } else {
                // 2b. If parent is null, return undefined.
                Ok(JsValue::undefined())
            }
        }
        Some(ref desc) => {
            // 3. If IsDataDescriptor(desc) is true, return desc.[[Value]].
            if let Some(value) = desc.value() {
                return Ok(value.clone());
            }
            // 4. Assert: IsAccessorDescriptor(desc) is true.
            // 5. Let getter be desc.[[Get]].
            if let Some(getter) = desc.get().and_then(|v| v.as_object()) {
                // 7. Return ? Call(getter, Receiver).
                return getter.call(&receiver, &[], context);
            }
            // 6. If getter is undefined, return undefined.
            Ok(JsValue::undefined())
        }
    }
}

// ── [[Set]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-set>
fn window_proxy_set(
    obj: &JsObject,
    key: PropertyKey,
    value: JsValue,
    receiver: JsValue,
    context: &mut InternalMethodPropertyContext<'_>,
) -> JsResult<bool> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let window = unwrap_window(obj);

    // Step 2: "Check if an access between two browsing contexts should be
    //          reported..."
    // Note: Same-origin check — access reporting is not yet implemented.

    // Step 3: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 3.1: "If P is an array index property name, then return false."
    if key_to_u32(&key).is_some() {
        return Ok(false);
    }

    // Step 3.2: "Return ? OrdinarySet(W, P, V, Receiver)."
    //
    // NOTE: OrdinarySet delegates to the Window's [[Set]], which calls
    // Window.[[GetOwnProperty]](P) and either updates an existing own
    // property or walks the prototype chain.  When the property is found
    // on the prototype (Window.prototype), the setter is called with
    // `Receiver` as the `this` value.  When not found at all, the new
    // property is created on `Receiver` — but [[DefineOwnProperty]] on
    // the receiver (the WindowProxy) delegates to the Window, so the
    // new property ends up on the Window as desired.
    //
    // We use set_with_receiver to pass the original Receiver through.
    window.set_with_receiver(&key, value, receiver, context)
}

// ── [[Delete]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-delete>
fn window_proxy_delete(
    obj: &JsObject,
    key: &PropertyKey,
    context: &mut InternalMethodPropertyContext<'_>,
) -> JsResult<bool> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let window = unwrap_window(obj);

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true:"
    // Note: Same-origin is always true (single origin per content process).

    // Step 2.1: "If P is an array index property name:"
    if key_to_u32(key).is_some() {
        // Step 2.1.1: "Let desc be ! this.[[GetOwnProperty]](P)."
        let desc = obj.get_own_property_descriptor(key, context)?;
        // Step 2.1.2: "If desc is undefined, then return true."
        // Step 2.1.3: "Return false."
        return Ok(desc.is_none());
    }

    // Step 2.2: "Return ? OrdinaryDelete(W, P)."
    window.delete_property(key, context)
}

// ── [[OwnPropertyKeys]] ──
//
// <https://html.spec.whatwg.org/#windowproxy-ownpropertykeys>
fn window_proxy_own_property_keys(
    obj: &JsObject,
    context: &mut Context,
) -> JsResult<Vec<PropertyKey>> {
    // Step 1: "Let W be the value of the [[Window]] internal slot of this."
    let window = unwrap_window(obj);

    // Step 2: "Let maxProperties be W's associated Document's
    //          document-tree child navigables's size."
    // Note: Child navigable support is not yet implemented.
    // Step 3: "Let keys be the range 0 to maxProperties, exclusive."
    // → empty for now.
    let mut keys: Vec<PropertyKey> = Vec::new();

    // Step 4: "If IsPlatformObjectSameOrigin(W) is true, then return the
    //          concatenation of keys and OrdinaryOwnPropertyKeys(W)."
    let window_keys = window.own_property_keys(context)?;
    keys.extend(window_keys);

    Ok(keys)
}

// ── Helpers ──

/// Check whether a PropertyKey is an array index property name.
///
/// <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>
fn key_to_u32(key: &PropertyKey) -> Option<u32> {
    match key {
        PropertyKey::Index(index) => Some(index.get()),
        PropertyKey::String(string) => {
            // A string property name P is an array index if:
            // - P is not the empty string
            // - ToUint32(P) is not 2^32-1
            // - SameValue(ToString(ToUint32(P)), P) is true
            let s = string.to_std_string_escaped();
            if s.is_empty() {
                return None;
            }
            let parsed: u64 = s.parse().ok()?;
            if parsed >= u32::MAX as u64 {
                return None;
            }
            // Verify round-trip: must be the canonical decimal representation.
            if parsed.to_string() == s {
                Some(parsed as u32)
            } else {
                None
            }
        }
        PropertyKey::Symbol(_) => None,
    }
}

// ── Cross-origin helper operations ──
// These implement the abstract operations from HTML spec § 7.2.1.3.
// They are ready for cross-origin support; the same-origin fast path
// in the internal methods above bypasses them.

/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
#[allow(dead_code)]
struct CrossOriginPropertyEntry {
    property: &'static str,
    needs_get: bool,
    needs_set: bool,
}

/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
#[allow(dead_code)]
fn cross_origin_window_properties() -> Vec<CrossOriginPropertyEntry> {
    vec![
        CrossOriginPropertyEntry { property: "window", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "self", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "location", needs_get: true, needs_set: true },
        CrossOriginPropertyEntry { property: "close", needs_get: false, needs_set: false },
        CrossOriginPropertyEntry { property: "closed", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "focus", needs_get: false, needs_set: false },
        CrossOriginPropertyEntry { property: "blur", needs_get: false, needs_set: false },
        CrossOriginPropertyEntry { property: "frames", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "length", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "top", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "opener", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "parent", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "postMessage", needs_get: false, needs_set: false },
    ]
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn cross_origin_own_property_keys() -> Vec<PropertyKey> {
    let mut keys: Vec<PropertyKey> = cross_origin_window_properties()
        .into_iter()
        .map(|p| PropertyKey::String(js_string!(p.property)))
        .collect();

    keys.push(PropertyKey::String(js_string!("then")));
    keys.push(PropertyKey::Symbol(boa_engine::JsSymbol::to_string_tag()));
    keys.push(PropertyKey::Symbol(boa_engine::JsSymbol::has_instance()));
    keys.push(PropertyKey::Symbol(
        boa_engine::JsSymbol::is_concat_spreadable(),
    ));

    keys
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_window_properties()
        .iter()
        .any(|p| p.property == name)
}

/// <https://html.spec.whatwg.org/#isplatformobjectsameorigin-(-o-)>
///
/// Currently hardcoded to `true` because the content process only runs a
/// single origin.  When multi-origin support is added, this must check
/// whether the active document of the given Window's browsing context is
/// same-origin with the active document of the entry settings object's
/// browsing context.
///
/// Hardcoding to `true` means that if cross-origin windows are ever
/// returned from `window_open_steps`, all their properties will be
/// silently leaked through the WindowProxy.
#[allow(dead_code)]
pub(crate) fn is_platform_object_same_origin(_window: &JsObject, _context: &JsObject) -> bool {
    true
}
