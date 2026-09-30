use crate::js::Types;
use crate::js::bindings::file_api::blob_from_value;
use crate::webidl::bindings::{
    AttributeDef, ConstantDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};
use crate::webidl::{
    clamp_unsigned_short, convert_js_to_sequence, get_a_copy_of_the_buffer_source,
    is_buffer_source, usv_string_value,
};
use crate::websockets::websocket::{CLOSED, CLOSING, CONNECTING, OPEN};
use crate::websockets::{BinaryType, WebSocket, WebSocketSendData};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{
    event_handlers, optional_usv_string_argument, string_value, this_as, usv_string_argument,
};

type JsValue = <Types as JsTypes>::JsValue;

fn socket(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<WebSocket, Types> {
    this_as::<WebSocket>(this, "WebSocket", ec)
}

impl WebIdlInterface<Types> for WebSocket {
    const NAME: &'static str = "WebSocket";

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
        if args.is_empty() {
            return Err(
                ec.new_type_error("WebSocket constructor: 1 argument required, but only 0 present")
            );
        }
        let url = usv_string_argument(args, 0, ec)?;
        let protocols = match args.get(1) {
            Some(value) if !Types::value_is_undefined(value) => {
                let iterator_key = ec.property_key_from_well_known_symbol("iterator");
                let is_sequence = Types::value_as_object(value).is_some()
                    && ec.get_method(value.clone(), iterator_key)?.is_some();
                if is_sequence {
                    convert_js_to_sequence(value, usv_string_value, ec)?
                } else {
                    vec![ec.to_rust_string(value.clone())?]
                }
            }
            _ => Vec::new(),
        };
        WebSocket::constructor(url, protocols, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_constant(ConstantDef::number("CONNECTING", f64::from(CONNECTING)));
        def.add_constant(ConstantDef::number("OPEN", f64::from(OPEN)));
        def.add_constant(ConstantDef::number("CLOSING", f64::from(CLOSING)));
        def.add_constant(ConstantDef::number("CLOSED", f64::from(CLOSED)));
        for (id, getter, setter) in [
            ("url", url as _, None),
            ("readyState", ready_state as _, None),
            ("bufferedAmount", buffered_amount as _, None),
            ("extensions", extensions as _, None),
            ("protocol", protocol as _, None),
            ("binaryType", binary_type as _, Some(set_binary_type as _)),
            ("onopen", get_onopen as _, Some(set_onopen as _)),
            ("onerror", get_onerror as _, Some(set_onerror as _)),
            ("onclose", get_onclose as _, Some(set_onclose as _)),
            ("onmessage", get_onmessage as _, Some(set_onmessage as _)),
        ] {
            def.add_attribute(AttributeDef {
                id,
                getter,
                setter,
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
        for (id, length, method) in [("send", 1, send as _), ("close", 0, close as _)] {
            def.add_operation(OperationDef {
                id,
                length,
                method,
                static_: false,
                unforgeable: false,
                promise_type: false,
                exposed: None,
            });
        }
    }
}

event_handlers! {
    get_onopen, set_onopen, "open";
    get_onerror, set_onerror, "error";
    get_onclose, set_onclose, "close";
    get_onmessage, set_onmessage, "message";
}

fn url(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let url = socket(this, ec)?.url();
    Ok(string_value(&url, ec))
}

fn ready_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let state = socket(this, ec)?.ready_state();
    Ok(ec.value_from_number(f64::from(state)))
}

fn buffered_amount(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let amount = socket(this, ec)?.buffered_amount();
    Ok(ec.value_from_number(amount as f64))
}

fn extensions(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let extensions = socket(this, ec)?.extensions();
    Ok(string_value(&extensions, ec))
}

fn protocol(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let protocol = socket(this, ec)?.protocol();
    Ok(string_value(&protocol, ec))
}

fn binary_type(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let binary_type = socket(this, ec)?.binary_type();
    Ok(string_value(binary_type.as_idl(), ec))
}

fn set_binary_type(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let socket = socket(this, ec)?;
    let value = usv_string_argument(args, 0, ec)?;
    socket.set_binary_type(BinaryType::from_idl(&value));
    Ok(ec.value_undefined())
}

fn send(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let socket = socket(this, ec)?;
    if args.is_empty() {
        return Err(ec.new_type_error("WebSocket.send: 1 argument required, but only 0 present"));
    }
    let value = &args[0];
    let data = if let Some(blob) = blob_from_value(value, ec) {
        WebSocketSendData::Blob(blob)
    } else if is_buffer_source(value, ec) {
        let bytes = get_a_copy_of_the_buffer_source(value, ec)?;
        let is_array_buffer = Types::value_as_object(value)
            .is_some_and(|object| Types::object_as_array_buffer(&object).is_some());
        if is_array_buffer {
            WebSocketSendData::ArrayBuffer(bytes)
        } else {
            WebSocketSendData::ArrayBufferView(bytes)
        }
    } else {
        WebSocketSendData::String(ec.to_rust_string(value.clone())?)
    };
    socket.send(data, ec)?;
    Ok(ec.value_undefined())
}

fn close(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let socket = socket(this, ec)?;
    let code = match args.first() {
        Some(value) if !Types::value_is_undefined(value) => Some(clamp_unsigned_short(value, ec)?),
        _ => None,
    };
    let reason = optional_usv_string_argument(args, 1, ec)?;
    socket.close(code, reason, ec)?;
    Ok(ec.value_undefined())
}
