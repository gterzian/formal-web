use crate::html::{DedicatedWorkerGlobalScope, WindowOrWorkerGlobalScope};
use crate::js::Types;
use crate::js::bindings::html::worker_global_scope::{
    event_handler_getter, event_handler_setter, worker_global_scope_domain_from,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for DedicatedWorkerGlobalScope {
    const NAME: &'static str = "DedicatedWorkerGlobalScope";

    fn parent_name() -> Option<&'static str> {
        Some("WorkerGlobalScope")
    }

    fn is_global() -> bool {
        true
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "name",
            getter: get_name,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: true,
            put_forwards: None,
            legacy_lenient_setter: false,
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
        def.add_operation(OperationDef {
            id: "close",
            length: 0,
            method: close,
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
        // <https://html.spec.whatwg.org/#windoworworkerglobalscope>
        define_window_or_worker_global_scope_members(def);
    }
}

fn get_name(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&worker_global_scope.name_value())))
}

/// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-postmessage>
/// The two overloads: `postMessage(message, transfer)` (a sequence of
/// transferable objects) and `postMessage(message, options)` (a
/// StructuredSerializeOptions dictionary).
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
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    worker_global_scope.post_message(message, transfer, ec)?;
    Ok(ec.value_undefined())
}

/// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-close>
fn close(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    worker_global_scope.close();
    Ok(ec.value_undefined())
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
    // The first time a MessagePort object's onmessage IDL attribute is set,
    // the port's port message queue must be enabled.  For the worker's inside
    // port, the message event target is the worker global scope, so setting
    // onmessage on the global scope enables the inside port's queue.
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

fn define_window_or_worker_global_scope_members(def: &mut InterfaceDefinition<Types>) {
    def.add_operation(OperationDef {
        id: "setTimeout",
        length: 1,
        method: set_timeout_method,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "clearTimeout",
        length: 1,
        method: clear_timeout_method,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "setInterval",
        length: 1,
        method: set_interval_method,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "clearInterval",
        length: 1,
        method: clear_interval_method,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "structuredClone",
        length: 1,
        method: structured_clone_method,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
}

/// <https://html.spec.whatwg.org/#dom-settimeout>
fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let handler = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let timeout = args.get(1).cloned().unwrap_or(undefined);
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    let timer_id = worker_global_scope.set_timeout(&handler, &timeout, Vec::new(), ec)?;
    Ok(ec.value_from_number(f64::from(timer_id)))
}

/// <https://html.spec.whatwg.org/#dom-cleartimeout>
fn clear_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let timer_id = ec.to_number(args.first().cloned().unwrap_or(undefined))?;
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    worker_global_scope.clear_timeout(timer_id as u32, ec);
    Ok(ec.value_undefined())
}

/// <https://html.spec.whatwg.org/#dom-setinterval>
fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let handler = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let timeout = args.get(1).cloned().unwrap_or(undefined);
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    let timer_id = worker_global_scope.set_interval(&handler, &timeout, Vec::new(), ec)?;
    Ok(ec.value_from_number(f64::from(timer_id)))
}

/// <https://html.spec.whatwg.org/#dom-clearinterval>
fn clear_interval_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let timer_id = ec.to_number(args.first().cloned().unwrap_or(undefined))?;
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    worker_global_scope.clear_interval(timer_id as u32, ec);
    Ok(ec.value_undefined())
}

/// <https://html.spec.whatwg.org/#dom-structuredclone>
fn structured_clone_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let value = args.first().cloned().unwrap_or_else(|| undefined.clone());
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    let result = worker_global_scope.structured_clone(value, None, ec)?;
    Ok(result)
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
/// values per Web IDL.
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
