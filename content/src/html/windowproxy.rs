//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
//!
//! The WindowProxy is an exotic object that wraps a Window ordinary object
//! and is implemented as a JavaScript Proxy following the Web IDL observable
//! array pattern (§3.10).  The Proxy target is the inner Window and the
//! handler is an ordinary object whose trap functions implement the
//! WindowProxy semantics per HTML §7.2.3.
//!
//! Each trap matches a WindowProxy internal method.  Traps that differ from
//! the default Proxy behavior (which delegates to the target) are explicitly
//! implemented.  Traps whose default behavior matches the WindowProxy spec
//! for the same-origin case are omitted — the Proxy spec's default
//! [[GetOwnProperty]] / [[Get]] / [[Set]] / [[Delete]] / [[DefineOwnProperty]]
//! / [[HasProperty]] / [[OwnPropertyKeys]] all delegate to the target's
//! ordinary operations, which IS the WindowProxy same-origin algorithm.

use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    builtins::proxy::Proxy,
    js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    property::{PropertyDescriptor, PropertyKey},
};

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
pub(crate) fn create_window_proxy(
    window: &JsObject,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Create handler with null prototype (Web IDL observable array pattern).
    let handler = JsObject::with_null_proto();

    // ── [[SetPrototypeOf]] trap ──
    // <https://html.spec.whatwg.org/#windowproxy-setprototypeof>
    // Step 1: "Return ! SetImmutablePrototype(this, V)."
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, args, win: &JsObject, _context| {
                let current = win.prototype();
                let undefined_val = JsValue::undefined();
                let val = args.get(1).unwrap_or(&undefined_val);

                let same = match (&current, val) {
                    (Some(current_proto), _) => val
                        .as_object()
                        .map_or(false, |v| JsObject::equals(current_proto, &v)),
                    (None, _) => val.is_null(),
                };
                Ok(JsValue::new(same))
            },
            window.clone(),
        );
        let fn_obj = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("setPrototypeOf"))
            .length(2)
            .build();
        handler.create_data_property_or_throw(js_string!("setPrototypeOf"), fn_obj, context)?;
    }

    // ── [[PreventExtensions]] trap ──
    // <https://html.spec.whatwg.org/#windowproxy-preventextensions>
    // Step 1: "Return false."
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, _args, _win: &JsObject, _context| Ok(JsValue::new(false)),
            window.clone(),
        );
        let fn_obj = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("preventExtensions"))
            .length(1)
            .build();
        handler.create_data_property_or_throw(
            js_string!("preventExtensions"),
            fn_obj,
            context,
        )?;
    }

    // ── [[DefineOwnProperty]] trap ──
    // <https://html.spec.whatwg.org/#windowproxy-defineownproperty>
    // Array index: return false (spec step 2.1).
    // Non-array-index: Return ? OrdinaryDefineOwnProperty(W, P, Desc).
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, args, win: &JsObject, context| {
                let undefined_val = JsValue::undefined();
                let key = args.get(1).unwrap_or(&undefined_val);
                let desc_obj = args.get(2).unwrap_or(&undefined_val);

                // Step 2.1: "If P is an array index property name, return false."
                if is_array_index_key(key) {
                    return Ok(JsValue::new(false));
                }

                // Step 2.2: "Return ? OrdinaryDefineOwnProperty(W, P, Desc)."
                let desc = desc_from_obj(desc_obj, context)?;
                let prop_key = property_key_from_value_with_ctx(key, context);
                match win.define_property_or_throw(prop_key, desc, context) {
                    Ok(_) => Ok(JsValue::new(true)),
                    Err(_) => Ok(JsValue::new(false)),
                }
            },
            window.clone(),
        );
        let fn_obj = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("defineProperty"))
            .length(3)
            .build();
        handler.create_data_property_or_throw(js_string!("defineProperty"), fn_obj, context)?;
    }

    // ── [[Set]] trap ──
    // <https://html.spec.whatwg.org/#windowproxy-set>
    // Array index: return false (spec step 3.1).
    // Non-array-index: Return ? OrdinarySet(W, P, V, Receiver).
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, args, win: &JsObject, context| {
                let undefined_val = JsValue::undefined();
                let key = args.get(1).unwrap_or(&undefined_val);

                // Step 3.1: "If P is an array index property name, return false."
                if is_array_index_key(key) {
                    return Ok(JsValue::new(false));
                }

                // Step 3.2: "Return ? OrdinarySet(W, P, V, Receiver)."
                let value = args.get(2).cloned().unwrap_or(JsValue::undefined());
                let prop_key = property_key_from_value_with_ctx(key, context);
                // target.set() uses self as receiver, which is correct here
                // because OrdinarySet(W, P, V, Receiver) starts property
                // lookup on W (the Window), not on the proxy.
                let result = win.set(prop_key, value, false, context)?;
                Ok(JsValue::new(result))
            },
            window.clone(),
        );
        let fn_obj = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("set"))
            .length(4)
            .build();
        handler.create_data_property_or_throw(js_string!("set"), fn_obj, context)?;
    }

    // ── [[Delete]] trap ──
    // <https://html.spec.whatwg.org/#windowproxy-delete>
    // Array index: check own property (spec step 2.1).
    // Non-array-index: Return ? OrdinaryDelete(W, P).
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, args, win: &JsObject, context| {
                let undefined_val = JsValue::undefined();
                let key = args.get(1).unwrap_or(&undefined_val);

                // Step 2.1: "If P is an array index property name:"
                if is_array_index_key(key) {
                    let prop_key = property_key_from_value_with_ctx(key, context);
                    // Step 2.1.1: "Let desc be ! this.[[GetOwnProperty]](P)."
                    // Step 2.1.2: "If desc is undefined, return true."
                    // Step 2.1.3: "Return false."
                    let has = win.has_own_property(prop_key, context)?;
                    return Ok(JsValue::new(!has));
                }

                // Step 2.2: "Return ? OrdinaryDelete(W, P)."
                let prop_key = property_key_from_value_with_ctx(key, context);
                let result = win.delete_property_or_throw(prop_key, context)?;
                Ok(JsValue::new(result))
            },
            window.clone(),
        );
        let fn_obj = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("deleteProperty"))
            .length(2)
            .build();
        handler.create_data_property_or_throw(js_string!("deleteProperty"), fn_obj, context)?;
    }

    // ── [[GetPrototypeOf]] not provided ──
    // <https://html.spec.whatwg.org/#windowproxy-getprototypeof>
    // Same-origin: delegates to OrdinaryGetPrototypeOf(W) → Proxy default.
    // Cross-origin: return null — not yet active.

    // ── [[IsExtensible]] not provided ──
    // <https://html.spec.whatwg.org/#windowproxy-isextensible>
    // Proxy default delegates to target.[[IsExtensible]]().  For an
    // extensible Window this returns true, matching the spec.  Providing
    // a trap would risk Proxy invariant violations (if target differs).

    // ── [[GetOwnProperty]] not provided ──
    // <https://html.spec.whatwg.org/#windowproxy-getownproperty>
    // Same-origin non-array-index: Proxy default delegates to target's
    // [[GetOwnProperty]] which IS OrdinaryGetOwnProperty(W, P).
    // Array-index child navigable support not yet implemented.

    // ── [[Get]] not provided ──
    // <https://html.spec.whatwg.org/#windowproxy-get>
    // Same-origin: Proxy default calls target.[[Get]](P, Receiver) with
    // the proxy as receiver — this IS OrdinaryGet(this, P, Receiver).

    // ── [[HasProperty]] not provided ──
    // Not listed as overridden in the WindowProxy spec.  Proxy default is correct.

    // ── [[OwnPropertyKeys]] not provided ──
    // <https://html.spec.whatwg.org/#windowproxy-ownpropertykeys>
    // Same-origin: Proxy default returns target's own property keys.
    // Child navigable indices not yet implemented.

    // ── Create the Proxy ──
    Proxy::create(&JsValue::from(window.clone()), &handler.into(), context)
        .map(JsValue::from)
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Resolve the inner Window from a value that may be a WindowProxy
/// (Proxy exotic object wrapping a Window).  When accessors on
/// Window.prototype are invoked via a WindowProxy (e.g.
/// `proxy.onload = ...`), the receiver (`this`) is the Proxy object.
/// This function extracts the Proxy's target (the inner Window) so
/// that downcasts succeed.
pub(crate) fn resolve_window(value: &JsValue, context: &Context) -> JsObject {
    if let Some(object) = value.as_object() {
        if let Some(proxy) = object.downcast_ref::<Proxy>() {
            if let Ok((target, _)) = proxy.try_data() {
                return target;
            }
        }
        return object.clone();
    }
    context.global_object()
}

