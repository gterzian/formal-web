//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

use boa_engine::{JsNativeError, JsObject, JsValue};

use crate::html::Window;
use crate::webidl::is_array_index_key;

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── Trap functions ──
//
// Each trap is called by the Proxy internal methods and receives:
//     fn(args: &[JsValue], _this: JsValue, ec: &mut dyn ExecutionContext<crate::js::Types>)
//         -> Completion<JsValue, crate::js::Types>
//
// Per the ECMAScript Proxy internal methods (10.5), the **target** is always
// the first argument (`args[0]`).  Since the WindowProxy is created with the
// Window as the proxy target, `args[0]` IS the Window in every trap call.
//
// These functions are used as built-in function behaviours: each is wrapped
// with `ec.create_builtin_function()` and set as a property on the handler
// object passed to `ec.create_proxy()`.

/// <https://html.spec.whatwg.org/#windowproxy-getprototypeof>
fn trap_get_prototype_of(
    _args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(_args)?;

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true, then return !
    //           OrdinaryGetPrototypeOf(W)."
    let proto = ec.get_prototype_of(win)?;
    match proto {
        Some(p) => Ok(<crate::js::Types as JsTypes>::value_from_object(p)),
        // Step 3: "Return null."
        None => Ok(ec.value_null()),
    }
}

/// <https://html.spec.whatwg.org/#windowproxy-setprototypeof>
fn trap_set_prototype_of(
    args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(args)?;
    let val = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());

    // Step 1: "Return ! SetImmutablePrototype(this, V)."
    let current = ec.get_prototype_of(win)?;
    let same = match (&current, val.as_object()) {
        (Some(current_proto), Some(v)) => *current_proto == v,
        (None, None) => val.is_null(),
        _ => false,
    };
    Ok(ec.value_from_bool(same))
}

/// <https://html.spec.whatwg.org/#windowproxy-preventextensions>
fn trap_prevent_extensions(
    _args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 1: "Return false."
    Ok(ec.value_from_bool(false))
}

/// <https://html.spec.whatwg.org/#windowproxy-isextensible>
fn trap_is_extensible(
    _args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 1: "Return true."
    Ok(ec.value_from_bool(true))
}

/// <https://html.spec.whatwg.org/#windowproxy-defineownproperty>
fn trap_define_property(
    args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(args)?;
    let key = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());
    let desc_obj_val = args.get(2).cloned().unwrap_or_else(|| ec.value_undefined());

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 2.1: "If P is an array index property name, return false."
    if is_array_index_key(&key) {
        return Ok(ec.value_from_bool(false));
    }

    // Step 2.2: "Return ? OrdinaryDefineOwnProperty(W, P, Desc)."
    let desc_obj = ec.to_object(desc_obj_val)?;
    let desc = ec.to_property_descriptor(desc_obj)?;
    let prop_key = ec.to_property_key(key)?;
    match ec.define_property_or_throw(win, prop_key, desc) {
        Ok(_) => Ok(ec.value_from_bool(true)),
        Err(_) => Ok(ec.value_from_bool(false)),
    }
}

/// <https://html.spec.whatwg.org/#windowproxy-get>
fn trap_get(
    args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(args)?;
    let key = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());

    // Step 2: "Check if an access between two browsing contexts should be
    //           reported, given the current global object's browsing context,
    //           W's browsing context, P, and the current settings object."
    // Note: Access reporting is not yet implemented.
    // Step 3: "If IsPlatformObjectSameOrigin(W) is true, then return ?
    //           OrdinaryGet(this, P, Receiver)."
    let prop_key = ec.to_property_key(key)?;
    let win_val = <crate::js::Types as JsTypes>::value_from_object(win);
    ec.get_v(win_val, prop_key)
}

