use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    object::JsObject,
    native_function::NativeFunction,
};

use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::webidl::callback_function_value;

use super::event_target::register_event_target_methods;

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
    class
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

fn set_timeout_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(window.set_timeout(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        context,
    )?))
}

fn clear_timeout_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let timer_id = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.clear_timeout(timer_id);
    Ok(JsValue::undefined())
}

fn set_interval_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(window.set_interval(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        context,
    )?))
}

fn clear_interval_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let timer_id = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.clear_interval(timer_id);
    Ok(JsValue::undefined())
}

fn current_window_object(this: &JsValue, context: &Context) -> JsObject {
    if let Some(object) = this.as_object() {
        return object.clone();
    }

    context.global_object()
}

fn downcast_window(object: &JsObject) -> JsResult<boa_gc::GcRef<'_, Window>> {
    object
        .downcast_ref::<Window>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not a Window").into())
}
