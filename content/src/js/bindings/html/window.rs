use boa_engine::{Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use std::marker::PhantomData;

use crate::html::{
    Location, Window, WindowOrWorkerGlobalScope, resolve_window,
    safe_passing_of_structured_data::StructuredCloneOptions,
    window_computed_style_properties_for_element,
};
use crate::js::platform_objects::{
    location_object as cached_location_object, store_location_object,
};
use crate::js::with_event_target_mut;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{callback_function_value, nullable_value};

use crate::js::bindings::dom::with_element_ref;

use super::{hyperlink_element_utils::document_creation_url, style_declaration_object};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for Window {
    const NAME: &'static str = "Window";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn is_global() -> bool {
        true
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let window_object = current_window_object(this, ctx);
    let window = crate::js::js_result_to_completion(downcast_window(&window_object), ctx)?;

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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let url = args
        .get(0)
        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
        .transpose()
        .map_err(|e| e.into_opaque(ctx).unwrap_or_else(|_| JsValue::undefined()))?
        .unwrap_or_default();
    let target = args
        .get(1)
        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
        .transpose()
        .map_err(|e| e.into_opaque(ctx).unwrap_or_else(|_| JsValue::undefined()))?
        .unwrap_or_default();
    let features = args
        .get(2)
        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
        .transpose()
        .map_err(|e| e.into_opaque(ctx).unwrap_or_else(|_| JsValue::undefined()))?
        .unwrap_or_default();

    let window_object = current_window_object(this, ctx);
    let window = crate::js::js_result_to_completion(downcast_window(&window_object), ctx)?;
    window.open(&url, &target, &features, ec)
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let callback = callback_function_value(args.get_or_undefined(0))?;
        let window_object = current_window_object(this, ctx);
        let window = downcast_window(&window_object)?;
        Ok(JsValue::from(
            window.global_scope.request_animation_frame(callback),
        ))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_onload(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let window_object = current_window_object(this, ctx);
        let window = downcast_window(&window_object)?;
        Ok(window
            .onload_value()
            .map(|callback| callback.to_js_value())
            .unwrap_or_else(JsValue::null))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_onload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let window_object = current_window_object(this, ctx);
        let callback = nullable_value(args.get_or_undefined(0), callback_function_value)?;
        let previous = with_window_mut(&window_object, |window| {
            window.replace_onload(callback.clone())
        })?;

        if let Some(previous) = previous {
            let receiver = JsValue::from(window_object.clone());
            with_event_target_mut(&receiver, |target| {
                target.remove_event_listener_entry("load", &previous, false);
            })?;
        }

        if let Some(callback) = callback {
            let receiver = JsValue::from(window_object.clone());
            with_event_target_mut(&receiver, |target| {
                target.add_event_listener(
                    &window_object,
                    String::from("load"),
                    Some(callback),
                    false,
                    false,
                    Some(false),
                    None,
                )
            })??;
        }

        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_parent(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> { Ok(JsValue::from(current_window_object(this, ctx))) })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_top(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> { Ok(JsValue::from(current_window_object(this, ctx))) })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_location(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let location_val = location_object(ec)?;
    Ok(JsValue::from(location_val))
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let handle = args.get_or_undefined(0).to_u32(ctx)?;
        let window_object = current_window_object(this, ctx);
        let window = downcast_window(&window_object)?;
        window.global_scope.cancel_animation_frame(handle);
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let window_object = current_window_object(this, ctx);
    let window = crate::js::js_result_to_completion(downcast_window(&window_object), ctx)?;
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let timer_id = crate::js::js_result_to_completion(args.get_or_undefined(0).to_u32(ctx), ctx)?;
    let window_object = current_window_object(this, ctx);
    let window = crate::js::js_result_to_completion(downcast_window(&window_object), ctx)?;
    window.clear_timeout(timer_id);
    Ok(JsValue::undefined())
}

fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let window_object = current_window_object(this, ctx);
    let window = crate::js::js_result_to_completion(downcast_window(&window_object), ctx)?;
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let timer_id = crate::js::js_result_to_completion(args.get_or_undefined(0).to_u32(ctx), ctx)?;
    let window_object = current_window_object(this, ctx);
    let window = crate::js::js_result_to_completion(downcast_window(&window_object), ctx)?;
    window.clear_interval(timer_id);
    Ok(JsValue::undefined())
}

fn get_computed_style_method(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let pseudo_elt = if args.get_or_undefined(1).is_null_or_undefined() {
            None
        } else {
            Some(
                args.get_or_undefined(1)
                    .to_string(ctx)?
                    .to_std_string_escaped(),
            )
        };

        with_element_ref(args.get_or_undefined(0), |element| {
            style_declaration_object(
                &window_computed_style_properties_for_element(element, pseudo_elt.as_deref()),
                ctx,
            )
            .map(JsValue::from)
        })?
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///

fn location_object(ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsObject, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsObject> {
        if let Some(object) = cached_location_object(ctx)? {
            return Ok(object);
        }

        let url = document_creation_url(ctx)?;
        let window = ctx.global_object();
        let object = create_interface_instance::<BoaTypes, Location>(
            Location::new(url, window),
            crate::js::context_as_ec(ctx),
        )
        .map_err(JsError::from_opaque)?;
        store_location_object(ctx, object.clone())?;
        Ok(object)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Resolve the Window from a receiver that may be a Window or a WindowProxy.
/// Delegates to the domain layer's `resolve_window`.
fn current_window_object(this: &JsValue, context: &Context) -> JsObject {
    resolve_window(this, context)
}

fn downcast_window(object: &JsObject) -> JsResult<boa_gc::GcRef<'_, Window>> {
    object.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("receiver is not a Window")
            .into()
    })
}

fn with_window_mut<R>(object: &JsObject, f: impl FnOnce(&mut Window) -> R) -> JsResult<R> {
    let Some(mut window) = object.downcast_mut::<Window>() else {
        return Err(JsNativeError::typ()
            .with_message("receiver is not a Window")
            .into());
    };
    Ok(f(&mut window))
}
