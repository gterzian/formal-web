use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::JsObject,
    property::Attribute,
};

use crate::boa::platform_objects::{
    location_object as cached_location_object, store_location_object,
};
use crate::boa::with_event_target_mut;
use crate::html::{
    Location, Window, WindowOrWorkerGlobalScope, window_computed_style_properties_for_element,
};
use crate::webidl::{callback_function_value, nullable_value};

use crate::boa::bindings::dom::{register_event_target_methods, with_element_ref};

use super::{hyperlink_element_utils::document_creation_url, style_declaration_object};

impl Class for Window {
    const NAME: &'static str = "Window";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)?;
        register_window_methods(class)
    }
}

pub(crate) fn register_window_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("onload"),
            Some(NativeFunction::from_fn_ptr(get_onload).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_onload).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("parent"),
            Some(NativeFunction::from_fn_ptr(get_parent).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("top"),
            Some(NativeFunction::from_fn_ptr(get_top).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("location"),
            Some(NativeFunction::from_fn_ptr(get_location).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .method(
            js_string!("requestAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(request_animation_frame_method),
        )
        .method(
            js_string!("cancelAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(cancel_animation_frame_method),
        )
        .method(
            js_string!("setTimeout"),
            1,
            NativeFunction::from_fn_ptr(set_timeout_method),
        )
        .method(
            js_string!("clearTimeout"),
            1,
            NativeFunction::from_fn_ptr(clear_timeout_method),
        )
        .method(
            js_string!("setInterval"),
            1,
            NativeFunction::from_fn_ptr(set_interval_method),
        )
        .method(
            js_string!("clearInterval"),
            1,
            NativeFunction::from_fn_ptr(clear_interval_method),
        )
        .method(
            js_string!("getComputedStyle"),
            1,
            NativeFunction::from_fn_ptr(get_computed_style_method),
        );
    Ok(())
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let callback = callback_function_value(args.get_or_undefined(0))?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(
        window.global_scope.request_animation_frame(callback),
    ))
}

fn get_onload(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(window
        .onload_value()
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(JsValue::null))
}

fn set_onload(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
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
}

fn get_parent(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(current_window_object(this, context)))
}

fn get_top(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(current_window_object(this, context)))
}

fn get_location(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(location_object(context)?))
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let handle = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.global_scope.cancel_animation_frame(handle);
    Ok(JsValue::undefined())
}

fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(window.set_timeout(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        context,
    )?))
}

fn clear_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let timer_id = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.clear_timeout(timer_id);
    Ok(JsValue::undefined())
}

fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(window.set_interval(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        context,
    )?))
}

fn clear_interval_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let timer_id = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.clear_interval(timer_id);
    Ok(JsValue::undefined())
}

fn get_computed_style_method(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let pseudo_elt = if args.get_or_undefined(1).is_null_or_undefined() {
        None
    } else {
        Some(
            args.get_or_undefined(1)
                .to_string(context)?
                .to_std_string_escaped(),
        )
    };

    with_element_ref(args.get_or_undefined(0), |element| {
        style_declaration_object(
            &window_computed_style_properties_for_element(element, pseudo_elt.as_deref()),
            context,
        )
        .map(JsValue::from)
    })?
}

fn location_object(context: &mut Context) -> JsResult<JsObject> {
    if let Some(object) = cached_location_object(context)? {
        return Ok(object);
    }

    let url = document_creation_url(context)?;
    let window = context.global_object();
    let object = Location::from_data(Location::new(url, window), context)?;
    store_location_object(context, object.clone())?;
    Ok(object)
}

fn current_window_object(this: &JsValue, context: &Context) -> JsObject {
    if let Some(object) = this.as_object() {
        return object.clone();
    }

    context.global_object()
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
