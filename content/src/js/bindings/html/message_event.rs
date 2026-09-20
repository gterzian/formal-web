use crate::html::message_event::{MessageEvent, MessageEventInit};
use crate::js::bindings::initialization::init_flag;
use crate::js::with_cloned_platform_mut;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <crate::js::Types as JsTypes>::JsValue;

fn with_message_event_ref(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(
        &MessageEvent,
        &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("MessageEvent receiver is not an object"))?;
    let message_event = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<MessageEvent>().cloned());
    let Some(message_event) = message_event else {
        return Err(ec.new_type_error("receiver is not a MessageEvent"));
    };
    f(&message_event, ec)
}

/// Read `init["key"]` as a string with a default, per the Web IDL dictionary
/// conversion for the `origin` and `lastEventId` members.
fn init_string(
    init: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    key: &str,
    default: &str,
) -> Completion<String, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(init) else {
        return Ok(default.to_owned());
    };
    let key_pk = ec.property_key_from_str(key);
    let value = ExecutionContext::get(ec, object, key_pk)?;
    if crate::js::Types::value_is_undefined(&value) {
        return Ok(default.to_owned());
    }
    ec.to_rust_string(value)
}

/// Read `init["data"]`: the `data` member is `any`, so the raw value is used.
fn init_data(
    init: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(init) else {
        return Ok(ec.value_null());
    };
    let key_pk = ec.property_key_from_str("data");
    let value = ExecutionContext::get(ec, object, key_pk)?;
    if crate::js::Types::value_is_undefined(&value) {
        return Ok(ec.value_null());
    }
    Ok(value)
}

/// Read `init["source"]`: the `source` member is `MessageEventSource?`; the
/// value is used as-is when it is an object.
fn init_source(
    init: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<<crate::js::Types as JsTypes>::JsObject>, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(init) else {
        return Ok(None);
    };
    let key_pk = ec.property_key_from_str("source");
    let value = ExecutionContext::get(ec, object, key_pk)?;
    Ok(crate::js::Types::value_as_object(&value))
}

