use crate::html::WorkerLocation;
use crate::js::Types;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

fn with_worker_location_ref(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&WorkerLocation, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WorkerLocation receiver is not an object"))?;
    let location = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<WorkerLocation>().cloned());
    let Some(location) = location else {
        return Err(ec.new_type_error("receiver is not a WorkerLocation"));
    };
    f(&location, ec)
}

impl WebIdlInterface<Types> for WorkerLocation {
    const NAME: &'static str = "WorkerLocation";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        // The stringifier is exposed as toString returning the href
        // serialization.
        // <https://html.spec.whatwg.org/#dom-workerlocation-href>
        def.add_operation(OperationDef {
            id: "toString",
            length: 0,
            method: to_string_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "href",
            getter: get_href,
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
            id: "protocol",
            getter: get_protocol,
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
            id: "host",
            getter: get_host,
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
            id: "hostname",
            getter: get_hostname,
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
            id: "port",
            getter: get_port,
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
            id: "pathname",
            getter: get_pathname,
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
            id: "search",
            getter: get_search,
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
            id: "hash",
            getter: get_hash,
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
    }
}

/// <https://html.spec.whatwg.org/#dom-workerlocation-href>
fn string_value(
    location: &WorkerLocation,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(ec.value_from_string(ec.js_string_from_str(&location.href())))
}

fn to_string_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_worker_location_ref(this, ec, string_value)
}

macro_rules! define_worker_location_getter {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            with_worker_location_ref(this, ec, |location, ec| {
                Ok(ec.value_from_string(ec.js_string_from_str(&location.$method())))
            })
        }
    };
}

define_worker_location_getter!(get_href, href);
define_worker_location_getter!(get_origin, origin);
define_worker_location_getter!(get_protocol, protocol);
define_worker_location_getter!(get_host, host);
define_worker_location_getter!(get_hostname, hostname);
define_worker_location_getter!(get_port, port);
define_worker_location_getter!(get_pathname, pathname);
define_worker_location_getter!(get_search, search);
define_worker_location_getter!(get_hash, hash);
