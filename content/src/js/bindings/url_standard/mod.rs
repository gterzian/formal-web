//! Bindings for the URL Standard (<https://url.spec.whatwg.org/>): argument
//! conversion to the IDL types of `crate::url_standard`, then the domain call.

mod url;
mod url_search_params;

pub(super) use super::{optional_usv_string_argument, string_value, this_as, usv_string_argument};

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
    ($def:expr, operation $id:literal, $length:literal, $method:expr) => {
        $def.add_operation(crate::webidl::bindings::OperationDef {
            id: $id,
            length: $length,
            method: $method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        })
    };
    ($def:expr, static operation $id:literal, $length:literal, $method:expr) => {
        $def.add_operation(crate::webidl::bindings::OperationDef {
            id: $id,
            length: $length,
            method: $method,
            static_: true,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        })
    };
}
pub(super) use member;