/// Read `init["ports"]`: the `ports` member is a `sequence<MessagePort>`; each
/// element is an object.
fn init_ports(
    init: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<<crate::js::Types as JsTypes>::JsObject>, crate::js::Types> {
    let Some(object) = crate::js::Types::value_as_object(init) else {
        return Ok(Vec::new());
    };
    let key_pk = ec.property_key_from_str("ports");
    let value = ExecutionContext::get(ec, object, key_pk)?;
    let Some(ports_object) = crate::js::Types::value_as_object(&value) else {
        return Ok(Vec::new());
    };
    let length_key = ec.property_key_from_str("length");
    let length_val = ExecutionContext::get(ec, ports_object.clone(), length_key)?;
    let length = ec.to_length(length_val)?;
    let mut ports = Vec::with_capacity(length as usize);
    for i in 0..length {
        let index_key = ec.property_key_from_str(&i.to_string());
        let item = ExecutionContext::get(ec, ports_object.clone(), index_key)?;
        if let Some(port) = crate::js::Types::value_as_object(&item) {
            ports.push(port);
        }
    }
    Ok(ports)
}

impl WebIdlInterface<crate::js::Types> for MessageEvent {
    const NAME: &'static str = "MessageEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let undefined = ec.value_undefined();
        let type_ = ec.to_rust_string(args.first().cloned().unwrap_or(undefined.clone()))?;
        let init = args.get(1).cloned().unwrap_or(undefined);
        Ok(MessageEvent::new(
            type_,
            MessageEventInit {
                bubbles: init_flag(&init, "bubbles", ec)?,
                cancelable: init_flag(&init, "cancelable", ec)?,
                composed: init_flag(&init, "composed", ec)?,
                data: init_data(&init, ec)?,
                origin: init_string(&init, ec, "origin", "")?,
                last_event_id: init_string(&init, ec, "lastEventId", "")?,
                source: init_source(&init, ec)?,
                ports: init_ports(&init, ec)?,
            },
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            id: "data",
            getter: get_data,
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
            id: "origin",
            getter: get_origin,
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
            id: "lastEventId",
            getter: get_last_event_id,
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
            id: "source",
            getter: get_source,
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
            id: "ports",
            getter: get_ports,
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
        def.add_operation(OperationDef {
            id: "initMessageEvent",
            length: 8,
            method: init_message_event_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn get_data(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    with_message_event_ref(this, ec, |message_event, ec| {
        Ok(message_event
            .data_value(ec)
            .unwrap_or_else(|| ec.value_null()))
    })
}

fn get_origin(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    with_message_event_ref(this, ec, |message_event, ec| {
        let origin = message_event.origin_value(ec);
        Ok(ec.value_from_string(ec.js_string_from_str(&origin)))
    })
}

fn get_last_event_id(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    with_message_event_ref(this, ec, |message_event, ec| {
        let last_event_id = message_event.last_event_id_value(ec);
        Ok(ec.value_from_string(ec.js_string_from_str(&last_event_id)))
    })
}

fn get_source(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    with_message_event_ref(this, ec, |message_event, ec| {
        Ok(message_event
            .source_value(ec)
            .map(crate::js::Types::value_from_object)
            .unwrap_or_else(|| ec.value_null()))
    })
}

fn get_ports(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    with_message_event_ref(this, ec, |message_event, ec| {
        Ok(crate::js::Types::value_from_object(
            message_event.ports_value_frozen(ec)?,
        ))
    })
}

/// <https://html.spec.whatwg.org/#dom-messageevent-initmessageevent>
fn init_message_event_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    // <https://webidl.spec.whatwg.org/#dfn-overload-resolution>
    // The `type` argument is required; calling with no arguments throws.
    if args.is_empty() {
        return Err(ec.new_type_error(
            "Failed to execute 'initMessageEvent': 1 argument required, but only 0 present.",
        ));
    }
    let type_ = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined.clone()))?;
    let bubbles = args.get(1).map(|v| ec.to_boolean(v)).unwrap_or(false);
    let cancelable = args.get(2).map(|v| ec.to_boolean(v)).unwrap_or(false);
    let data = args.get(3).cloned().unwrap_or_else(|| ec.value_null());
    let origin = args
        .get(4)
        .map(|v| ec.to_rust_string(v.clone()))
        .transpose()?
        .unwrap_or_default();
    let last_event_id = args
        .get(5)
        .map(|v| ec.to_rust_string(v.clone()))
        .transpose()?
        .unwrap_or_default();
    let source = args
        .get(6)
        .and_then(|v| crate::js::Types::value_as_object(v));
    let ports = args
        .get(7)
        .map(|v| {
            let object = crate::js::Types::value_as_object(v);
            let mut ports = Vec::new();
            if let Some(object) = object {
                let length_key = ec.property_key_from_str("length");
                if let Ok(length_val) = ExecutionContext::get(ec, object.clone(), length_key)
                    && let Ok(length) = ec.to_length(length_val)
                {
                    for i in 0..length {
                        let index_key = ec.property_key_from_str(&i.to_string());
                        if let Ok(item) = ExecutionContext::get(ec, object.clone(), index_key)
                            && let Some(port) = crate::js::Types::value_as_object(&item)
                        {
                            ports.push(port);
                        }
                    }
                }
            }
            ports
        })
        .unwrap_or_default();

    let object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("MessageEvent receiver is not an object"))?;
    // The platform data is cloned out so the mutation can call `ec` without
    // holding a borrow into it, then written back; the shared cells carry the
    // attribute updates.
    let result = with_cloned_platform_mut::<MessageEvent, _>(&object, ec, |message_event, ec| {
        // <https://dom.spec.whatwg.org/#dom-event-initevent>
        // Note: The Event fields are re-initialized directly (initEvent
        // is not exposed as a separate binding step).
        message_event.event.type_ = type_;
        *message_event.event.bubbles.borrow_mut(ec) = bubbles;
        *message_event.event.cancelable.borrow_mut(ec) = cancelable;
        message_event.data.set(Some(data), ec);
        message_event.origin.set(origin, ec);
        message_event.last_event_id.set(last_event_id, ec);
        message_event.source.set(source, ec);
        message_event.ports.set(ports, ec);
        // Invalidate the cached frozen array so the next `ports`
        // getter builds one from the new ports sequence.
        message_event.ports_array.set(None, ec);
        Ok(ec.value_undefined())
    });
    result.unwrap_or_else(|| Err(ec.new_type_error("receiver is not a MessageEvent")))
}
