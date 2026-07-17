//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

use crate::html::Window;
use crate::webidl::is_array_index_key;

use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;
use crate::js::create_builtin_fn_with_traced_captures;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

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
// with `ec.create_builtin_fn()` and set as a property on the handler
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
    if is_array_index_key(&key, ec) {
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
    let win_val = <crate::js::Types as JsTypes>::value_from_object(win.clone());
    let result = ec.get_v(win_val, prop_key)?;

    // Note: Wrap callable results so they are invoked with `this` = the
    // Window target, not the WindowProxy.  The Proxy [[Get]] returns
    // trapResult, but the subsequent Call expression uses the base object
    // (the Proxy) as `this`, and resolve_window cannot extract the Window
    // from a Proxy.
    if let Some(func_obj) = <Types as JsTypes>::value_as_object(&result) {
        if ec.is_callable(&result) {
            let name_key = ec.property_key_from_str("wrapped");
            let wrapper_fn = create_builtin_fn_with_traced_captures(
                ec,
                WindowProxyGetCapture {
                    window: win.clone(),
                    original_fn: func_obj,
                },
                window_proxy_get_wrapper_behaviour,
                0,
                name_key,
                false,
            );
            let wrapper_obj = <Types as JsTypes>::object_from_function(wrapper_fn);
            return Ok(<Types as JsTypes>::value_from_object(wrapper_obj));
        }
    }

    Ok(result)
}

/// <https://html.spec.whatwg.org/#windowproxy-set>
fn trap_set(
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
    // Step 3: "If IsPlatformObjectSameOrigin(W) is true:"
    // Step 3.1: "If P is an array index property name, return false."
    if is_array_index_key(&key, ec) {
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
    if is_array_index_key(&key, ec) {
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

/// Extract the target Window from the proxy trap arguments.
///
/// The proxy target IS W (the Window object), passed as `args[0]` by the
/// ECMAScript Proxy internal methods (10.5).
fn target_window(args: &[JsValue]) -> Result<JsObject, JsValue> {
    args.first()
        .and_then(|value| <Types as JsTypes>::value_as_object(value))
        .ok_or_else(|| JsValue::default())
}

/// Captures for the wrapper function created by `trap_get`.
///
/// Stores the Window target (to use as `this` in the wrapped call) and
/// the original callable value (to invoke with the corrected `this`).
#[gc_struct]
struct WindowProxyGetCapture {
    /// The Window to use as `this` when calling the wrapped function.
    window: JsObject,
    /// The original callable function object to invoke.
    original_fn: JsObject,
}

/// Behaviour function for the wrapper created by `trap_get`.
///
/// Ignores `this` (which is the WindowProxy) and calls the original
/// function with `this` set to the captured Window.
fn window_proxy_get_wrapper_behaviour(
    args: &[JsValue],
    _this: JsValue,
    captures: &WindowProxyGetCapture,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let this_value = <Types as JsTypes>::value_from_object(captures.window.clone());
    ec.call(&captures.original_fn, &this_value, args)
}

/// <https://webidl.spec.whatwg.org/#js-observable-arrays>
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
    #[gc_struct]
    struct TrapCapture {
        #[ignore_trace]
        func: fn(
            &[JsValue],
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>,
    }

    fn trap_behaviour(
        args: &[JsValue],
        this: JsValue,
        captures: &TrapCapture,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        (captures.func)(args, this, ec)
    }

    for &(trap_fn, length, name) in traps.iter() {
        let name_key = ec.property_key_from_str(name);
        let builtin_fn = create_builtin_fn_with_traced_captures(
            ec,
            TrapCapture { func: trap_fn },
            trap_behaviour,
            length,
            name_key,
            false,
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

/// Resolve the Window from a value that may be a WindowProxy or a
/// direct Window object.
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
pub(crate) fn cross_origin_own_property_keys() -> Vec<<Types as JsTypes>::PropertyKey> {
    // Note: Cross-origin support requires engine-specific Symbol construction.
    // Return empty until cross-origin is implemented.
    Vec::new()
}

#[allow(dead_code)]
pub(crate) fn is_platform_object_same_origin(_w: &JsObject) -> bool {
    // In a single-origin content process, all accesses are same-origin.
    true
}
