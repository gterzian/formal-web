use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::boa::with_abort_controller_ref;
use crate::dom::{AbortController, AbortSignal, create_abort_signal};

use super::abort_signal::{abort_reason_from_argument, signal_abort_with_context};

impl Class for AbortController {
    const NAME: &'static str = "AbortController";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let signal = create_abort_signal(AbortSignal::new(), context)?;
        Ok(AbortController::new(signal))
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_abort_controller_methods(class)
    }
}

pub(crate) fn register_abort_controller_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("signal"),
            Some(NativeFunction::from_fn_ptr(get_signal).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .method(js_string!("abort"), 1, NativeFunction::from_fn_ptr(abort));
    Ok(())
}

fn get_signal(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let controller = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortController receiver is not an object")
    })?;
    let signal = with_abort_controller_ref(&controller, |controller| controller.signal_object())??;
    Ok(JsValue::from(signal))
}

fn abort(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortController receiver is not an object")
    })?;
    let signal = with_abort_controller_ref(&controller, |controller| controller.signal())?;
    let reason = abort_reason_from_argument(args.get(0), context)?;
    signal_abort_with_context(&signal, reason, context)?;
    Ok(JsValue::undefined())
}
