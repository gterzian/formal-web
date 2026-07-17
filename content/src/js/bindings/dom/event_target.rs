type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

use crate::dom::EventTarget;
use crate::js::try_with_event_target_mut;
use crate::webidl::{
    callback_interface_type_value, convert_boolean_or_add_event_listener_options, nullable_value,
};

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

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_target = current_event_target_object(this, ec);
    let undefined = ec.value_undefined();
    let type_ = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let options_union =
        convert_boolean_or_add_event_listener_options(args.get(2).unwrap_or(&undefined), ec)?;
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
    let options_union =
        convert_boolean_or_add_event_listener_options(args.get(2).unwrap_or(&undefined), ec)?;
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
