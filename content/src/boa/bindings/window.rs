use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
};

use crate::dom::Window;
use crate::html::{cancel_animation_frame, request_animation_frame};
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
        );
    Ok(())
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    require_window_receiver(this)?;
    let callback = callback_function_value(args.get_or_undefined(0))?;
    Ok(JsValue::from(request_animation_frame(context, callback)?))
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    require_window_receiver(this)?;
    let handle = args.get_or_undefined(0).to_u32(context)?;
    cancel_animation_frame(context, handle)?;
    Ok(JsValue::undefined())
}

fn require_window_receiver(this: &JsValue) -> JsResult<()> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("window receiver is not an object"))?;
    if object.downcast_ref::<Window>().is_some() {
        return Ok(());
    }

    Err(JsNativeError::typ()
        .with_message("receiver is not a Window")
        .into())
}
