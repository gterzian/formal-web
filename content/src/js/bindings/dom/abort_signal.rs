use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, js_string,
    native_function::NativeFunction,
};
use std::marker::PhantomData;

use crate::dom::{
    AbortSignal, DOMException, create_abort_signal, initialize_dependent_abort_signal, signal_abort,
};
use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::js::{with_abort_signal_mut, with_abort_signal_ref, with_event_target_mut};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{callback_function_value, nullable_value};

use super::event_target::ContextEventDispatchHost;
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for AbortSignal {
    const NAME: &'static str = "AbortSignal";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "aborted",
            getter: get_aborted,
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

            id: "reason",
            getter: get_reason,
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

            id: "onabort",
            getter: get_onabort,
            setter: Some(set_onabort),
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

            id: "throwIfAborted",
            length: 0,
            method: throw_if_aborted,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        // https://dom.spec.whatwg.org/#AbortSignal-static-members
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "abort",
            length: 1,
            method: abort_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "timeout",
            length: 1,
            method: timeout_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "any",
            length: 1,
            method: any_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
    }
}

pub(crate) fn abort_reason_from_argument(
    argument: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let Some(argument) = argument else {
        return abort_error_value(ec);
    };

    if argument.is_undefined() {
        return abort_error_value(ec);
    }

    Ok(argument.clone())
}

pub(crate) fn timeout_reason(context: &mut Context) -> Completion<JsValue, BoaTypes> {
    Ok(JsValue::from(create_interface_instance::<
        BoaTypes,
        DOMException,
    >(
        DOMException::timeout_error(),
        crate::js::context_as_ec(context),
    )?))
}

pub(crate) fn signal_abort_with_context(
    signal: &AbortSignal,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    let mut host = ContextEventDispatchHost::new(context);
    signal_abort(&mut host, signal, reason)
}

pub(crate) fn abort_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let signal = create_abort_signal(AbortSignal::aborted_with_reason(reason), ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    Ok(JsValue::from(signal.object().map_err(|e| {
        e.into_opaque(ctx).unwrap_or(value_undefined)
    })?))
}

pub(crate) fn timeout_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let milliseconds = ec.to_length(args.get_or_undefined(0).clone())?;
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let signal = create_abort_signal(AbortSignal::new(), ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    let callback = NativeFunction::from_copy_closure_with_captures(
        |_, _, signal: &AbortSignal, context| {
            let reason = timeout_reason(context).map_err(JsError::from_opaque)?;
            signal_abort_with_context(signal, reason, context)?;
            Ok(JsValue::undefined())
        },
        signal.clone(),
    )
    .to_js_function(ctx.realm());

    let global = ctx.global_object();
    let window = global
        .downcast_ref::<Window>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal.timeout() requires a Window global")
        })
        .map_err(|e| {
            JsError::from(e)
                .into_opaque(ctx)
                .unwrap_or(value_undefined.clone())
        })?;
    let _ = window
        .set_timeout(
            &JsValue::from(callback),
            &JsValue::from(milliseconds as f64),
            Vec::new(),
            ctx,
        )
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;

    Ok(JsValue::from(signal.object().map_err(|e| {
        e.into_opaque(ctx).unwrap_or(value_undefined)
    })?))
}

pub(crate) fn any_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let signals = sequence_abort_signals(args.get_or_undefined(0), ec)?;
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    let result_signal = create_abort_signal(AbortSignal::new(), ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    initialize_dependent_abort_signal(&result_signal, &signals)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    Ok(JsValue::from(result_signal.object().map_err(|e| {
        e.into_opaque(ctx).unwrap_or(value_undefined.clone())
    })?))
}

fn get_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let signal = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal receiver is not an object")
        })?;
        with_abort_signal_ref(&signal, |signal| JsValue::from(signal.aborted_value()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_reason(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let signal = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal receiver is not an object")
        })?;
        with_abort_signal_ref(&signal, |signal| signal.reason_value())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn throw_if_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let signal = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal receiver is not an object")
        })?;
        let aborted = with_abort_signal_ref(&signal, |signal| signal.aborted_value())?;
        if !aborted {
            return Ok(JsValue::undefined());
        }

        let reason = with_abort_signal_ref(&signal, |signal| signal.reason_value())?;
        Err(JsError::from_opaque(reason))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_onabort(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let signal = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal receiver is not an object")
        })?;
        with_abort_signal_ref(&signal, |signal| {
            signal
                .onabort_value()
                .map(|callback| callback.to_js_value())
                .unwrap_or_else(JsValue::null)
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_onabort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let signal_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal receiver is not an object")
        })?;
        let callback = nullable_value(args.get_or_undefined(0), callback_function_value)?;
        let previous =
            with_abort_signal_mut(this, |signal| signal.replace_onabort(callback.clone()))?;

        if let Some(previous) = previous {
            with_event_target_mut(this, |target| {
                target.remove_event_listener_entry("abort", &previous, false);
            })?;
        }

        if let Some(callback) = callback {
            with_event_target_mut(this, |target| {
                target.add_event_listener(
                    &signal_object,
                    String::from("abort"),
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

fn sequence_abort_signals(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<Vec<AbortSignal>, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<Vec<AbortSignal>> {
        let object = value.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("AbortSignal.any() requires a sequence of AbortSignal objects")
        })?;
        let length = object.get(js_string!("length"), ctx)?.to_length(ctx)?;
        let mut signals = Vec::with_capacity(length as usize);

        for index in 0..length {
            let signal_value = object.get(index, ctx)?;
            let signal_object = signal_value.as_object().ok_or_else(|| {
                JsNativeError::typ().with_message("AbortSignal.any() requires AbortSignal objects")
            })?;
            let signal = signal_object
                .downcast_ref::<AbortSignal>()
                .map(|signal| signal.clone())
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("AbortSignal.any() requires AbortSignal objects")
                })?;
            signals.push(signal);
        }

        Ok(signals)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn abort_error_value(ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        Ok(JsValue::from(
            create_interface_instance::<BoaTypes, DOMException>(
                DOMException::abort_error(),
                crate::js::context_as_ec(ctx),
            )
            .map_err(JsError::from_opaque)?,
        ))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
