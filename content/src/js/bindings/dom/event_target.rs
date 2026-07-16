type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

use crate::dom::{AbortSignal, EventTarget, dispatch};
use crate::js::try_with_event_target_mut;
use crate::webidl::{callback_interface_type_value, nullable_value};

use js_engine::{Completion, ExecutionContext, JsTypes};

#[derive(Clone)]
pub(crate) struct AddEventListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
    pub signal: Option<AbortSignal>,
}

use crate::webidl::bindings::{
    InterfaceDefinition, OperationDef, WebIdlInterface,
};

impl WebIdlInterface<crate::js::Types> for EventTarget {
    const NAME: &'static str = "EventTarget";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        Ok(EventTarget::default())
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_operation(OperationDef {
            id: "addEventListener",
            length: 3,
            method: add_event_listener,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "removeEventListener",
            length: 3,
            method: remove_event_listener,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "dispatchEvent",
            length: 1,
            method: dispatch_event,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_target = current_event_target_object(this, ec);
    let undefined = ec.value_undefined();
    let type_ = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let options = flatten_more(args.get(2).unwrap_or(&undefined), ec)?;
    let callback = nullable_value(
        args.get(1).unwrap_or(&undefined),
        ec,
        callback_interface_type_value,
    )?;
    let receiver = crate::js::Types::value_from_object(event_target.clone());

    try_with_event_target_mut(&receiver, ec, |target| {
        target.add_event_listener(
            target.clone(),
            type_,
            callback,
            options.capture,
            options.once,
            options.passive,
            options.signal,
        );
    })?;

    Ok(ec.value_undefined())
}

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_target = current_event_target_object(this, ec);
    let undefined = ec.value_undefined();
    let type_ = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let Some(callback) = nullable_value(
        args.get(1).unwrap_or(&undefined),
        ec,
        callback_interface_type_value,
    )?
    else {
        return Ok(ec.value_undefined());
    };
    let capture = flatten(args.get(2).unwrap_or(&undefined), ec)?;
    let receiver = crate::js::Types::value_from_object(event_target);

    try_with_event_target_mut(&receiver, ec, |target| {
        target.remove_event_listener_entry(&type_, &callback, capture);
    })?;

    Ok(ec.value_undefined())
}

fn dispatch_event(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_obj = match args
        .first()
        .and_then(|v| <crate::js::Types as JsTypes>::value_as_object(v))
    {
        Some(obj) => obj,
        None => return Err(ec.new_type_error("dispatchEvent requires an Event")),
    };
    let target_object = current_event_target_object(this, ec);
    let target_value = <crate::js::Types as JsTypes>::value_from_object(target_object.clone());
    let target = crate::js::try_with_event_target_mut(&target_value, ec, |et| et.clone())
        .map_err(|error| {
            log::error!("dispatchEvent: failed to extract EventTarget: {error:?}");
            error
        })?;

    let canceled = dispatch(ec, &target, &event_obj, false)?;
    Ok(ec.value_from_bool(!canceled))
}

fn current_event_target_object(
    this: &JsValue,
    ec: &dyn ExecutionContext<crate::js::Types>,
) -> JsObject {
    if let Some(object) = <crate::js::Types as JsTypes>::value_as_object(this) {
        return object.clone();
    }

    ec.global_object()
}

pub(crate) fn flatten(
    options: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<bool, crate::js::Types> {
    if let Some(boolean) = crate::js::Types::value_as_bool(options) {
        return Ok(boolean);
    }
    let Some(object) = crate::js::Types::value_as_object(options) else {
        return Ok(false);
    };
    let capture_key = ec.property_key_from_str("capture");
    let capture_val = ExecutionContext::get(ec, object, capture_key)?;
    Ok(ec.to_boolean(&capture_val))
}

pub(crate) fn flatten_more(
    options: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<AddEventListenerOptions, crate::js::Types> {
    let capture = flatten(options, ec)?;
    let Some(object) = <crate::js::Types as JsTypes>::value_as_object(options) else {
        return Ok(AddEventListenerOptions {
            capture,
            once: false,
            passive: None,
            signal: None,
        });
    };
    let once_key = ec.property_key_from_str("once");
    let once_val = ExecutionContext::get(ec, object.clone(), once_key)?;
    let once = ec.to_boolean(&once_val);
    let passive = {
        let passive_key = ec.property_key_from_str("passive");
        let has_passive = ExecutionContext::has_property(ec, object.clone(), passive_key.clone())?;
        if !has_passive {
            None
        } else {
            let value = ExecutionContext::get(ec, object.clone(), passive_key)?;
            Some(ec.to_boolean(&value))
        }
    };
    let signal = {
        let signal_key = ec.property_key_from_str("signal");
        let has_signal = ExecutionContext::has_property(ec, object.clone(), signal_key.clone())?;
        if !has_signal {
            None
        } else {
            let value = ExecutionContext::get(ec, object.clone(), signal_key)?;
            let signal_obj =
                <crate::js::Types as JsTypes>::value_as_object(&value).ok_or_else(|| {
                    ec.new_type_error("addEventListener signal must be an AbortSignal")
                })?;
            let signal = ec
                .with_object_any(&signal_obj)
                .and_then(|d| d.downcast_ref::<AbortSignal>())
                .map(|s| s.clone())
                .ok_or_else(|| {
                    ec.new_type_error("addEventListener signal must be an AbortSignal")
                })?;
            Some(signal)
        }
    };
    Ok(AddEventListenerOptions {
        capture,
        once,
        passive,
        signal,
    })
}
