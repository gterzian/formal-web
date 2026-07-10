use crate::dom::{Event, UIEvent};
type JsValue = <crate::js::Types as JsTypes>::JsValue;

fn with_event_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Event) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(event) = data.downcast_ref::<Event>() {
            return Ok(f(event));
        }
        // Handle Event subclasses that embed Event as a field.
        // Note: this mirrors the hierarchy-walking pattern used in
        // try_with_element_ref for Element/HTMLElement/etc.
        if let Some(ui_event) = data.downcast_ref::<UIEvent>() {
            return Ok(f(&ui_event.event));
        }
    }
    Err(ec.new_type_error("receiver is not an Event"))
}

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

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

pub(crate) fn init_flag(
    init: &JsValue,
    key: &str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<bool, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(init) else {
        return Ok(false);
    };
    let property_key = ec.property_key_from_str(key);
    let value = ExecutionContext::get(ec, object, property_key)?;
    Ok(ec.to_boolean(&value))
}

fn get_type(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let type_value = with_event_ref(this, ec, |event| event.type_value().to_string())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&type_value)))
}

fn get_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let target = with_event_ref(this, ec, |event| event.target_value())?;
    Ok(target
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_current_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let target = with_event_ref(this, ec, |event| event.current_target_value())?;
    Ok(target
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_event_phase(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.event_phase_value())?;
    Ok(ec.value_from_number(val as f64))
}

fn get_bubbles(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.bubbles_value())?;
    Ok(ec.value_from_bool(val))
}

fn get_cancelable(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.cancelable_value())?;
    Ok(ec.value_from_bool(val))
}

fn get_default_prevented(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.default_prevented())?;
    Ok(ec.value_from_bool(val))
}

fn get_cancel_bubble(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.cancel_bubble())?;
    Ok(ec.value_from_bool(val))
}

/// Like [`with_event_ref`] but for mutable access, handling Event subclass
/// data layouts (e.g. UIEvent embeds an `event` field).
fn with_event_mut<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&mut Event) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Event receiver is not an object"))?;
    let result = ec.with_object_any_mut(&obj).and_then(|data| {
        if let Some(event) = data.downcast_mut::<Event>() {
            return Some(f(event));
        }
        if let Some(ui_event) = data.downcast_mut::<UIEvent>() {
            return Some(f(&mut ui_event.event));
        }
        None
    });
    result.ok_or_else(|| ec.new_type_error("receiver is not an Event"))
}

fn set_cancel_bubble(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    with_event_mut(this, ec, |event| event.set_cancel_bubble(value))?;
    Ok(undef)
}

fn get_is_trusted(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.is_trusted())?;
    Ok(ec.value_from_bool(val))
}

fn get_time_stamp(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_event_ref(this, ec, |event| event.time_stamp_value())?;
    Ok(ec.value_from_number(val))
}

fn stop_propagation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    with_event_mut(this, ec, |event| event.stop_propagation())?;
    Ok(undef)
}

fn stop_immediate_propagation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    with_event_mut(this, ec, |event| event.stop_immediate_propagation())?;
    Ok(undef)
}

fn prevent_default(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    with_event_mut(this, ec, |event| event.prevent_default())?;
    Ok(undef)
}
