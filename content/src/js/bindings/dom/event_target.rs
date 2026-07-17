type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

use crate::dom::{BooleanOrAddEventListenerOptions, EventTarget};
use crate::js::try_with_event_target_mut;
use crate::webidl::{callback_interface_type_value, dictionary, nullable_value};

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::webidl::bindings::{InterfaceDefinition, OperationDef, WebIdlInterface};

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

/// <https://webidl.spec.whatwg.org/#js-union>
/// Convert a JS value to the Web IDL union `(boolean or AddEventListenerOptions)`.
fn convert_options_union(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<BooleanOrAddEventListenerOptions, crate::js::Types> {

    // Step 12: If V is a Boolean, then: if types includes boolean, convert.
    if let Some(b) = <crate::js::Types as JsTypes>::value_as_bool(value) {
        return Ok(BooleanOrAddEventListenerOptions::Boolean(b));
    }

    // Step 4.1: If V is null or undefined and types includes dictionary, convert.
    // Step 11.4: If V is an Object and types includes dictionary, convert.
    let access = dictionary::convert_js_to_dictionary::<crate::js::Types>(value, ec)?;

    // Step 4: For each dictionary member in AddEventListenerOptions
    let mut dict = crate::dom::AddEventListenerOptions::default();

    // Member: capture (boolean, default false) — inherited from EventListenerOptions
    if let Some(val) = access.get_member("capture", ec)? {
        dict.capture = ec.to_boolean(&val);
    }

    // Member: once (boolean, default false)
    if let Some(val) = access.get_member("once", ec)? {
        dict.once = ec.to_boolean(&val);
    }

    // Member: passive (boolean, no default — stays None if absent)
    if let Some(val) = access.get_member("passive", ec)? {
        dict.passive = Some(ec.to_boolean(&val));
    }

    // Member: signal (AbortSignal, no default — stays None if absent)
    if let Some(val) = access.get_member("signal", ec)? {
        let signal_obj = <crate::js::Types as JsTypes>::value_as_object(&val)
            .ok_or_else(|| ec.new_type_error("addEventListener signal must be an AbortSignal"))?;
        dict.signal = Some(
            ec
                .with_object_any(&signal_obj)
                .and_then(|d| d.downcast_ref::<crate::dom::AbortSignal>().cloned())
                .ok_or_else(|| ec.new_type_error("addEventListener signal must be an AbortSignal"))?
        );
    }

    Ok(BooleanOrAddEventListenerOptions::Dict(dict))
}

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_target = current_event_target_object(this, ec);
    let undefined = ec.value_undefined();
    let type_ = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let options_union = convert_options_union(args.get(2).unwrap_or(&undefined), ec)?;
    let options = crate::dom::flatten_more(options_union);
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
    let options_union = convert_options_union(args.get(2).unwrap_or(&undefined), ec)?;
    let capture = crate::dom::flatten(&options_union);
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
    let event: crate::dom::Event = ec
        .with_object_any(&event_obj)
        .and_then(|data| data.downcast_ref::<crate::dom::Event>().cloned())
        .ok_or_else(|| ec.new_type_error("dispatchEvent: event_obj is not an Event"))?;

    let target_object = current_event_target_object(this, ec);
    let path = crate::js::platform_objects::build_path_from_target_js_object(&target_object, ec);

    let target_value = <crate::js::Types as JsTypes>::value_from_object(target_object);
    let target = crate::js::try_with_event_target_mut(&target_value, ec, |target| target.clone())?;
    let canceled = target.dispatch_event(&event, &path, ec)?;
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
