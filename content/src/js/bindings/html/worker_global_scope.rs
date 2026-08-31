use crate::html::{DedicatedWorkerGlobalScope, WindowOrWorkerGlobalScope, WorkerGlobalScope};
use crate::js::Types;
use crate::js::downcast::event_target_from_js_object;
use crate::js::platform_objects::with_worker_global_scope;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::{callback_function_value, nullable_value};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

fn worker_global_scope_domain_from(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<WorkerGlobalScope, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WorkerGlobalScope receiver is not an object"))?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<WorkerGlobalScope>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not a WorkerGlobalScope"))
}

impl WebIdlInterface<Types> for WorkerGlobalScope {
    const NAME: &'static str = "WorkerGlobalScope";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn is_global() -> bool {
        true
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "self",
            getter: get_self,
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
            id: "location",
            getter: get_location,
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
            id: "navigator",
            getter: get_navigator,
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
            id: "importScripts",
            length: 0,
            method: import_scripts,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        define_worker_global_scope_event_handlers(def);
    }
}

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

/// <https://html.spec.whatwg.org/#dom-workerglobalscope-self>
fn get_self(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    Ok(worker_global_scope.self_value(ec))
}

/// <https://html.spec.whatwg.org/#dom-workerglobalscope-location>
fn get_location(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    // The domain method creates the WorkerLocation on first access and caches
    // its JS object on the global scope; the binding returns that cached
    // object.
    worker_global_scope.location_value(ec)?;
    let location_object = with_worker_global_scope(ec, |worker_global_scope, ec| {
        Ok(worker_global_scope.location_object.borrow(ec).clone())
    })?
    .ok_or_else(|| ec.new_type_error("worker global scope has no WorkerLocation object"))?;
    Ok(Types::value_from_object(location_object))
}

/// <https://html.spec.whatwg.org/#dom-workerglobalscope-navigator>
fn get_navigator(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    // The domain method creates the WorkerNavigator on first access and caches
    // its JS object on the global scope; the binding returns that cached
    // object.
    worker_global_scope.navigator_value(ec)?;
    let navigator_object = with_worker_global_scope(ec, |worker_global_scope, ec| {
        Ok(worker_global_scope.navigator_object.borrow(ec).clone())
    })?
    .ok_or_else(|| ec.new_type_error("worker global scope has no WorkerNavigator object"))?;
    Ok(Types::value_from_object(navigator_object))
}

/// <https://html.spec.whatwg.org/#dom-workerglobalscope-importscripts>
fn import_scripts(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let mut urls = Vec::with_capacity(args.len());
    for arg in args {
        urls.push(ec.to_rust_string(arg.clone())?);
    }
    let worker_global_scope = worker_global_scope_domain_from(this, ec)?;
    worker_global_scope.import_scripts(urls, ec)?;
    Ok(ec.value_undefined())
}

/// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-name>
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
    worker_global_scope.close(ec);
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

/// <https://html.spec.whatwg.org/#event-handler-idl-attributes>
fn event_handler_getter(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    event_type: &str,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WorkerGlobalScope receiver is not an object"))?;
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
    enable_inside_port_queue: bool,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WorkerGlobalScope receiver is not an object"))?;
    let callback = nullable_value(
        args.first().unwrap_or(&ec.value_undefined()),
        ec,
        callback_function_value,
    )?;
    let worker_global_scope = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<WorkerGlobalScope>().cloned());
    let Some(worker_global_scope) = worker_global_scope else {
        return Err(ec.new_type_error("receiver is not a WorkerGlobalScope"));
    };
    let previous = worker_global_scope
        .event_target
        .event_handler_value(event_type, ec);
    if let Some(previous) = previous {
        worker_global_scope
            .event_target
            .remove_event_listener_entry(event_type, &previous, false, ec);
    }
    if let Some(callback) = callback.clone() {
        worker_global_scope.event_target.add_event_listener(
            worker_global_scope.event_target.clone(),
            event_type.to_owned(),
            Some(callback),
            false,
            false,
            Some(false),
            None,
            ec,
        );
    }
    worker_global_scope
        .event_target
        .set_event_handler_value(event_type, callback, ec);
    if enable_inside_port_queue {
        worker_global_scope.enable_inside_port_queue(ec);
    }
    Ok(ec.value_undefined())
}

/// The event handler IDL attributes of the WorkerGlobalScope interface
/// (onerror, onlanguagechange, onoffline, ononline, onrejectionhandled,
/// onunhandledrejection).
/// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
fn define_worker_global_scope_event_handlers(def: &mut InterfaceDefinition<Types>) {
    macro_rules! define_handler {
        ($attr:ident, $event:ident) => {
            def.add_attribute(AttributeDef {
                id: stringify!($attr),
                getter: |this, _args, ec| event_handler_getter(this, ec, stringify!($event)),
                setter: Some(|this, args, ec| {
                    event_handler_setter(this, args, ec, stringify!($event), false)
                }),
                static_: false,
                unforgeable: false,
                promise_type: false,
                legacy_lenient_this: false,
                replaceable: false,
                put_forwards: None,
                legacy_lenient_setter: false,
                exposed: None,
            });
        };
    }
    define_handler!(onerror, error);
    define_handler!(onlanguagechange, languagechange);
    define_handler!(onoffline, offline);
    define_handler!(ononline, online);
    define_handler!(onrejectionhandled, rejectionhandled);
    define_handler!(onunhandledrejection, unhandledrejection);
}

/// The WindowOrWorkerGlobalScope mixin members shared with the Window
/// interface (setTimeout, clearTimeout, setInterval, clearInterval,
/// structuredClone).
/// <https://html.spec.whatwg.org/#windoworworkerglobalscope>
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
