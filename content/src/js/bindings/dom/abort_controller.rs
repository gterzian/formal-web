type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::dom::{AbortController, AbortSignal, create_abort_signal};
use crate::js::try_with_abort_controller_ref;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use super::abort_signal::{abort_reason_from_argument, signal_abort};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for AbortController {
    const NAME: &'static str = "AbortController";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let signal = create_abort_signal(AbortSignal::new(ec), ec)?;
        Ok(AbortController::new(signal))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "abort",
            length: 1,
            method: abort,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn get_signal(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortController receiver is not an object"))?;
    let signal_object =
        try_with_abort_controller_ref(&obj, ec, |controller| controller.signal_object())?;
    let signal_object = signal_object
        .ok_or_else(|| ec.new_type_error("AbortSignal is missing its JavaScript object"))?;
    Ok(crate::js::Types::value_from_object(signal_object))
}

fn abort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let controller = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortController receiver is not an object"))?;
    let signal = try_with_abort_controller_ref(&controller, ec, |controller| controller.signal())?;
    signal_abort(&signal, reason, ec)?;
    Ok(ec.value_undefined())
}
