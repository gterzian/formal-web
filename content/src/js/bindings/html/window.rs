type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

use crate::html::windowproxy::resolve_window;
use crate::html::{
    Location, Window, WindowOrWorkerGlobalScope,
    safe_passing_of_structured_data::StructuredCloneOptions,
    window_computed_style_properties_for_element,
};
use crate::js::platform_objects;
use crate::js::try_with_event_target_mut;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{callback_function_value, nullable_value};

use crate::dom::Element;

use super::hyperlink_element_utils::document_creation_url;
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
        def.add_attribute(AttributeDef {
            id: "onload",
            getter: get_onload,
            setter: Some(set_onload),
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
            id: "location",
            getter: get_location,
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

fn structured_clone_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let undefined = ec.value_undefined();
    let value = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let options = parse_structured_clone_options(args.get(1), ec);

    let mut result = Err(ec.new_type_error("receiver is not a Window"));
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, ec2| {
            if let Some(window) = data.downcast_ref::<Window>() {
                result = window.structured_clone(value, options, ec2);
            }
        }),
    );
    result
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

fn open_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let url = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let target = ec.to_rust_string(args.get(1).cloned().unwrap_or_else(|| undefined.clone()))?;
    let features = ec.to_rust_string(args.get(2).cloned().unwrap_or_else(|| undefined))?;

    let window_object = current_window_object_from(this, ec);
    let mut result = Err(ec.new_type_error("receiver is not a Window"));
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, ec2| {
            if let Some(window) = data.downcast_ref::<Window>() {
                result = window.open(&url, &target, &features, ec2);
            }
        }),
    );
    result
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let callback = callback_function_value(args.first().unwrap_or(&undefined), ec)?;
    let window_object = current_window_object_from(this, ec);
    let mut handle = 0u32;
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, _ec2| {
            if let Some(window) = data.downcast_ref::<Window>() {
                handle = window.global_scope.request_animation_frame(callback);
            }
        }),
    );
    Ok(ec.value_from_number(handle as f64))
}

fn get_onload(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let err = ec.new_type_error("receiver is not a Window");
    let window = ec
        .with_object_any(&window_object)
        .and_then(|d| d.downcast_ref::<Window>())
        .ok_or(err)?;
    Ok(window
        .onload_value()
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

fn set_onload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let undefined = ec.value_undefined();
    let callback = nullable_value(
        args.first().unwrap_or(&undefined),
        ec,
        callback_function_value,
    )?;

    let previous = {
        if let Some(data) = ec.with_object_any_mut(&window_object) {
            if let Some(window) = data.downcast_mut::<Window>() {
                window.replace_onload(callback.clone())
            } else {
                return Err(ec.new_type_error("receiver is not a Window"));
            }
        } else {
            return Err(ec.new_type_error("receiver is not a Window"));
        }
    };

    if let Some(previous) = previous {
        let receiver = crate::js::Types::value_from_object(window_object.clone());
        try_with_event_target_mut(&receiver, ec, |target| {
            target.remove_event_listener_entry("load", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        let receiver = crate::js::Types::value_from_object(window_object.clone());
        try_with_event_target_mut(&receiver, ec, |target| {
            target.add_event_listener(
                &window_object,
                String::from("load"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            );
        })?;
    }

    Ok(ec.value_undefined())
}

fn get_parent(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(crate::js::Types::value_from_object(
        current_window_object_from(this, ec),
    ))
}

fn get_top(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(crate::js::Types::value_from_object(
        current_window_object_from(this, ec),
    ))
}

fn get_location(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let location_val = location_object(ec)?;
    Ok(crate::js::Types::value_from_object(location_val))
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let handle = ec.to_uint32(args.first().cloned().unwrap_or_else(|| undefined))?;
    let window_object = current_window_object_from(this, ec);
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, _ec2| {
            if let Some(window) = data.downcast_mut::<Window>() {
                window.global_scope.cancel_animation_frame(handle);
            }
        }),
    );
    Ok(ec.value_undefined())
}

fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let undefined = ec.value_undefined();
    let handler = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let delay = args.get(1).cloned().unwrap_or_else(|| undefined);
    let extra_args: Vec<JsValue> = args.iter().skip(2).cloned().collect();
    let mut result = Err(ec.new_type_error("receiver is not a Window"));
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, ec2| {
            if let Some(window) = data.downcast_ref::<Window>() {
                result = window
                    .set_timeout(&handler, &delay, extra_args.clone(), ec2)
                    .map(|id| ec2.value_from_number(id as f64));
            }
        }),
    );
    result
}

fn clear_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let timer_id = ec.to_uint32(args.first().cloned().unwrap_or_else(|| undefined))?;
    let window_object = current_window_object_from(this, ec);
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, _ec2| {
            if let Some(window) = data.downcast_mut::<Window>() {
                window.clear_timeout(timer_id);
            }
        }),
    );
    Ok(ec.value_undefined())
}

fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let undefined = ec.value_undefined();
    let handler = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let delay = args.get(1).cloned().unwrap_or_else(|| undefined);
    let extra_args: Vec<JsValue> = args.iter().skip(2).cloned().collect();
    let mut result = Err(ec.new_type_error("receiver is not a Window"));
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, ec2| {
            if let Some(window) = data.downcast_ref::<Window>() {
                result = window
                    .set_interval(&handler, &delay, extra_args.clone(), ec2)
                    .map(|id| ec2.value_from_number(id as f64));
            }
        }),
    );
    result
}

fn clear_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let timer_id = ec.to_uint32(args.first().cloned().unwrap_or_else(|| undefined))?;
    let window_object = current_window_object_from(this, ec);
    ec.with_object_any_mut_with(
        &window_object,
        Box::new(|data, _ec2| {
            if let Some(window) = data.downcast_mut::<Window>() {
                window.clear_interval(timer_id);
            }
        }),
    );
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
        let err_element = ec.new_type_error("receiver is not an Element");
        let err_object = ec.new_type_error("element receiver is not an object");
        let object = match args
            .first()
            .and_then(|v| <crate::js::Types as JsTypes>::value_as_object(v))
        {
            Some(o) => o,
            None => return Err(err_object),
        };
        let element = match ec
            .with_object_any(&object)
            .and_then(|a| a.downcast_ref::<Element>())
        {
            Some(e) => e,
            None => return Err(err_element),
        };
        window_computed_style_properties_for_element(element, pseudo_elt.as_deref())
    };
    // ec borrow from with_object_any is released here.
    style_declaration_object(&properties, ec).map(|obj| crate::js::Types::value_from_object(obj))
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///

fn location_object(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    if let Some(object) = platform_objects::location_object(ec)? {
        return Ok(object);
    }

    let url = document_creation_url(ec)?;
    let window = ec.global_object();
    let object =
        create_interface_instance::<crate::js::Types, Location>(Location::new(url, window), ec)?;
    platform_objects::store_location_object(ec, object.clone())?;
    Ok(object)
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
