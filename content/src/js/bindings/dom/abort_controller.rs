use boa_engine::{JsNativeError, JsResult, JsValue};
use std::marker::PhantomData;

use crate::dom::{AbortController, AbortSignal, create_abort_signal};
use crate::js::{try_with_abort_controller_ref, with_abort_controller_ref};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use super::abort_signal::{abort_reason_from_argument, signal_abort_with_context};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for AbortController {
    const NAME: &'static str = "AbortController";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let signal = create_abort_signal(AbortSignal::new(), ec)?;
        Ok(AbortController::new(signal))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
            _phantom: PhantomData,

            id: "abort",
            length: 1,
            method: abort,
            static_: false,
            unforgeable: false,
            promise_type: false,
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
    Ok(JsValue::from(signal_object))
}

fn abort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortController receiver is not an object")
        })?;
        let signal = with_abort_controller_ref(&controller, |controller| controller.signal())?;
        signal_abort_with_context(&signal, reason, ctx)?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
