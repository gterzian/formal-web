use crate::dom::{
    AbortSignal, DOMException, create_abort_signal, initialize_dependent_abort_signal,
    signal_abort as dom_signal_abort,
};
use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::js::{
    create_builtin_fn_with_traced_captures, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_target_mut, with_cloned_platform_mut,
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
    let signal = create_abort_signal(AbortSignal::aborted_with_reason(reason, ec), ec)?;
    Ok(<Types as JsTypes>::value_from_object(
        signal.object(ec).ok_or_else(|| ec.value_undefined())?,
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

    // Get the Window from the global object and schedule the timeout. The
    // window is cloned out so `set_timeout` can use `ec` without holding a
    // borrow into the platform data; the clone is written back so the direct
    // timer-counter state persists.
    let global = ec.global_object();
    let set_result = with_cloned_platform_mut::<Window, _>(&global, ec, |window, ec| {
        window.set_timeout(&callback_val, &ms_val, Vec::new(), ec)
    })
    .unwrap_or_else(|| Err(ec.new_type_error("AbortSignal.timeout() requires a Window global")));
    set_result?;

    Ok(<Types as JsTypes>::value_from_object(
        signal.object(ec).ok_or_else(|| ec.value_undefined())?,
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
    initialize_dependent_abort_signal(&result_signal, &signals, ec);
    Ok(<Types as JsTypes>::value_from_object(
        result_signal.object(ec).ok_or(value_undefined)?,
    ))
}

fn get_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    // Clone the handle out of the object registry so its methods can borrow
    // `ec` mutably; the clone shares all GC-managed state.
    let signal = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned());
    let Some(signal) = signal else {
        return Err(ec.new_type_error("object is not an AbortSignal"));
    };
    let aborted = signal.aborted_value(ec);
    Ok(ec.value_from_bool(aborted))
}

fn get_reason(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    // Clone the handle out of the object registry so its methods can borrow
    // `ec` mutably; the clone shares all GC-managed state.
    let signal = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned());
    let Some(signal) = signal else {
        return Err(ec.new_type_error("object is not an AbortSignal"));
    };
    Ok(signal.reason_value(ec))
}

fn throw_if_aborted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    // Clone the handle out of the object registry so its methods can borrow
    // `ec` mutably; the clone shares all GC-managed state.
    let signal = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned());
    let Some(signal) = signal else {
        return Err(ec.new_type_error("object is not an AbortSignal"));
    };
    if !signal.aborted_value(ec) {
        return Ok(ec.value_undefined());
    }
    Err(signal.reason_value(ec))
}

fn get_onabort(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    // Clone the handle out of the object registry so its methods can borrow
    // `ec` mutably; the clone shares all GC-managed state.
    let signal = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned());
    let Some(signal) = signal else {
        return Err(ec.new_type_error("object is not an AbortSignal"));
    };
    let callback = signal.onabort_value(ec);
    Ok(callback
        .map(|c| c.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

fn set_onabort(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let _signal_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("AbortSignal receiver is not an object"))?;
    let callback = nullable_value(
        args.get(0).unwrap_or(&ec.value_undefined()),
        ec,
        callback_function_value,
    )?;

    let previous = try_with_abort_signal_mut(this, ec, |signal, ec| {
        signal.replace_onabort(callback.clone(), ec)
    })?;

    if let Some(previous) = previous {
        try_with_event_target_mut(this, ec, |target, ec| {
            target.remove_event_listener_entry("abort", &previous, false, ec);
        })?;
    }

    if let Some(callback) = callback {
        try_with_event_target_mut(this, ec, |target, ec| {
            target.add_event_listener(
                target.clone(),
                String::from("abort"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
                ec,
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
        let signal = try_with_abort_signal_ref(&signal_object, ec, |signal, _ec| signal.clone())?;
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
