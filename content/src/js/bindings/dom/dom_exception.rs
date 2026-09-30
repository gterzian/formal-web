use crate::dom::DOMException;
type JsValue = <crate::js::Types as JsTypes>::JsValue;

fn with_dom_exception_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&DOMException) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("DOMException receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(exception) = data.downcast_ref::<DOMException>() {
            return Ok(f(exception));
        }
    }
    Err(ec.new_type_error("receiver is not a DOMException"))
}

use crate::webidl::bindings::{AttributeDef, ConstantDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for DOMException {
    const NAME: &'static str = "DOMException";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let message = if let Some(value) = args.first() {
            ec.to_rust_string(value.clone())?
        } else {
            String::default()
        };
        let name = if let Some(value) = args.get(1) {
            ec.to_rust_string(value.clone())?
        } else {
            String::from("Error")
        };
        Ok(DOMException::new(message, name))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        for (name, code) in LEGACY_CODE_CONSTANTS {
            def.add_constant(ConstantDef::number(name, f64::from(*code)));
        }
        def.add_attribute(AttributeDef {
            id: "name",
            getter: get_name,
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
            id: "message",
            getter: get_message,
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
            id: "code",
            getter: get_code,
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

fn get_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let name = with_dom_exception_ref(this, ec, |ex| ex.name_value().to_string())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&name)))
}

fn get_message(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let msg = with_dom_exception_ref(this, ec, |ex| ex.message_value().to_string())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&msg)))
}

fn get_code(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = with_dom_exception_ref(this, ec, |ex| ex.code_value())?;
    Ok(ec.value_from_number(val as f64))
}

/// <https://webidl.spec.whatwg.org/#idl-DOMException-error-names>
const LEGACY_CODE_CONSTANTS: &[(&str, u16)] = &[
    ("INDEX_SIZE_ERR", 1),
    ("DOMSTRING_SIZE_ERR", 2),
    ("HIERARCHY_REQUEST_ERR", 3),
    ("WRONG_DOCUMENT_ERR", 4),
    ("INVALID_CHARACTER_ERR", 5),
    ("NO_DATA_ALLOWED_ERR", 6),
    ("NO_MODIFICATION_ALLOWED_ERR", 7),
    ("NOT_FOUND_ERR", 8),
    ("NOT_SUPPORTED_ERR", 9),
    ("INUSE_ATTRIBUTE_ERR", 10),
    ("INVALID_STATE_ERR", 11),
    ("SYNTAX_ERR", 12),
    ("INVALID_MODIFICATION_ERR", 13),
    ("NAMESPACE_ERR", 14),
    ("INVALID_ACCESS_ERR", 15),
    ("VALIDATION_ERR", 16),
    ("TYPE_MISMATCH_ERR", 17),
    ("SECURITY_ERR", 18),
    ("NETWORK_ERR", 19),
    ("ABORT_ERR", 20),
    ("URL_MISMATCH_ERR", 21),
    ("QUOTA_EXCEEDED_ERR", 22),
    ("TIMEOUT_ERR", 23),
    ("INVALID_NODE_TYPE_ERR", 24),
    ("DATA_CLONE_ERR", 25),
];
