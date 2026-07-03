use boa_engine::{JsArgs, JsValue};
use std::marker::PhantomData;

use crate::dom::{
    AbortSignal, DOMException, create_abort_signal, initialize_dependent_abort_signal,
    signal_abort as dom_signal_abort,
};
use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::js::downcast::{
    try_with_abort_signal_mut, try_with_abort_signal_ref, try_with_event_target_mut,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{callback_function_value, nullable_value_ec};

use super::event_target::EcDispatchHost;

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

pub(crate) fn timeout_reason(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let exc = create_interface_instance::<crate::js::Types, DOMException>(
        DOMException::timeout_error(),
        ec,
    )?;
    Ok(crate::js::Types::value_from_object(exc))
}

pub(crate) fn signal_abort(
    signal: &AbortSignal,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let mut host = EcDispatchHost::new(ec);
    dom_signal_abort(&mut host, signal, reason)
}

pub(crate) fn abort_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reason = abort_reason_from_argument(args.get(0), ec)?;
    let value_undefined = ec.value_undefined();
    let signal = create_abort_signal(AbortSignal::aborted_with_reason(reason), ec)?;
    Ok(crate::js::Types::value_from_object(
        signal.object().ok_or(value_undefined)?,
    ))
}

pub(crate) fn timeout_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    use crate::js::Types;

    let milliseconds = ec.to_length(args.get_or_undefined(0).clone())?;
    let signal = create_abort_signal(AbortSignal::new(), ec)?;

    // Create the timeout callback as a builtin function.
    let signal_for_callback = signal.clone();
    let callback_fn = ec.create_builtin_function(
        Box::new(move |_args, _this, inner_ec| {
            let reason = timeout_reason(inner_ec).unwrap_or_else(|_| inner_ec.value_undefined());
            signal_abort(&signal_for_callback, reason, inner_ec)?;
            Ok(inner_ec.value_undefined())
        }),
        0,
        ec.property_key_from_str(""),
    );

    let callback_val = Types::value_from_object(Types::object_from_function(callback_fn));
    let ms_val = ec.value_from_number(milliseconds as f64);

    // Get the Window from the global object and schedule the timeout.
    // Use with_object_any_mut_with to avoid borrow conflict between
    // the downcast result and the subsequent set_timeout call.
    let global = ec.global_object();
    let mut set_result: Completion<u32, crate::js::Types> = Ok(0);
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

    Ok(Types::value_from_object(
        signal.object().ok_or_else(|| ec.value_undefined())?,
    ))
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
    Ok(crate::js::Types::value_from_object(
        result_signal.object().ok_or(value_undefined)?,
    ))
}

fn get_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let signal_object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let callback = nullable_value_ec(args.get_or_undefined(0), ec, callback_function_value)?;

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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<AbortSignal>, crate::js::Types> {
    let object = crate::js::Types::value_as_object(value).ok_or_else(|| {
        ec.new_type_error("AbortSignal.any() requires a sequence of AbortSignal objects")
    })?;
    let length_key = ec.property_key_from_str("length");
    let length_val = ExecutionContext::get(ec, object.clone(), length_key)?;
    let length = ec.to_length(length_val)?;
    let mut signals = Vec::with_capacity(length as usize);

    for index in 0..length {
        let index_key = ec.property_key_from_index(index as u32);
        let signal_value = ExecutionContext::get(ec, object.clone(), index_key)?;
        let signal_object = crate::js::Types::value_as_object(&signal_value)
            .ok_or_else(|| ec.new_type_error("AbortSignal.any() requires AbortSignal objects"))?;
        let signal = try_with_abort_signal_ref(&signal_object, ec, |signal| signal.clone())?;
        signals.push(signal);
    }

    Ok(signals)
}

fn abort_error_value(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    match create_interface_instance::<crate::js::Types, DOMException>(
        DOMException::abort_error(),
        ec,
    ) {
        Ok(obj) => Ok(crate::js::Types::value_from_object(obj)),
        Err(e) => Err(e),
    }
}
