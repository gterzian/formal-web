type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

use crate::html::windowproxy::resolve_window;
use crate::html::{
    PostMessageOptions, Window, WindowOrWorkerGlobalScope,
    structured_data::safe_passing_of_structured_data::StructuredCloneOptions,
    window_computed_style_properties_for_element,
};
use crate::js::bindings::html::global_event_handlers::define_global_event_handlers;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::callback_function_value;

use super::style_declaration_object;

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for Window {
    const NAME: &'static str = "Window";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn is_global() -> bool {
        true
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        define_global_event_handlers(def);
        def.add_attribute(AttributeDef {
            id: "window",
            getter: get_window,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "self",
            getter: get_self,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "document",
            getter: get_document,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "name",
            getter: get_name,
            setter: Some(set_name),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "location",
            getter: get_location,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: Some("href"),
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "close",
            length: 0,
            method: close_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "closed",
            getter: get_closed,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "focus",
            length: 0,
            method: focus_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "blur",
            length: 0,
            method: blur_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "frames",
            getter: get_frames,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "length",
            getter: get_length,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "top",
            getter: get_top,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "opener",
            getter: get_opener,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "parent",
            getter: get_parent,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "requestAnimationFrame",
            length: 1,
            method: request_animation_frame_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "cancelAnimationFrame",
            length: 1,
            method: cancel_animation_frame_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "setTimeout",
            length: 1,
            method: set_timeout_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "clearTimeout",
            length: 1,
            method: clear_timeout_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "setInterval",
            length: 1,
            method: set_interval_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "clearInterval",
            length: 1,
            method: clear_interval_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "getComputedStyle",
            length: 1,
            method: get_computed_style_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "open",
            length: 0,
            method: open_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        // The embedder host-message channel: not a Web IDL member, so it
        // carries no spec anchor. `webview::Embedder::host_message` is the
        // other end.
        def.add_operation(OperationDef {
            id: "__formalWebPostHostMessage",
            length: 1,
            method: post_host_message_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "postMessage",
            length: 1,
            method: post_message_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "structuredClone",
            length: 1,
            method: structured_clone_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

/// Resolve the domain [`Window`] from a receiver that may be a Window or a
/// WindowProxy, cloning it out of the object registry so no borrow is live
/// while the domain method runs.
fn window_domain_from(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Window, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    ec.with_object_any(&window_object)
        .and_then(|data| data.downcast_ref::<Window>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not a Window"))
}

fn structured_clone_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let options = parse_structured_clone_options(args.get(1), ec);

    // Clone the Window out of the object registry so no borrow on the global
    // object is live while the structured-clone algorithm runs: on the Boa
    // backend the window is the realm global object, and a mutable borrow held
    // across engine calls panics when the algorithm touches the global (e.g.
    // construct_typed_array_view looks up the typed-array constructor on it).
    let window = window_domain_from(this, ec)?;
    window.structured_clone(value, options, ec)
}

fn parse_structured_clone_options(
    options_arg: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Option<StructuredCloneOptions> {
    let options_val = options_arg?;
    let object = <crate::js::Types as JsTypes>::value_as_object(options_val)?;
    // Get options["transfer"]
    let transfer_key = ec.property_key_from_str("transfer");
    let Ok(transfer_value) =
        ExecutionContext::<crate::js::Types>::get(ec, object.clone(), transfer_key)
    else {
        return Some(StructuredCloneOptions { transfer: None });
    };
    if transfer_value.is_undefined() {
        return Some(StructuredCloneOptions { transfer: None });
    }
    // Convert JS array to Vec<JsValue>
    let transfer_object = match <crate::js::Types as JsTypes>::value_as_object(&transfer_value) {
        Some(obj) => obj,
        None => return Some(StructuredCloneOptions { transfer: None }),
    };
    let length_key = ec.property_key_from_str("length");
    let Ok(length_val) =
        ExecutionContext::<crate::js::Types>::get(ec, transfer_object.clone(), length_key)
    else {
        return Some(StructuredCloneOptions { transfer: None });
    };
    let Ok(length) = ec.to_length(length_val) else {
        return Some(StructuredCloneOptions { transfer: None });
    };
    if length == 0 {
        return Some(StructuredCloneOptions { transfer: None });
    }
    let mut transfer = Vec::with_capacity(length as usize);
    for i in 0..length {
        let idx_key = ec.property_key_from_str(&i.to_string());
        if let Ok(item) =
            ExecutionContext::<crate::js::Types>::get(ec, transfer_object.clone(), idx_key)
        {
            transfer.push(item);
        }
    }
    Some(StructuredCloneOptions {
        transfer: Some(transfer),
    })
}

fn post_message_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://webidl.spec.whatwg.org/#es-operations>
    // The message argument is required; calling with no arguments is a
    // Web IDL TypeError.
    let message = args
        .first()
        .cloned()
        .ok_or_else(|| ec.new_type_error("postMessage: message argument is required"))?;
    let options = parse_post_message_options(args, ec)?;

    let window = window_domain_from(this, ec)?;
    window.post_message(message, options, ec)?;
    Ok(ec.value_undefined())
}

/// <https://html.spec.whatwg.org/#dom-window-postmessage-options>
pub(crate) fn parse_post_message_options(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<PostMessageOptions, crate::js::Types> {
    let undefined = ec.value_undefined();

    // <https://html.spec.whatwg.org/#dom-window-postmessage-options>
    // The `postMessage(message, options)` form takes a dictionary as its
    // second argument; the legacy `postMessage(message, targetOrigin,
    // transfer)` form takes a string.  Web IDL overload resolution picks the
    // legacy form when the second argument is a string, and the options form
    // otherwise (including `undefined`).
    let is_legacy_form = args
        .get(1)
        .is_some_and(|second| crate::js::Types::value_as_string(second).is_some());
    if is_legacy_form {
        let second = args.get(1).cloned().unwrap_or_else(|| undefined.clone());
        let target_origin = ec.to_rust_string(second)?;
        let transfer = parse_transfer_sequence(args.get(2), ec)?;
        Ok(PostMessageOptions {
            target_origin,
            transfer,
        })
    } else {
        let options_value = args.get(1).cloned().unwrap_or_else(|| undefined.clone());
        Ok(PostMessageOptions {
            target_origin: options_dict_string(&options_value, ec, "targetOrigin", "/")?,
            transfer: options_dict_transfer(&options_value, ec)?,
        })
    }
}

/// Read a string dictionary member, applying the Web IDL default when the
/// member is absent or `undefined`.
fn options_dict_string(
    dict: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    key: &str,
    default: &str,
) -> Completion<String, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(dict) else {
        return Ok(default.to_owned());
    };
    let key_pk = ec.property_key_from_str(key);
    let value = ExecutionContext::get(ec, object, key_pk)?;
    if crate::js::Types::value_is_undefined(&value) {
        return Ok(default.to_owned());
    }
    ec.to_rust_string(value)
}

/// Read the `transfer` member (a `sequence<object>`) from the options
/// dictionary.
fn options_dict_transfer(
    dict: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<JsValue>, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(dict) else {
        return Ok(Vec::new());
    };
    let key_pk = ec.property_key_from_str("transfer");
    let value = ExecutionContext::get(ec, object, key_pk)?;
    parse_transfer_sequence(Some(&value), ec)
}

/// Convert the `transfer` argument (a `sequence<object>`) to a list of
/// values per Web IDL: an absent or `undefined` value converts to the
/// default empty sequence, a non-object or non-iterable value throws a
/// TypeError, and each iterated element must be an object.
/// <https://webidl.spec.whatwg.org/#es-sequence>
fn parse_transfer_sequence(
    transfer_value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<JsValue>, crate::js::Types> {
    let Some(transfer_value) = transfer_value else {
        return Ok(Vec::new());
    };
    if crate::js::Types::value_is_undefined(transfer_value) {
        return Ok(Vec::new());
    }
    if crate::js::Types::value_as_object(transfer_value).is_none() {
        // Step 1: If V is not an Object, throw a TypeError.
        return Err(ec.new_type_error("transfer is not an object"));
    }
    // Step 2: Let method be ? GetMethod(V, %Symbol.iterator%).
    // Step 3: If method is undefined, throw a TypeError.
    // Step 4: Return the result of creating a sequence from V and method.
    let mut iterator =
        ec.get_iterator(transfer_value.clone(), js_engine::IteratorKind::Sync, None)?;
    let mut transfer = Vec::new();
    loop {
        let next = ec.iterator_step_value(&mut iterator)?;
        let Some(next) = next else {
            break;
        };
        // <https://webidl.spec.whatwg.org/#es-object>
        // Each element converts to the `object` IDL type.
        if crate::js::Types::value_as_object(&next).is_none() {
            return Err(ec.new_type_error("transfer element is not an object"));
        }
        transfer.push(next);
    }
    Ok(transfer)
}

fn open_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let url = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let target = ec.to_rust_string(args.get(1).cloned().unwrap_or_else(|| undefined.clone()))?;
    let features = ec.to_rust_string(args.get(2).cloned().unwrap_or_else(|| undefined))?;

    let window = window_domain_from(this, ec)?;
    window.open(&url, &target, &features, ec)
}

fn post_host_message_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let body = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let window = window_domain_from(this, ec)?;
    window.post_host_message(body);
    Ok(undefined)
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let callback = callback_function_value(args.first().unwrap_or(&undefined), ec)?;
    let window = window_domain_from(this, ec)?;
    let handle = window.global_scope.request_animation_frame(callback, ec);
    Ok(ec.value_from_number(handle as f64))
}

fn get_window(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-window>
    let window = window_domain_from(this, ec)?;
    Ok(window.window_value(ec))
}

fn get_self(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-self>
    let window = window_domain_from(this, ec)?;
    Ok(window.self_value(ec))
}

fn get_frames(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-frames>
    let window = window_domain_from(this, ec)?;
    Ok(window.frames_value(ec))
}

fn get_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-name>
    let window = window_domain_from(this, ec)?;
    let name = window.name_value();
    Ok(ec.value_from_string(ec.js_string_from_str(&name)))
}

fn set_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-name>
    let window = window_domain_from(this, ec)?;
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    window.set_name_value(value);
    Ok(ec.value_undefined())
}

fn get_length(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-length>
    let window = window_domain_from(this, ec)?;
    Ok(ec.value_from_number(window.length_value() as f64))
}

fn get_top(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-top>
    let window = window_domain_from(this, ec)?;
    window.top_value(ec)
}

fn get_parent(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-parent>
    let window = window_domain_from(this, ec)?;
    window.parent_value(ec)
}

fn get_opener(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-opener>
    let window = window_domain_from(this, ec)?;
    Ok(window.opener_value(ec))
}

fn get_document(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-document>
    let window = window_domain_from(this, ec)?;
    window.document_value(ec)
}

fn get_location(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-location>
    let window = window_domain_from(this, ec)?;
    // The domain method creates the Location on first access and caches its
    // JS object on the global scope; the binding returns that cached object.
    window.location_value(ec)?;
    let location_object = window
        .global_scope
        .location_object(ec)
        .ok_or_else(|| ec.new_type_error("window has no Location object"))?;
    Ok(<crate::js::Types as JsTypes>::value_from_object(
        location_object,
    ))
}

fn close_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-window-close>
    let window = window_domain_from(this, ec)?;
    window.close();
    Ok(ec.value_undefined())
}

fn get_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-window-closed>
    let window = window_domain_from(this, ec)?;
    Ok(ec.value_from_bool(window.closed_value()))
}

