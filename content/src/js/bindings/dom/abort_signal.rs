use crate::dom::{
    AbortSignal, DOMException, create_abort_signal, initialize_dependent_abort_signal,
    signal_abort as dom_signal_abort,
};
use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::js::{
    create_builtin_fn_with_traced_captures, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_target_mut,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{callback_function_value, nullable_value};


use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for AbortSignal {
    const NAME: &'static str = "AbortSignal";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "throwIfAborted",
            length: 0,
            method: throw_if_aborted,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        // https://dom.spec.whatwg.org/#AbortSignal-static-members
        def.add_operation(OperationDef {
            id: "abort",
            length: 1,
            method: abort_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "timeout",
            length: 1,
            method: timeout_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "any",
            length: 1,
            method: any_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

pub(crate) fn abort_reason_from_argument(
    argument: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let Some(argument) = argument else {
        return abort_error_value(ec);
    };

    if argument.is_undefined() {
        return abort_error_value(ec);
    }

    Ok(argument.clone())
}

pub(crate) fn timeout_reason(ec: &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> {
    let exc = create_interface_instance::<Types, DOMException>(DOMException::timeout_error(), ec)?;
    Ok(<Types as JsTypes>::value_from_object(exc))
}

pub(crate) fn signal_abort(
    signal: &AbortSignal,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    dom_signal_abort(ec, signal, reason)
}

pub(crate) fn abort_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let signal = create_abort_signal(AbortSignal::aborted_with_reason(reason), ec)?;
    Ok(<Types as JsTypes>::value_from_object(
        signal.object().ok_or_else(|| ec.value_undefined())?,
    ))
}

pub(crate) fn timeout_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value_undefined = ec.value_undefined();
    let milliseconds = ec.to_length(args.get(0).cloned().unwrap_or(value_undefined))?;
    let signal = create_abort_signal(AbortSignal::new(ec), ec)?;

    // Create the timeout callback as a builtin function.
    let name_key = ec.property_key_from_str("");
    let callback_fn = create_builtin_fn_with_traced_captures(
        ec,
        signal.clone(),
        abort_signal_timeout_callback_fn,
        0,
        name_key,
        false,
    );
    let callback_val = <Types as JsTypes>::value_from_object(
        <Types as JsTypes>::object_from_function(callback_fn),
    );
    let ms_val = ec.value_from_number(milliseconds as f64);

    // Get the Window from the global object and schedule the timeout.
    // Use with_object_any_mut_with to avoid borrow conflict between
    // the downcast result and the subsequent set_timeout call.
    let global = ec.global_object();
    let mut set_result: Completion<u32, Types> = Ok(0);
    ec.with_object_any_mut_with(
        &global,
        Box::new(|data, ec2| {
            set_result = match data.downcast_ref::<Window>() {
                Some(window) => window.set_timeout(&callback_val, &ms_val, Vec::new(), ec2),
                None => Err(ec2.new_type_error("AbortSignal.timeout() requires a Window global")),
            };
        }),
    );
    set_result?;

    Ok(<Types as JsTypes>::value_from_object(
        signal.object().ok_or_else(|| ec.value_undefined())?,
    ))
}

/// Handler for `AbortSignal.timeout` callback.
/// Aborts the signal with a timeout reason.
fn abort_signal_timeout_callback_fn(
    _args: &[JsValue],
    _this: JsValue,
    signal: &AbortSignal,
    inner_ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reason = timeout_reason(inner_ec).unwrap_or_else(|_| inner_ec.value_undefined());
    signal_abort(signal, reason, inner_ec)?;
    Ok(inner_ec.value_undefined())
}

pub(crate) fn any_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value_undefined = ec.value_undefined();
    let signals = sequence_abort_signals(args.get(0).unwrap_or(&value_undefined), ec)?;
    let result_signal = create_abort_signal(AbortSignal::new(ec), ec)?;
    initialize_dependent_abort_signal(&result_signal, &signals);
    Ok(<Types as JsTypes>::value_from_object(
        result_signal.object().ok_or(value_undefined)?,
    ))
}

fn get_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            return Ok(ec.value_from_bool(signal.aborted_value()));
        }
    }
    Err(ec.new_type_error("object is not an AbortSignal"))
}

fn get_reason(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            return Ok(signal.reason_value());
        }
    }
    Err(ec.new_type_error("object is not an AbortSignal"))
}

fn throw_if_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            if !signal.aborted_value() {
                return Ok(ec.value_undefined());
            }
            return Err(signal.reason_value());
        }
    }
    Err(ec.new_type_error("object is not an AbortSignal"))
}

fn get_onabort(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            let callback = signal.onabort_value();
            return Ok(callback
                .map(|c| c.to_js_value())
                .unwrap_or_else(|| ec.value_null()));
        }
    }
    Err(ec.new_type_error("object is not an AbortSignal"))
}

fn set_onabort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let signal_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let callback = nullable_value(
        args.get(0).unwrap_or(&ec.value_undefined()),
        ec,
        callback_function_value,
    )?;

    let previous =
        try_with_abort_signal_mut(this, ec, |signal| signal.replace_onabort(callback.clone()))?;

    if let Some(previous) = previous {
        try_with_event_target_mut(this, ec, |target| {
            target.remove_event_listener_entry("abort", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        try_with_event_target_mut(this, ec, |target| {
            target.add_event_listener(
                &signal_object,
                String::from("abort"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            );
        })?;
    }

    Ok(ec.value_undefined())
}

fn sequence_abort_signals(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<AbortSignal>, Types> {
    let object = <Types as JsTypes>::value_as_object(value).ok_or_else(|| {
        ec.new_type_error("AbortSignal.any() requires a sequence of AbortSignal objects")
    })?;
    let length_key = ec.property_key_from_str("length");
    let length_val = ExecutionContext::get(ec, object.clone(), length_key)?;
    let length = ec.to_length(length_val)?;
    let mut signals = Vec::with_capacity(length as usize);

    for index in 0..length {
        let index_key = ec.property_key_from_index(index as u32);
        let signal_value = ExecutionContext::get(ec, object.clone(), index_key)?;
        let signal_object = <Types as JsTypes>::value_as_object(&signal_value)
            .ok_or_else(|| ec.new_type_error("AbortSignal.any() requires AbortSignal objects"))?;
        let signal = try_with_abort_signal_ref(&signal_object, ec, |signal| signal.clone())?;
        signals.push(signal);
    }

    Ok(signals)
}

fn abort_error_value(ec: &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> {
    match create_interface_instance::<Types, DOMException>(DOMException::abort_error(), ec) {
        Ok(obj) => Ok(<Types as JsTypes>::value_from_object(obj)),
        Err(e) => Err(e),
    }
}