// ── Helper functions ──

/// <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>
fn is_array_index_key(key: &JsValue) -> bool {
    if let Some(s) = key.as_string() {
        let s = s.to_std_string_escaped();
        if s.is_empty() {
            return false;
        }
        let parsed: u64 = match s.parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if parsed >= u32::MAX as u64 {
            return false;
        }
        parsed.to_string() == s
    } else if key.is_number() {
        if let Some(n) = key.as_number() {
            n.fract() == 0.0 && n >= 0.0 && n < u32::MAX as f64
        } else {
            false
        }
    } else {
        false
    }
}

/// Convert a JsValue property key to a PropertyKey, using a Context
/// for ToString conversion when needed.
fn property_key_from_value_with_ctx(
    key: &JsValue,
    context: &mut Context,
) -> PropertyKey {
    if let Some(s) = key.as_string() {
        PropertyKey::String(s.clone())
    } else if let Some(n) = key.as_number() {
        if n.fract() == 0.0 && n >= 0.0 && n < u32::MAX as f64 {
            PropertyKey::from(n as u32)
        } else {
            PropertyKey::from(n)
        }
    } else if let Some(sym) = key.as_symbol() {
        PropertyKey::Symbol(sym)
    } else {
        // null, undefined, boolean — convert via ToString
        let s = key.to_string(context).unwrap_or_else(|_| js_string!(""));
        PropertyKey::String(s)
    }
}

/// Convert a JsValue descriptor object to a PropertyDescriptor.
fn desc_from_obj(desc_obj: &JsValue, context: &mut Context) -> JsResult<PropertyDescriptor> {
    match desc_obj.as_object() {
        Some(o) => o.to_property_descriptor(context),
        None => Err(JsNativeError::typ()
            .with_message("Property descriptor must be an object")
            .into()),
    }
}

// ── Cross-origin helper operations ──
// These implement the abstract operations from HTML spec § 7.2.1.3.
// They are ready for cross-origin support.

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
    keys.push(PropertyKey::Symbol(boa_engine::JsSymbol::is_concat_spreadable()));

    keys
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_window_properties()
        .iter()
        .any(|p| p.property == name)
}