fn focus_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-window-focus>
    let window = window_domain_from(this, ec)?;
    window.focus();
    Ok(ec.value_undefined())
}

fn blur_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // <https://html.spec.whatwg.org/#dom-window-blur>
    let window = window_domain_from(this, ec)?;
    window.blur();
    Ok(ec.value_undefined())
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let handle = ec.to_uint32(args.first().cloned().unwrap_or_else(|| undefined))?;
    let window = window_domain_from(this, ec)?;
    window.global_scope.cancel_animation_frame(handle, ec);
    Ok(ec.value_undefined())
}

fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let handler = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let delay = args.get(1).cloned().unwrap_or_else(|| undefined);
    let extra_args: Vec<JsValue> = args.iter().skip(2).cloned().collect();
    let window = window_domain_from(this, ec)?;
    window
        .set_timeout(&handler, &delay, extra_args, ec)
        .map(|id| ec.value_from_number(id as f64))
}

fn clear_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let timer_id = ec.to_uint32(args.first().cloned().unwrap_or_else(|| undefined))?;
    let window = window_domain_from(this, ec)?;
    window.clear_timeout(timer_id, ec);
    Ok(ec.value_undefined())
}

fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let handler = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let delay = args.get(1).cloned().unwrap_or_else(|| undefined);
    let extra_args: Vec<JsValue> = args.iter().skip(2).cloned().collect();
    let window = window_domain_from(this, ec)?;
    window
        .set_interval(&handler, &delay, extra_args, ec)
        .map(|id| ec.value_from_number(id as f64))
}

