//! Bindings for WebRTC (<https://w3c.github.io/webrtc-pc/>): argument
//! conversion to the IDL types of `crate::webrtc`, then the domain call.

mod events;
mod rtc_data_channel;
mod rtc_ice_candidate;
mod rtc_peer_connection;
mod rtc_session_description;

use crate::js::Types;
use crate::webidl::dictionary::{DictionaryAccess, convert_js_to_dictionary};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

/// Downcast the receiver to its platform object data.
pub(super) fn this_as<T: Clone + 'static>(
    this: &JsValue,
    interface: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<T, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error(&format!("{interface} receiver is not an object")))?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<T>().cloned())
        .ok_or_else(|| ec.new_type_error(&format!("receiver is not an {interface}")))
}

/// <https://webidl.spec.whatwg.org/#js-dictionary>
pub(super) fn dictionary(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<DictionaryAccess<Types>, Types> {
    match value {
        Some(value) => convert_js_to_dictionary(value, ec),
        None => Ok(DictionaryAccess::Empty),
    }
}

/// A DOMString dictionary member.
pub(super) fn string_member(
    dict: &DictionaryAccess<Types>,
    key: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    match dict.get_member(key, ec)? {
        Some(value) => Ok(Some(ec.to_rust_string(value)?)),
        None => Ok(None),
    }
}

/// A `DOMString?` dictionary member: null and absent are both None.
pub(super) fn nullable_string_member(
    dict: &DictionaryAccess<Types>,
    key: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    match dict.get_member(key, ec)? {
        Some(value) if !Types::value_is_null(&value) => Ok(Some(ec.to_rust_string(value)?)),
        _ => Ok(None),
    }
}

/// A `boolean` dictionary member with a default.
pub(super) fn boolean_member(
    dict: &DictionaryAccess<Types>,
    key: &str,
    default: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types> {
    Ok(match dict.get_member(key, ec)? {
        Some(value) => ec.to_boolean(&value),
        None => default,
    })
}

/// Event handler IDL attributes: a getter and a setter per event type.
/// <https://html.spec.whatwg.org/#event-handler-idl-attributes>
macro_rules! event_handlers {
    ($($getter:ident, $setter:ident, $event:literal);* $(;)?) => {
        $(
            fn $getter(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                let object = Types::value_as_object(this)
                    .ok_or_else(|| ec.new_type_error("receiver is not an object"))?;
                let Some(target) = crate::js::downcast::event_target_from_js_object(ec, &object) else {
                    return Ok(ec.value_null());
                };
                let callback =
                    crate::html::event_handler::event_handler_idl_attribute_getter(&target, $event, ec);
                Ok(callback
                    .map(|callback| callback.to_js_value())
                    .unwrap_or_else(|| ec.value_null()))
            }

            fn $setter(
                this: &JsValue,
                args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                let object = Types::value_as_object(this)
                    .ok_or_else(|| ec.new_type_error("receiver is not an object"))?;
                let undefined = ec.value_undefined();
                let callback = crate::webidl::nullable_value(
                    args.first().unwrap_or(&undefined),
                    ec,
                    crate::webidl::callback_function_value,
                )?;
                if let Some(target) = crate::js::downcast::event_target_from_js_object(ec, &object) {
                    crate::html::event_handler::event_handler_idl_attribute_setter(
                        &target, $event, callback, ec,
                    );
                }
                Ok(ec.value_undefined())
            }
        )*
    };
}
pub(super) use event_handlers;

/// Register a read-only attribute, a read-write attribute or an operation.
macro_rules! member {
    ($def:expr, attribute $id:literal, $getter:expr) => {
        $def.add_attribute(crate::webidl::bindings::AttributeDef {
            id: $id,
            getter: $getter,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        })
    };
    ($def:expr, attribute $id:literal, $getter:expr, $setter:expr) => {
        $def.add_attribute(crate::webidl::bindings::AttributeDef {
            id: $id,
            getter: $getter,
            setter: Some($setter),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        })
    };
    ($def:expr, operation $id:literal, $length:expr, $method:expr, promise $promise:expr) => {
        $def.add_operation(crate::webidl::bindings::OperationDef {
            id: $id,
            length: $length,
            method: $method,
            static_: false,
            unforgeable: false,
            promise_type: $promise,
            exposed: None,
        })
    };
}
pub(super) use member;
