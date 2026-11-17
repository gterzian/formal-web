use crate::dom::Event;
use crate::js::bindings::initialization::init_flag;
use crate::js::downcast::event_from_js_object;
type JsValue = <crate::js::Types as JsTypes>::JsValue;

fn with_event_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Event, &mut dyn ExecutionContext<crate::js::Types>) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably while the platform object is accessed. The clone shares all
    // GC-managed state with the registered platform object.
    let event = event_from_js_object(ec, &obj);
    let Some(event) = event else {
        return Err(ec.new_type_error("receiver is not an Event"));
    };
    Ok(f(&event, ec))
}

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for Event {
    const NAME: &'static str = "Event";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let undefined = ec.value_undefined();
        let type_ = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
        let init = args.get(1).cloned().unwrap_or(ec.value_undefined());
        Ok(Event::new(
            type_,
            init_flag(&init, "bubbles", ec)?,
            init_flag(&init, "cancelable", ec)?,
            init_flag(&init, "composed", ec)?,
            false,
            0.0,
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
        });

        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            id: "stopPropagation",
            length: 0,
            method: stop_propagation,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "stopImmediatePropagation",
            length: 0,
            method: stop_immediate_propagation,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "preventDefault",
            length: 0,
            method: prevent_default,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn get_type(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let type_value = with_event_ref(this, ec, |event, _ec| event.type_value().to_string())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&type_value)))
}

fn get_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let target = with_event_ref(this, ec, |event, ec| event.target_value(ec))?;
    Ok(target
        .and_then(|et| et.reflector.clone())
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_current_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let target = with_event_ref(this, ec, |event, ec| event.current_target_value(ec))?;
    Ok(target
        .and_then(|et| et.reflector.clone())
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_event_phase(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.event_phase_value(ec))?;
    Ok(ec.value_from_number(val as f64))
}

fn get_bubbles(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.bubbles_value(ec))?;
    Ok(ec.value_from_bool(val))
}

fn get_cancelable(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.cancelable_value(ec))?;
    Ok(ec.value_from_bool(val))
}

fn get_default_prevented(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.default_prevented(ec))?;
    Ok(ec.value_from_bool(val))
}

fn get_cancel_bubble(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.cancel_bubble(ec))?;
    Ok(ec.value_from_bool(val))
}

/// Like [`with_event_ref`] but for mutable access; the receiver downcasts to
/// its embedded `Event` via [`event_from_js_object`].
fn with_event_mut<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&mut Event, &mut dyn ExecutionContext<crate::js::Types>) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably while the platform object is accessed. The clone shares all
    // GC-managed state with the registered platform object.
    let event = event_from_js_object(ec, &obj);
    let Some(mut event) = event else {
        return Err(ec.new_type_error("receiver is not an Event"));
    };
    Ok(f(&mut event, ec))
}

fn set_cancel_bubble(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let value = args.first().is_some_and(|v| ec.to_boolean(v));
    with_event_mut(this, ec, |event, ec| event.set_cancel_bubble(value, ec))?;
    Ok(undef)
}

fn get_is_trusted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.is_trusted(ec))?;
    Ok(ec.value_from_bool(val))
}

fn get_time_stamp(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event, ec| event.time_stamp_value(ec))?;
    Ok(ec.value_from_number(val))
}

fn stop_propagation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    with_event_mut(this, ec, |event, ec| event.stop_propagation(ec))?;
    Ok(undef)
}

fn stop_immediate_propagation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    with_event_mut(this, ec, |event, ec| event.stop_immediate_propagation(ec))?;
    Ok(undef)
}

fn prevent_default(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    with_event_mut(this, ec, |event, ec| event.prevent_default(ec))?;
    Ok(undef)
}
