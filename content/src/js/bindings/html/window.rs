use boa_engine::{Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use std::marker::PhantomData;

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
use crate::js::bindings::dom::with_element_ref;

use super::hyperlink_element_utils::document_creation_url;
use super::style_declaration_object;

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

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
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "requestAnimationFrame",
            length: 1,
            method: request_animation_frame_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "cancelAnimationFrame",
            length: 1,
            method: cancel_animation_frame_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "setTimeout",
            length: 1,
            method: set_timeout_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "clearTimeout",
            length: 1,
            method: clear_timeout_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "setInterval",
            length: 1,
            method: set_interval_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "clearInterval",
            length: 1,
            method: clear_interval_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "getComputedStyle",
            length: 1,
            method: get_computed_style_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "open",
            length: 0,
            method: open_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "structuredClone",
            length: 1,
            method: structured_clone_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

fn structured_clone_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;

    let value = args.get_or_undefined(0).clone();
    let options = args.get(1).and_then(parse_structured_clone_options);

    window.structured_clone(value, options, ec)
}

fn parse_structured_clone_options(value: &JsValue) -> Option<StructuredCloneOptions> {
    let object = value.as_object()?;
    // Try to get options["transfer"]
    let _ = object;
    // For now, we create a simple options check
    None
}

fn open_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let url_val = args.get(0).cloned().unwrap_or_default();
    let url = ec.to_rust_string(url_val.clone())?;
    let target_val = args.get(1).cloned().unwrap_or_default();
    let target = ec.to_rust_string(target_val.clone())?;
    let features_val = args.get(2).cloned().unwrap_or_default();
    let features = ec.to_rust_string(features_val.clone())?;

    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    window.open(&url, &target, &features, ec)
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let callback = callback_function_value(args.get_or_undefined(0), ec)?;
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    Ok(JsValue::from(
        window.global_scope.request_animation_frame(callback),
    ))
}

fn get_onload(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let window = window_object
        .downcast_ref::<Window>()
        .ok_or_else(|| ec.new_type_error("receiver is not a Window"))?;
    Ok(window
        .onload_value()
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(JsValue::null))
}

fn set_onload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let callback = nullable_value(args.get_or_undefined(0), ec, callback_function_value)?;

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
        let receiver = JsValue::from(window_object.clone());
        try_with_event_target_mut(&receiver, ec, |target| {
            target.remove_event_listener_entry("load", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        let receiver = JsValue::from(window_object.clone());
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

    Ok(JsValue::undefined())
}

fn get_parent(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(JsValue::from(current_window_object_from(this, ec)))
}

fn get_top(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(JsValue::from(current_window_object_from(this, ec)))
}

fn get_location(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let location_val = location_object(ec)?;
    Ok(JsValue::from(location_val))
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let handle = ec.to_uint32(args.get_or_undefined(0).clone())?;
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    window.global_scope.cancel_animation_frame(handle);
    Ok(JsValue::undefined())
}

fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    Ok(JsValue::from(window.set_timeout(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        ec,
    )?))
}

fn clear_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let timer_id = ec.to_uint32(args.get_or_undefined(0).clone())?;
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    window.clear_timeout(timer_id);
    Ok(JsValue::undefined())
}

fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    Ok(JsValue::from(window.set_interval(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        ec,
    )?))
}

fn clear_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let timer_id = ec.to_uint32(args.get_or_undefined(0).clone())?;
    let window_object = current_window_object_from(this, ec);
    let window = downcast_window(&window_object, ec)?;
    window.clear_interval(timer_id);
    Ok(JsValue::undefined())
}

fn get_computed_style_method(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let pseudo_elt = if args.get_or_undefined(1).is_null_or_undefined() {
        None
    } else {
        Some(ec.to_rust_string(args.get_or_undefined(1).clone())?)
    };

    // Extract element ref using with_object_any, release ec borrow before calling _ec fn.
    let properties = {
        let err_element = ec.new_type_error("receiver is not an Element");
        let err_object = ec.new_type_error("element receiver is not an object");
        let object = match <crate::js::Types as JsTypes>::value_as_object(args.get_or_undefined(0))
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
    style_declaration_object(&properties, ec).map(JsValue::from)
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

fn downcast_window<'a>(
    object: &'a JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<boa_gc::GcRef<'a, Window>, crate::js::Types> {
    object
        .downcast_ref::<Window>()
        .ok_or_else(|| ec.new_type_error("receiver is not a Window"))
}

fn with_window_mut<R>(object: &JsObject, f: impl FnOnce(&mut Window) -> R) -> JsResult<R> {
    let Some(mut window) = object.downcast_mut::<Window>() else {
        return Err(JsNativeError::typ()
            .with_message("receiver is not a Window")
            .into());
    };
    Ok(f(&mut window))
}