fn clear_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let timer_id = ec.to_uint32(args.first().cloned().unwrap_or_else(|| undefined))?;
    let window = window_domain_from(this, ec)?;
    window.clear_interval(timer_id, ec);
    Ok(ec.value_undefined())
}

fn get_computed_style_method(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let pseudo_elt = if args.get(1).map_or(true, |v| {
        crate::js::Types::value_is_null(v) || crate::js::Types::value_is_undefined(v)
    }) {
        None
    } else {
        Some(ec.to_rust_string(args.get(1).cloned().unwrap_or_else(|| undefined.clone()))?)
    };

    // Extract element ref using with_object_any, release ec borrow before calling _ec fn.
    let properties = {
        let err_object = ec.new_type_error("element receiver is not an object");
        let object = match args
            .first()
            .and_then(|v| <crate::js::Types as JsTypes>::value_as_object(v))
        {
            Some(o) => o,
            None => return Err(err_object),
        };
        let receiver = <crate::js::Types as JsTypes>::value_from_object(object.clone());
        let element = crate::js::bindings::dom::try_with_element_ref(&receiver, ec, |element| {
            element.clone()
        })?;
        window_computed_style_properties_for_element(&element, pseudo_elt.as_deref())
    };
    // ec borrow from with_object_any is released here.
    style_declaration_object(&properties, ec).map(|obj| crate::js::Types::value_from_object(obj))
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Resolve the Window from a receiver that may be a Window or a WindowProxy.
/// Delegates to the domain layer's `resolve_window`.
fn current_window_object_from(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsObject {
    resolve_window(this, ec)
}
