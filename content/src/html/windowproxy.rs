//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    builtins::proxy::Proxy,
    js_string,
    object::builtins::JsProxyBuilder,
    property::{PropertyDescriptor, PropertyKey},
};

use crate::html::Window;
use crate::webidl::is_array_index_key;
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// ── Trap functions ──
//
// Each trap is a `NativeFunctionPointer`:
//     fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>
//
// Per the ECMAScript Proxy internal methods (10.5), the **target** is always
// the first argument (`args[0]`).  Since the WindowProxy is created with the
// Window as the proxy target, `args[0]` IS the Window in every trap call.
//
// Note: `JsProxyBuilder` is a Boa-specific convenience API used here instead
// of the lower-level `ProxyCreate(target, handler)` algorithm from ECMAScript.
// It lets us supply each trap as a plain function pointer — no captures, no
// custom handler struct — and the builder wires them into the handler object
// and calls `ProxyCreate` internally.

/// <https://html.spec.whatwg.org/#windowproxy-getprototypeof>
fn trap_get_prototype_of(
    _this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let win = target_window(args)?;

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true, then return !
    //           OrdinaryGetPrototypeOf(W)."
    let proto = win.prototype();
    match proto {
        Some(p) => Ok(JsValue::from(p)),
        // Step 3: "Return null."
        None => Ok(JsValue::null()),
    }
}

/// <https://html.spec.whatwg.org/#windowproxy-setprototypeof>
fn trap_set_prototype_of(
    _this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let win = target_window(args)?;

    // Step 1: "Return ! SetImmutablePrototype(this, V)."
    let current = win.prototype();
    let undefined = JsValue::undefined();
    let val = args.get(1).unwrap_or(&undefined);
    let same = match (&current, val) {
        (Some(current_proto), _) => val.as_object().map_or(false, |v| *current_proto == v),
        (None, _) => val.is_null(),
    };
    Ok(JsValue::new(same))
}

/// <https://html.spec.whatwg.org/#windowproxy-preventextensions>
fn trap_prevent_extensions(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Return false."
    Ok(JsValue::new(false))
}

/// <https://html.spec.whatwg.org/#windowproxy-isextensible>
fn trap_is_extensible(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Return true."
    Ok(JsValue::new(true))
}

/// <https://html.spec.whatwg.org/#windowproxy-defineownproperty>
fn trap_define_property(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = target_window(args)?;
    let undefined = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined);
    let desc_obj = args.get(2).unwrap_or(&undefined);

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 2.1: "If P is an array index property name, return false."
    if is_array_index_key(key) {
        return Ok(JsValue::new(false));
    }

    // Step 2.2: "Return ? OrdinaryDefineOwnProperty(W, P, Desc)."
    let desc = desc_from_obj(desc_obj, context)?;
    let prop_key = key.to_property_key(context)?;
    match win.define_property_or_throw(prop_key, desc, context) {
        Ok(_) => Ok(JsValue::new(true)),
        Err(_) => Ok(JsValue::new(false)),
    }
}

/// <https://html.spec.whatwg.org/#windowproxy-get>
fn trap_get(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let win = target_window(args)?;
    let undefined = JsValue::undefined();
    let key_val = args.get(1).unwrap_or(&undefined);

    // Step 2: "Check if an access between two browsing contexts should be
    //           reported, given the current global object's browsing context,
    //           W's browsing context, P, and the current settings object."
    // Note: Access reporting is not yet implemented.
    // Step 3: "If IsPlatformObjectSameOrigin(W) is true, then return ?
    //           OrdinaryGet(this, P, Receiver)."
    let prop_key = key_val.to_property_key(context)?;
    win.get(prop_key, context)
}

/// <https://html.spec.whatwg.org/#windowproxy-set>
fn trap_set(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let win = target_window(args)?;
    let undefined = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined);

    // Step 2: "Check if an access between two browsing contexts should be
    //           reported, given the current global object's browsing context,
    //           W's browsing context, P, and the current settings object."
    // Note: Access reporting is not yet implemented.
    // Step 3: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 3.1: "If P is an array index property name, return false."
    if is_array_index_key(key) {
        return Ok(JsValue::new(false));
    }

    // Step 3.2: "Return ? OrdinarySet(W, P, V, Receiver)."
    let value = args.get(2).cloned().unwrap_or(JsValue::undefined());
    let prop_key = key.to_property_key(context)?;
    let result = win.set(prop_key, value, false, context)?;
    Ok(JsValue::new(result))
}

/// <https://html.spec.whatwg.org/#windowproxy-delete>
fn trap_delete_property(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = target_window(args)?;
    let undefined = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined);

    // Step 2: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 2.1: "If P is an array index property name:"
    if is_array_index_key(key) {
        let prop_key = key.to_property_key(context)?;
        // Step 2.1.1: "Let desc be ! this.[[GetOwnProperty]](P)."
        // Note: Uses has_own_property (public API) instead of
        // [[GetOwnProperty]] (pub(crate)).  The result is equivalent:
        // if desc is undefined, return true; otherwise return false.
        // Step 2.1.2: "If desc is undefined, then return true."
        // Step 2.1.3: "Return false."
        let has = win.has_own_property(prop_key, context)?;
        return Ok(JsValue::new(!has));
    }

    // Step 2.2: "Return ? OrdinaryDelete(W, P)."
    let prop_key = key.to_property_key(context)?;
    let result = win.delete_property_or_throw(prop_key, context)?;
    Ok(JsValue::new(result))
}