/// <https://html.spec.whatwg.org/#windowproxy-set>
fn trap_set(
    args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(args)?;
    let key = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());
    let receiver = args.get(3).cloned().unwrap_or_else(|| _this);

    // Step 2: "Check if an access between two browsing contexts should be
    //           reported, given the current global object's browsing context,
    //           W's browsing context, P, and the current settings object."
    // Note: Access reporting is not yet implemented.
    // Step 3: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 3.1: "If P is an array index property name, return false."
    if is_array_index_key(&key) {
        return Ok(ec.value_from_bool(false));
    }

    // Step 3.2: "Return ? OrdinarySet(W, P, V, Receiver)."
    let value = args.get(2).cloned().unwrap_or_else(|| ec.value_undefined());
    let prop_key = ec.to_property_key(key)?;
    ec.set(win, prop_key, value, false)?;
    Ok(ec.value_from_bool(true))
}

/// <https://html.spec.whatwg.org/#windowproxy-delete>
fn trap_delete_property(
    args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(args)?;
    let key = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 2.1: "If P is an array index property name:"
    if is_array_index_key(&key) {
        let prop_key = ec.to_property_key(key)?;
        // Step 2.1.1: "Let desc be ! this.[[GetOwnProperty]](P)."
        // Uses has_own_property as proxy for "desc is undefined".
        // Step 2.1.2: "If desc is undefined, then return true."
        // Step 2.1.3: "Return false."
        let has = ec.has_own_property(win, prop_key)?;
        return Ok(ec.value_from_bool(!has));
    }

    // Step 2.2: "Return ? OrdinaryDelete(W, P)."
    let prop_key = ec.to_property_key(key)?;
    ec.delete_property_or_throw(win, prop_key)?;
    Ok(ec.value_from_bool(true))
}

/// <https://html.spec.whatwg.org/#windowproxy-has>
fn trap_has(
    args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(args)?;
    let key = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());

    // Note: The WindowProxy spec does not override [[HasProperty]].  This
    // trap is provided for completeness.  "length" returns true (child
    // frame count); all other keys delegate to the target's [[HasProperty]].
    if let Some(s) = key.as_string() {
        if s == "length" {
            return Ok(ec.value_from_bool(true));
        }
    }

    let prop_key = ec.to_property_key(key)?;
    let result = ec.has_property(win, prop_key)?;
    Ok(ec.value_from_bool(result))
}

/// <https://html.spec.whatwg.org/#windowproxy-ownpropertykeys>
fn trap_own_keys(
    _args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let win = target_window(_args)?;

    // Step 2: "Let maxProperties be W's associated Document's document-tree
    //          child navigables's size."
    // Note: Child navigable support not yet implemented — keys is empty.
    // Step 3: "Let keys be the range 0 to maxProperties, exclusive."
    // Step 4: "If IsPlatformObjectSameOrigin(W) is true, then return the
    //           concatenation of keys and OrdinaryOwnPropertyKeys(W)."
    let window_keys = ec.own_property_keys(win)?;
    let key_array = ec.create_empty_array();
    for val in window_keys.into_iter() {
        let js_val: JsValue = val.into();
        ec.array_push(&key_array, js_val)?;
    }
    Ok(<crate::js::Types as JsTypes>::value_from_object(key_array))
}

// ── Helper ──

/// Extract the target Window from the proxy trap arguments.
///
/// The proxy target IS W (the Window object), passed as `args[0]` by the
/// ECMAScript Proxy internal methods (10.5).
fn target_window(args: &[JsValue]) -> Result<JsObject, JsValue> {
    args.first()
        .and_then(|value| value.as_object())
        .ok_or_else(|| JsValue::undefined())
}

