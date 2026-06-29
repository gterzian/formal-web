use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, js_string,
    native_function::NativeFunction,
};
use std::marker::PhantomData;

use crate::dom::{
    AbortSignal, DOMException, create_abort_signal, initialize_dependent_abort_signal, signal_abort,
};
use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::js::{with_abort_signal_mut, with_event_target_mut};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{callback_function_value, nullable_value};

use super::event_target::ContextEventDispatchHost;

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for AbortSignal {
    const NAME: &'static str = "AbortSignal";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let Some(argument) = argument else {
        return abort_error_value(ec);
    };

    if argument.is_undefined() {
        return abort_error_value(ec);
    }

    Ok(argument.clone())
}

pub(crate) fn timeout_reason(context: &mut Context) -> Completion<JsValue, crate::js::Types> {
    Ok(JsValue::from(create_interface_instance::<
        crate::js::Types,
        DOMException,
    >(
        DOMException::timeout_error(),
        js_engine::boa::context_as_ec(context),
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let value_undefined = ec.value_undefined();
    let signal = create_abort_signal(AbortSignal::aborted_with_reason(reason), ec)?;
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    Ok(JsValue::from(signal.object().map_err(|e| {
        e.into_opaque(ctx).unwrap_or(value_undefined)
    })?))
}

pub(crate) fn timeout_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let milliseconds = ec.to_length(args.get_or_undefined(0).clone())?;
    let signal = create_abort_signal(AbortSignal::new(), ec)?;
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
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
            let js_error: JsError = JsError::from(e);
            js_error.into_opaque(ctx).unwrap_or(value_undefined.clone())
        })?;
    // Use context_as_ec to get ec from ctx for the set_timeout call,
    // avoiding a double borrow on the original ec.
    let ec_ref = js_engine::boa::context_as_ec(ctx);
    window.set_timeout(
        &JsValue::from(callback),
        &JsValue::from(milliseconds as f64),
        Vec::new(),
        ec_ref,
    )?;

    Ok(JsValue::from(signal.object().map_err(|e| {
        e.into_opaque(ctx).unwrap_or(value_undefined)
    })?))
}

pub(crate) fn any_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let signals = sequence_abort_signals(args.get_or_undefined(0), ec)?;
    let result_signal = create_abort_signal(AbortSignal::new(), ec)?;
    initialize_dependent_abort_signal(&result_signal, &signals);
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    Ok(JsValue::from(result_signal.object().map_err(|e| {
        e.into_opaque(ctx).unwrap_or(value_undefined.clone())
    })?))
}

fn get_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let signal = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let aborted = signal
        .downcast_ref::<AbortSignal>()
        .ok_or_else(|| ec.new_type_error("object is not an AbortSignal"))?
        .aborted_value();
    Ok(JsValue::from(aborted))
}

fn get_reason(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let signal = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let reason = signal
        .downcast_ref::<AbortSignal>()
        .ok_or_else(|| ec.new_type_error("object is not an AbortSignal"))?
        .reason_value();
    Ok(reason)
}

fn throw_if_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let signal = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let aborted_signal = signal
        .downcast_ref::<AbortSignal>()
        .ok_or_else(|| ec.new_type_error("object is not an AbortSignal"))?;
    if !aborted_signal.aborted_value() {
        return Ok(JsValue::undefined());
    }
    Err(aborted_signal.reason_value())
}

fn get_onabort(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let signal = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let callback = signal
        .downcast_ref::<AbortSignal>()
        .ok_or_else(|| ec.new_type_error("object is not an AbortSignal"))?
        .onabort_value();
    Ok(callback
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(JsValue::null))
}

fn set_onabort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<AbortSignal>, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
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

fn abort_error_value(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        Ok(JsValue::from(
            create_interface_instance::<crate::js::Types, DOMException>(
                DOMException::abort_error(),
                js_engine::boa::context_as_ec(ctx),
            )
            .map_err(JsError::from_opaque)?,
        ))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
