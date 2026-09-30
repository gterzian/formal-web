pub(crate) mod dom;
pub(crate) mod encoding;
pub(crate) mod file_api;
pub(crate) mod html;
pub(crate) mod initialization;
pub(crate) mod streams;
pub(crate) mod testutils;
pub(crate) mod ui_events;
pub(crate) mod url_standard;
#[cfg(all(boa_backend, feature = "wasm"))]
pub(crate) mod wasm;
pub(crate) mod webrtc;
pub(crate) mod websockets;

pub(crate) use dom::install_document_property;
#[cfg(all(boa_backend, feature = "wasm"))]
pub(crate) use wasm::install_wasm_namespace;

use crate::js::Types;
use crate::webidl::dictionary::{DictionaryAccess, convert_js_to_dictionary};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

pub(crate) fn this_as<T: Clone + 'static>(
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

pub(crate) fn dictionary(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<DictionaryAccess<Types>, Types> {
    match value {
        Some(value) => convert_js_to_dictionary(value, ec),
        None => Ok(DictionaryAccess::Empty),
    }
}

pub(crate) fn string_member(
    dict: &DictionaryAccess<Types>,
    key: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    match dict.get_member(key, ec)? {
        Some(value) => Ok(Some(ec.to_rust_string(value)?)),
        None => Ok(None),
    }
}

pub(crate) fn nullable_string_member(
    dict: &DictionaryAccess<Types>,
    key: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    match dict.get_member(key, ec)? {
        Some(value) if !Types::value_is_null(&value) => Ok(Some(ec.to_rust_string(value)?)),
        _ => Ok(None),
    }
}

pub(crate) fn boolean_member(
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

pub(crate) fn usv_string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    let undefined = ec.value_undefined();
    ec.to_rust_string(args.get(index).cloned().unwrap_or(undefined))
}

pub(crate) fn optional_usv_string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    match args.get(index) {
        Some(value) if !Types::value_is_undefined(value) => {
            Ok(Some(ec.to_rust_string(value.clone())?))
        }
        _ => Ok(None),
    }
}

pub(crate) fn string_value(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}
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
pub(crate) use event_handlers;