// ── Public API ──

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Creates a WindowProxy exotic object by following the same recipe as
/// <https://webidl.spec.whatwg.org/#js-observable-arrays>:
/// 1. Create a handler object via OrdinaryObjectCreate(null)
/// 2. For each trap, CreateBuiltinFunction → CreateDataPropertyOrThrow(handler, name, fn)
/// 3. ProxyCreate(window, handler) — see `ec.create_proxy()`
///
/// The 10 trap functions implement the WindowProxy override algorithms from
/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>.
pub(crate) fn create_window_proxy(
    window: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let handler = ec.create_plain_object(None::<&JsObject>);

    let traps: &[(
        fn(
            &[JsValue],
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>,
        u32,
        &str,
    )] = &[
        (trap_get_prototype_of, 1, "getPrototypeOf"),
        (trap_set_prototype_of, 2, "setPrototypeOf"),
        (trap_is_extensible, 1, "isExtensible"),
        (trap_prevent_extensions, 1, "preventExtensions"),
        (trap_define_property, 3, "defineProperty"),
        (trap_get, 3, "get"),
        (trap_set, 4, "set"),
        (trap_delete_property, 2, "deleteProperty"),
        (trap_has, 2, "has"),
        (trap_own_keys, 1, "ownKeys"),
    ];
    for &(trap_fn, length, name) in traps.iter() {
        let builtin_fn = ec.create_builtin_function(
            Box::new(move |args, this, ec| trap_fn(args, this, ec)),
            length,
            ec.property_key_from_str(name),
        );
        let builtin_fn_jsobj = <crate::js::Types as JsTypes>::object_from_function(builtin_fn);
        ec.set(
            handler.clone(),
            ec.property_key_from_str(name),
            <crate::js::Types as JsTypes>::value_from_object(builtin_fn_jsobj),
            false,
        )?;
    }

    let proxy = ec.create_proxy(window.clone(), handler)?;
    Ok(<crate::js::Types as JsTypes>::value_from_object(proxy))
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Resolve the Window from a value that may be a WindowProxy (Proxy) or a
/// direct Window object.  For same-origin WindowProxies, the target Window
/// is the realm\'s global object.
pub(crate) fn resolve_window(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsObject {
    if let Some(object) = value.as_object() {
        // Direct Window: check via with_object_any downcast.
        if let Some(_) = ec
            .with_object_any(&object)
            .and_then(|a| a.downcast_ref::<Window>())
        {
            return object;
        }
        // For non-Window objects (Proxy or unknown), return the global.
        return ec.global_object();
    }

    // For non-object values, fall back to the global object.
    ec.global_object()
}

// ── Cross-origin support (unreachable in single-origin content process) ──

#[allow(dead_code)]
struct CrossOriginPropertyEntry {
    property: &'static str,
    needs_get: bool,
    needs_set: bool,
}

#[allow(dead_code)]
fn cross_origin_window_properties() -> Vec<CrossOriginPropertyEntry> {
    vec![
        CrossOriginPropertyEntry {
            property: "window",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "self",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "location",
            needs_get: true,
            needs_set: true,
        },
        CrossOriginPropertyEntry {
            property: "close",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "closed",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "focus",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "blur",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "frames",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "length",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "top",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "opener",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "parent",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "postMessage",
            needs_get: false,
            needs_set: false,
        },
    ]
}

#[allow(dead_code)]
pub(crate) fn cross_origin_own_property_keys() -> Vec<boa_engine::property::PropertyKey> {
    let mut keys: Vec<boa_engine::property::PropertyKey> = cross_origin_window_properties()
        .into_iter()
        .map(|p| boa_engine::property::PropertyKey::String(boa_engine::js_string!(p.property)))
        .collect();
    keys.push(boa_engine::property::PropertyKey::String(
        boa_engine::js_string!("then"),
    ));
    keys.push(boa_engine::property::PropertyKey::Symbol(
        boa_engine::JsSymbol::to_string_tag(),
    ));
    keys.push(boa_engine::property::PropertyKey::Symbol(
        boa_engine::JsSymbol::has_instance(),
    ));
    keys.push(boa_engine::property::PropertyKey::Symbol(
        boa_engine::JsSymbol::is_concat_spreadable(),
    ));
    keys
}

#[allow(dead_code)]
pub(crate) fn is_platform_object_same_origin(_w: &JsObject) -> bool {
    // In a single-origin content process, all accesses are same-origin.
    true
}