/// <https://html.spec.whatwg.org/#windowproxy-has>
fn trap_has(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let win = target_window(args)?;
    let undefined = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined);

    // Note: The WindowProxy spec does not override [[HasProperty]].  This
    // trap is provided for completeness.  "length" returns true (child
    // frame count); all other keys delegate to the target's [[HasProperty]].
    if let Some(s) = key.as_string() {
        if s == "length" {
            return Ok(JsValue::new(true));
        }
    }

    let prop_key = key.to_property_key(context)?;
    let result = win.has_property(prop_key, context)?;
    Ok(JsValue::new(result))
}

/// <https://html.spec.whatwg.org/#windowproxy-ownpropertykeys>
fn trap_own_keys(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let win = target_window(args)?;

    // Step 2: "Let maxProperties be W's associated Document's document-tree
    //          child navigables's size."
    // Note: Child navigable support not yet implemented — keys is empty.
    // Step 3: "Let keys be the range 0 to maxProperties, exclusive."
    // Step 4: "If IsPlatformObjectSameOrigin(W) is true, then return the
    //           concatenation of keys and OrdinaryOwnPropertyKeys(W)."
    let window_keys = win.own_property_keys(context)?;
    let key_values: Vec<JsValue> = window_keys.into_iter().map(JsValue::from).collect();

    let key_array = JsObject::with_object_proto(context.intrinsics());
    for (i, val) in key_values.iter().enumerate() {
        key_array
            .create_data_property_or_throw(i as u32, val.clone(), context)
            .expect("CreateArrayFromList: creating array properties");
    }
    key_array.set_prototype(None);
    key_array
        .create_data_property_or_throw(
            js_string!("length"),
            JsValue::new(key_values.len()),
            context,
        )
        .expect("CreateArrayFromList: setting length");

    Ok(JsValue::from(key_array))
}

// ── Helpers ──

/// Extract the target Window from the proxy trap arguments.
///
/// The proxy target IS W (the Window object), passed as `args[0]` by the
/// ECMAScript Proxy internal methods (10.5).
fn target_window(args: &[JsValue]) -> JsResult<JsObject> {
    args.first()
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WindowProxy trap: missing target Window")
                .into()
        })
}

fn desc_from_obj(desc_obj: &JsValue, context: &mut Context) -> JsResult<PropertyDescriptor> {
    match desc_obj.as_object() {
        Some(object) => object.to_property_descriptor(context),
        None => Err(JsNativeError::typ()
            .with_message("Property descriptor must be an object")
            .into()),
    }
}

// ── Public API ──

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Note: Uses `JsProxyBuilder` (Boa's higher-level public Proxy API) to
/// construct the WindowProxy with native-function traps for each of the
/// 10 overridden internal methods.  This is a Boa-specific convenience;
/// the ECMAScript spec constructs a Proxy via `ProxyCreate(target, handler)`.
pub(crate) fn create_window_proxy(
    window: &JsObject,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context.
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    let proxy = JsProxyBuilder::new(window.clone())
        .get_prototype_of(trap_get_prototype_of)
        .set_prototype_of(trap_set_prototype_of)
        .is_extensible(trap_is_extensible)
        .prevent_extensions(trap_prevent_extensions)
        .define_property(trap_define_property)
        .get(trap_get)
        .set(trap_set)
        .delete_property(trap_delete_property)
        .has(trap_has)
        .own_keys(trap_own_keys)
        .build(context)
        .map_err(|_error| {
            crate::js::native_error_to_js_value(
                JsNativeError::typ().with_message("WindowProxy creation failed"),
                context,
            )
        })?;
    Ok(JsValue::from(proxy))
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Resolve the Window from a value that may be a WindowProxy (Proxy) or a
/// direct Window object.  For same-origin WindowProxies, the target Window
/// is the context's global object.
///
/// Note: This cannot use `Proxy::try_data()` (pub(crate) in upstream Boa)
/// to extract the target, so it checks whether the object is a Proxy and
/// falls back to `context.global_object()` for the same-origin case.
pub(crate) fn resolve_window(value: &JsValue, ec: &mut dyn ExecutionContext<BoaTypes>) -> JsObject {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context.
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    if let Some(object) = value.as_object() {
        // Direct Window: return as-is.
        if object.is::<Window>() {
            return object;
        }
        // Proxy (WindowProxy): the target is the global (same-origin).
        if object.is::<Proxy>() {
            return context.global_object();
        }
        return object;
    }
    context.global_object()
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

#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_window_properties()
        .iter()
        .any(|p| p.property == name)
}
