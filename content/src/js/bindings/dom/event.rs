use std::marker::PhantomData;
use boa_engine::{js_string, Context, JsArgs, JsResult, JsString, JsValue};

use crate::dom::Event;
use crate::js::with_event_mut;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

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
        let type_ = args.get_or_undefined(0).to_string(ctx)?.to_std_string_escaped();
        let init = args.get_or_undefined(1);
        Ok(Event::new(type_,
            init_flag(init, js_string!("bubbles"), ctx)?,
            init_flag(init, js_string!("cancelable"), ctx)?,
            init_flag(init, js_string!("composed"), ctx)?, false, 0.0))
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

fn get_type(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        JsValue::from(JsString::from(event.type_value()))
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_target(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event
            .target_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_current_target(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event
            .current_target_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_event_phase(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.event_phase_value()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_bubbles(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.bubbles_value()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_cancelable(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.cancelable_value()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_default_prevented(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.default_prevented()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_cancel_bubble(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.cancel_bubble()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_cancel_bubble(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.set_cancel_bubble(args.first().is_some_and(JsValue::to_boolean));
        JsValue::undefined()
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_is_trusted(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.is_trusted()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_time_stamp(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.time_stamp_value()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn stop_propagation(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.stop_propagation();
        JsValue::undefined()
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn stop_immediate_propagation(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.stop_immediate_propagation();
        JsValue::undefined()
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn prevent_default(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.prevent_default();
        JsValue::undefined()
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
