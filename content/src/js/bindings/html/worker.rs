use crate::html::{Worker, WorkerType};
use crate::js::Types;
use crate::js::downcast::event_target_from_js_object;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::{callback_function_value, nullable_value};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

fn with_worker_ref(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&Worker, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Worker receiver is not an object"))?;
    let worker = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<Worker>().cloned());
    let Some(worker) = worker else {
        return Err(ec.new_type_error("receiver is not a Worker"));
    };
    f(&worker, ec)
}

impl WebIdlInterface<Types> for Worker {
    const NAME: &'static str = "Worker";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // The constructor steps (including the trusted-type and URL parsing
        // steps) run in `Worker::constructor`; this binding only performs the
        // Web IDL argument conversion (scriptURL as USVString, and the
        // WorkerOptions dictionary).
        let undefined = ec.value_undefined();
        let script_url = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
        let (name, worker_type) = parse_worker_options(args.get(1), ec);
        Worker::constructor(&script_url, name, WorkerType::from_idl(&worker_type), ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_operation(OperationDef {
            id: "terminate",
            length: 0,
            method: terminate,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "postMessage",
            length: 1,
            method: post_message,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "onmessage",
            getter: get_onmessage,
            setter: Some(set_onmessage),
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
            id: "onmessageerror",
            getter: get_onmessageerror,
            setter: Some(set_onmessageerror),
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
            id: "onerror",
            getter: get_onerror,
            setter: Some(set_onerror),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
    }
}

/// <https://html.spec.whatwg.org/#dom-worker-postmessage>
/// The two overloads: `postMessage(message, transfer)` (a sequence of
/// transferable objects) and `postMessage(message, options)` (a
/// StructuredSerializeOptions dictionary).  Web IDL overload resolution
/// picks the sequence form when the second argument is an array.
fn post_message(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let message = args.first().cloned().unwrap_or(undefined);
    let is_sequence_form = match args.get(1).and_then(Types::value_as_object) {
        Some(second_object) => ec.is_array(&Types::value_from_object(second_object))?,
        None => false,
    };
    let transfer = if is_sequence_form {
        parse_transfer_sequence(args.get(1), ec)?
    } else {
        options_dict_transfer(args.get(1), ec)?
    };
    with_worker_ref(this, ec, |worker, ec| {
        worker.post_message(message, transfer, ec)?;
        Ok(ec.value_undefined())
    })
}

/// <https://html.spec.whatwg.org/#dom-worker-terminate>
fn terminate(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_worker_ref(this, ec, |worker, ec| {
        worker.terminate();
        Ok(ec.value_undefined())
    })
}

fn get_onmessage(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    event_handler_getter(this, ec, "message")
}

fn set_onmessage(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // <https://html.spec.whatwg.org/#messageeventtarget>
    // The first time a Worker object's onmessage IDL attribute is set, the
    // outside port's port message queue must be enabled, as if the start()
    // method had been called.
    event_handler_setter(this, args, ec, "message", true)
}

fn get_onmessageerror(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    event_handler_getter(this, ec, "messageerror")
}

fn set_onmessageerror(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    event_handler_setter(this, args, ec, "messageerror", false)
}

fn get_onerror(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    event_handler_getter(this, ec, "error")
}

fn set_onerror(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    event_handler_setter(this, args, ec, "error", false)
}

/// <https://html.spec.whatwg.org/#event-handler-idl-attributes>
fn event_handler_getter(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    event_type: &str,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Worker receiver is not an object"))?;
    let Some(event_target) = event_target_from_js_object(ec, &object) else {
        return Ok(ec.value_null());
    };
    let callback = crate::html::event_handler::event_handler_idl_attribute_getter(
        &event_target,
        event_type,
        ec,
    );
    Ok(callback
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

/// <https://html.spec.whatwg.org/#event-handler-idl-attributes>
fn event_handler_setter(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
    event_type: &str,
    enable_outside_port_queue: bool,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Worker receiver is not an object"))?;
    let callback = nullable_value(
        args.first().unwrap_or(&ec.value_undefined()),
        ec,
        callback_function_value,
    )?;
    let worker = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<Worker>().cloned());
    let Some(worker) = worker else {
        return Err(ec.new_type_error("receiver is not a Worker"));
    };
    let previous = worker.event_target.event_handler_value(event_type, ec);
    if let Some(previous) = previous {
        worker
            .event_target
            .remove_event_listener_entry(event_type, &previous, false, ec);
    }
    if let Some(callback) = callback.clone() {
        worker.event_target.add_event_listener(
            worker.event_target.clone(),
            event_type.to_owned(),
            Some(callback),
            false,
            false,
            Some(false),
            None,
            ec,
        );
    }
    worker
        .event_target
        .set_event_handler_value(event_type, callback, ec);
    if enable_outside_port_queue {
        // <https://html.spec.whatwg.org/#messageeventtarget>
        // The outside port's message queue is enabled as if start() had been
        // called, so messages the worker posts before run-a-worker enables the
        // queue can be delivered.
        worker.enable_message_delivery(ec);
    }
    Ok(ec.value_undefined())
}

/// Read the `transfer` member (a `sequence<object>`) from the
/// StructuredSerializeOptions dictionary.
fn options_dict_transfer(
    dict: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<JsValue>, Types> {
    let Some(dict) = dict else {
        return Ok(Vec::new());
    };
    let Some(object) = Types::value_as_object(dict) else {
        return Ok(Vec::new());
    };
    let key_pk = ec.property_key_from_str("transfer");
    let value = ExecutionContext::get(ec, object, key_pk)?;
    parse_transfer_sequence(Some(&value), ec)
}

/// Convert the `transfer` argument (a `sequence<object>`) to a list of
/// values per Web IDL: an absent or `undefined` value converts to the
/// default empty sequence, a non-object or non-iterable value throws a
/// TypeError, and each iterated element must be an object.
/// <https://webidl.spec.whatwg.org/#es-sequence>
fn parse_transfer_sequence(
    transfer_value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<JsValue>, Types> {
    let Some(transfer_value) = transfer_value else {
        return Ok(Vec::new());
    };
    if Types::value_is_undefined(transfer_value) {
        return Ok(Vec::new());
    }
    if Types::value_as_object(transfer_value).is_none() {
        return Err(ec.new_type_error("transfer is not an object"));
    }
    let mut iterator =
        ec.get_iterator(transfer_value.clone(), js_engine::IteratorKind::Sync, None)?;
    let mut transfer = Vec::new();
    loop {
        let next = ec.iterator_step_value(&mut iterator)?;
        let Some(next) = next else {
            break;
        };
        if Types::value_as_object(&next).is_none() {
            return Err(ec.new_type_error("transfer element is not an object"));
        }
        transfer.push(next);
    }
    Ok(transfer)
}

/// Parse the WorkerOptions dictionary: `name` (default "") and `type`
/// (default "classic").  The `credentials` member is only used for module
/// workers and is not parsed.
/// <https://html.spec.whatwg.org/#dictdef-workeroptions>
fn parse_worker_options(
    options: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> (String, String) {
    let Some(options) = options else {
        return (String::new(), String::from("classic"));
    };
    let Some(object) = Types::value_as_object(options) else {
        return (String::new(), String::from("classic"));
    };
    let name_key = ec.property_key_from_str("name");
    let name = ExecutionContext::get(ec, object.clone(), name_key)
        .ok()
        .and_then(|value| {
            if Types::value_is_undefined(&value) {
                None
            } else {
                ec.to_rust_string(value).ok()
            }
        })
        .unwrap_or_default();
    let type_key = ec.property_key_from_str("type");
    let worker_type = ExecutionContext::get(ec, object, type_key)
        .ok()
        .and_then(|value| {
            if Types::value_is_undefined(&value) {
                None
            } else {
                ec.to_rust_string(value).ok()
            }
        })
        .unwrap_or_else(|| String::from("classic"));
    (name, worker_type)
}
