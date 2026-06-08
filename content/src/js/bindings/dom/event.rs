use boa_engine::{
    Context, JsArgs, JsResult, JsString, JsValue,
    js_string,
};

use crate::js::with_event_mut;
use crate::dom::Event;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for Event {
    const NAME: &'static str = "Event";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let type_ = args
            .get_or_undefined(0)
            .to_string(context)?
            .to_std_string_escaped();
        let init = args.get_or_undefined(1);
        Ok(Event::new(
            type_,
            init_flag(init, js_string!("bubbles"), context)?,
            init_flag(init, js_string!("cancelable"), context)?,
            init_flag(init, js_string!("composed"), context)?,
            false,
            0.0,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition) {
        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
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
            id: "stopPropagation",
            length: 0,
            method: stop_propagation,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "stopImmediatePropagation",
            length: 0,
            method: stop_immediate_propagation,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
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

fn get_type(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        JsValue::from(JsString::from(event.type_value()))
    })
}

fn get_target(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event
            .target_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
}

fn get_current_target(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event
            .current_target_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
}

fn get_event_phase(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.event_phase_value()))
}

fn get_bubbles(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.bubbles_value()))
}

fn get_cancelable(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.cancelable_value()))
}

fn get_default_prevented(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.default_prevented()))
}

fn get_cancel_bubble(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.cancel_bubble()))
}

fn set_cancel_bubble(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.set_cancel_bubble(args.first().is_some_and(JsValue::to_boolean));
        JsValue::undefined()
    })
}

fn get_is_trusted(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.is_trusted()))
}

fn get_time_stamp(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.time_stamp_value()))
}

fn stop_propagation(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.stop_propagation();
        JsValue::undefined()
    })
}

fn stop_immediate_propagation(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.stop_immediate_propagation();
        JsValue::undefined()
    })
}

fn prevent_default(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.prevent_default();
        JsValue::undefined()
    })
}
