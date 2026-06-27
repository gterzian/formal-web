use boa_engine::{Context, JsNativeError, JsResult, JsValue};
use std::marker::PhantomData;

use crate::dom::{AbortController, AbortSignal, create_abort_signal};
use crate::js::with_abort_controller_ref;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use super::abort_signal::{abort_reason_from_argument, signal_abort_with_context};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for AbortController {
    const NAME: &'static str = "AbortController";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { crate::js::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
            let signal = create_abort_signal(AbortSignal::new(), ctx)?;
            Ok(AbortController::new(signal))
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortController receiver is not an object")
        })?;
        let signal =
            with_abort_controller_ref(&controller, |controller| controller.signal_object())??;
        Ok(JsValue::from(signal))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn abort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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
