use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
};

use crate::boa::with_abort_controller_ref;
use crate::dom::{AbortController, AbortSignal, create_abort_signal};
use crate::webidl::binding::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

use super::abort_signal::{abort_reason_from_argument, signal_abort_with_context};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for AbortController {
    const NAME: &'static str = "AbortController";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let signal = create_abort_signal(AbortSignal::new(), context)?;
        Ok(AbortController::new(signal))
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_attribute(AttributeDef {
            id: "signal",
            getter: get_signal,
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
            id: "abort",
            length: 1,
            method: abort,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
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
