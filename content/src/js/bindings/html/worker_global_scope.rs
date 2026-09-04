use crate::html::WorkerGlobalScope;
use crate::js::Types;
use crate::js::downcast::event_target_from_js_object;
use crate::js::platform_objects::with_worker_global_scope;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::{callback_function_value, nullable_value};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

pub(crate) fn worker_global_scope_domain_from(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<WorkerGlobalScope, Types> {
    // The worker global scope's members are global members: calling one bare
    // (postMessage(...)) or accessing it on the global object resolves the
    // realm's worker global scope.  The Web IDL receiver is then undefined on
    // some backends (Boa passes undefined to a bare-call this) or the global
    // object on others (V8), so mirror `resolve_window`: resolve the receiver
    // to the realm's global object when it is not an object.
    let object = match Types::value_as_object(this) {
        Some(object) => object,
        None => ec.global_object(),
    };
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

/// <https://html.spec.whatwg.org/#workernavigator>
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

pub(crate) fn event_handler_getter(
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
pub(crate) fn event_handler_setter(
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
        // <https://html.spec.whatwg.org/#messageeventtarget>
        // The first time a MessagePort object's onmessage IDL attribute is
        // set, the port's port message queue must be enabled.  For the
        // worker's implicit port (bypassed; see workers/dedicated_worker_agent.rs),
        // the message event target is the worker global scope, so setting
        // onmessage on the global scope enables the worker's inbound message
        // queue.
        worker_global_scope.enable_inbound_messages();
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
