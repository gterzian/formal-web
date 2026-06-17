//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

use boa_engine::{
    Context, JsData, JsNativeError, JsObject, JsResult, JsValue,
    builtins::proxy::Proxy,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, JsPrototype},
    property::{PropertyDescriptor, PropertyKey},
};
use boa_gc::{Finalize, Trace};

use crate::webidl::is_array_index_key;

/// <https://webidl.spec.whatwg.org/#creating-an-observable-array-exotic-object>
#[derive(Trace, Finalize)]
struct WindowProxyHandler {
    window: JsObject,
}

impl JsData for WindowProxyHandler {}

// Step 1 of every WindowProxy internal method:
// "Let W be the value of the [[Window]] internal slot of this."
fn handler_window(this: &JsValue) -> JsResult<JsObject> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WindowProxy trap called with non-object this")
    })?;
    let handler = obj.downcast_ref::<WindowProxyHandler>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WindowProxy trap called on non-WindowProxy handler")
    })?;
    Ok(handler.window.clone())
}

/// <https://html.spec.whatwg.org/#windowproxy-setprototypeof>
fn trap_set_prototype_of(
    this: &JsValue,
    args: &[JsValue],
    _captures: &WindowProxyHandler,
    _context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    // Step 1: "Return ! SetImmutablePrototype(this, V)."
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
}

/// <https://html.spec.whatwg.org/#windowproxy-preventextensions>
fn trap_prevent_extensions(
    _this: &JsValue,
    _args: &[JsValue],
    _captures: &WindowProxyHandler,
    _context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Return false."
    Ok(JsValue::new(false))
}

/// <https://html.spec.whatwg.org/#windowproxy-isextensible>
fn trap_is_extensible(
    _this: &JsValue,
    _args: &[JsValue],
    _captures: &WindowProxyHandler,
    _context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Return true."
    Ok(JsValue::new(true))
}

/// <https://html.spec.whatwg.org/#windowproxy-defineownproperty>
fn trap_define_property(
    this: &JsValue,
    args: &[JsValue],
    _captures: &WindowProxyHandler,
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    let undefined_val = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined_val);
    let desc_obj = args.get(2).unwrap_or(&undefined_val);

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
fn trap_get(
    this: &JsValue,
    args: &[JsValue],
    _captures: &WindowProxyHandler,
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    let undefined_val = JsValue::undefined();
    let key_val = args.get(1).unwrap_or(&undefined_val);

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
fn trap_set(
    this: &JsValue,
    args: &[JsValue],
    _captures: &WindowProxyHandler,
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    let undefined_val = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined_val);

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
    this: &JsValue,
    args: &[JsValue],
    _captures: &WindowProxyHandler,
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    let undefined_val = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined_val);

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
fn trap_has(
    this: &JsValue,
    args: &[JsValue],
    _captures: &WindowProxyHandler,
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    let undefined_val = JsValue::undefined();
    let key = args.get(1).unwrap_or(&undefined_val);

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

/// <https://html.spec.whatwg.org/#windowproxy-getprototypeof>
fn trap_get_prototype_of(
    this: &JsValue,
    _args: &[JsValue],
    _captures: &WindowProxyHandler,
    _context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;
    // Step 2: "If IsPlatformObjectSameOrigin(W) is true, then return !
    //           OrdinaryGetPrototypeOf(W)."
    let proto = win.prototype();
    match proto {
        Some(p) => Ok(JsValue::from(p)),
        // Step 3: "Return null."
        None => Ok(JsValue::null()),
    }
}

/// <https://html.spec.whatwg.org/#windowproxy-ownpropertykeys>
fn trap_own_keys(
    this: &JsValue,
    _args: &[JsValue],
    _captures: &WindowProxyHandler,
    context: &mut Context,
) -> JsResult<JsValue> {
    let win = handler_window(this)?;

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

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
pub(crate) fn create_window_proxy(
    window: &JsObject,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Note: The handler is created with a [[Window]] internal slot.
    let handler_proto: JsPrototype = None;
    let handler: JsObject = JsObject::<WindowProxyHandler>::new(
        context.root_shape(),
        handler_proto,
        WindowProxyHandler {
            window: window.clone(),
        },
    )
    .upcast();

    // Note: Each trap is registered via CreateBuiltinFunction +
    // CreateDataPropertyOrThrow, matching the observable array pattern.
    let traps: &[(
        &str,
        usize,
        fn(&JsValue, &[JsValue], &WindowProxyHandler, &mut Context) -> JsResult<JsValue>,
    )] = &[
        ("getPrototypeOf", 1, trap_get_prototype_of as _),
        ("setPrototypeOf", 2, trap_set_prototype_of as _),
        ("isExtensible", 1, trap_is_extensible as _),
        ("preventExtensions", 1, trap_prevent_extensions as _),

        ("defineProperty", 3, trap_define_property as _),
        ("get", 3, trap_get as _),
        ("set", 4, trap_set as _),
        ("deleteProperty", 2, trap_delete_property as _),
        ("has", 2, trap_has as _),
        ("ownKeys", 1, trap_own_keys as _),
    ];

    for &(name_str, length, trap_fn) in traps {
        let name = js_string!(name_str);
        // Note: The captures value is a dummy; the real handler is
        // read from `this` inside each trap function.
        let trap = NativeFunction::from_copy_closure_with_captures(
            move |this, args, _captures: &WindowProxyHandler, context| {
                trap_fn(this, args, _captures, context)
            },
            WindowProxyHandler {
                window: window.clone(),
            },
        );
        let fn_obj = FunctionObjectBuilder::new(context.realm(), trap)
            .name(name.clone())
            .length(length)
            .build();
        handler.create_data_property_or_throw(name, fn_obj, context)?;
    }

    Proxy::create(&JsValue::from(window.clone()), &handler.into(), context)
        .map(JsValue::from)
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
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

fn desc_from_obj(desc_obj: &JsValue, context: &mut Context) -> JsResult<PropertyDescriptor> {
    match desc_obj.as_object() {
        Some(o) => o.to_property_descriptor(context),
        None => Err(JsNativeError::typ()
            .with_message("Property descriptor must be an object")
            .into()),
    }
}

#[allow(dead_code)]
struct CrossOriginPropertyEntry {
    property: &'static str,
    needs_get: bool,
    needs_set: bool,
}

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

#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_window_properties().iter().any(|p| p.property == name)
}
