use boa_engine::{Context, JsArgs, JsResult, JsString, JsValue, js_string};
use std::marker::PhantomData;

use crate::dom::Event;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for Event {
    const NAME: &'static str = "Event";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { crate::js::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
            let type_ = args
                .get_or_undefined(0)
                .to_string(ctx)?
                .to_std_string_escaped();
            let init = args.get_or_undefined(1);
            Ok(Event::new(
                type_,
                init_flag(init, js_string!("bubbles"), ctx)?,
                init_flag(init, js_string!("cancelable"), ctx)?,
                init_flag(init, js_string!("composed"), ctx)?,
                false,
                0.0,
            ))
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "type",
            getter: get_type,
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

            id: "target",
            getter: get_target,
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

            id: "currentTarget",
            getter: get_current_target,
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

            id: "eventPhase",
            getter: get_event_phase,
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

            id: "bubbles",
            getter: get_bubbles,
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

            id: "cancelable",
            getter: get_cancelable,
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

            id: "defaultPrevented",
            getter: get_default_prevented,
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

            id: "cancelBubble",
            getter: get_cancel_bubble,
            setter: Some(set_cancel_bubble),
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

            id: "isTrusted",
            getter: get_is_trusted,
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

            id: "timeStamp",
            getter: get_time_stamp,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });

        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "stopPropagation",
            length: 0,
            method: stop_propagation,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "stopImmediatePropagation",
            length: 0,
            method: stop_immediate_propagation,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "preventDefault",
            length: 0,
            method: prevent_default,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

pub(crate) fn init_flag(init: &JsValue, key: JsString, context: &mut Context) -> JsResult<bool> {
    let Some(object) = init.as_object() else {
        return Ok(false);
    };
    Ok(object.get(key, context)?.to_boolean())
}

fn get_type(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(event.type_value())))
}

fn get_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(event
        .target_value()
        .clone()
        .map(JsValue::from)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_current_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(event
        .current_target_value()
        .clone()
        .map(JsValue::from)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_event_phase(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_number(event.event_phase_value() as f64))
}

fn get_bubbles(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_bool(event.bubbles_value()))
}

fn get_cancelable(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_bool(event.cancelable_value()))
}

fn get_default_prevented(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_bool(event.default_prevented()))
}

fn get_cancel_bubble(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_bool(event.cancel_bubble()))
}

fn set_cancel_bubble(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let mut event = obj
        .downcast_mut::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    let value = args.first().map_or(false, |v| v.to_boolean());
    event.set_cancel_bubble(value);
    Ok(ec.value_undefined())
}

fn get_is_trusted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_bool(event.is_trusted()))
}

fn get_time_stamp(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let event = obj
        .downcast_ref::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    Ok(ec.value_from_number(event.time_stamp_value()))
}

fn stop_propagation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let mut event = obj
        .downcast_mut::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    event.stop_propagation();
    Ok(ec.value_undefined())
}

fn stop_immediate_propagation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let mut event = obj
        .downcast_mut::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    event.stop_immediate_propagation();
    Ok(ec.value_undefined())
}

fn prevent_default(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let mut event = obj
        .downcast_mut::<Event>()
        .ok_or_else(|| ec.new_type_error("receiver is not an Event"))?;
    event.prevent_default();
    Ok(ec.value_undefined())
}
