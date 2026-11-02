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
        // https://webidl.spec.whatwg.org/#idl-DOMException
        def.add_constant(ConstantDef::number("INDEX_SIZE_ERR", 1.0));
        def.add_constant(ConstantDef::number("DOMSTRING_SIZE_ERR", 2.0));
        def.add_constant(ConstantDef::number("HIERARCHY_REQUEST_ERR", 3.0));
        def.add_constant(ConstantDef::number("WRONG_DOCUMENT_ERR", 4.0));
        def.add_constant(ConstantDef::number("INVALID_CHARACTER_ERR", 5.0));
        def.add_constant(ConstantDef::number("NO_DATA_ALLOWED_ERR", 6.0));
        def.add_constant(ConstantDef::number("NO_MODIFICATION_ALLOWED_ERR", 7.0));
        def.add_constant(ConstantDef::number("NOT_FOUND_ERR", 8.0));
        def.add_constant(ConstantDef::number("NOT_SUPPORTED_ERR", 9.0));
        def.add_constant(ConstantDef::number("INUSE_ATTRIBUTE_ERR", 10.0));
        def.add_constant(ConstantDef::number("INVALID_STATE_ERR", 11.0));
        def.add_constant(ConstantDef::number("SYNTAX_ERR", 12.0));
        def.add_constant(ConstantDef::number("INVALID_MODIFICATION_ERR", 13.0));
        def.add_constant(ConstantDef::number("NAMESPACE_ERR", 14.0));
        def.add_constant(ConstantDef::number("INVALID_ACCESS_ERR", 15.0));
        def.add_constant(ConstantDef::number("VALIDATION_ERR", 16.0));
        def.add_constant(ConstantDef::number("TYPE_MISMATCH_ERR", 17.0));
        def.add_constant(ConstantDef::number("SECURITY_ERR", 18.0));
        def.add_constant(ConstantDef::number("NETWORK_ERR", 19.0));
        def.add_constant(ConstantDef::number("ABORT_ERR", 20.0));
        def.add_constant(ConstantDef::number("URL_MISMATCH_ERR", 21.0));
        def.add_constant(ConstantDef::number("QUOTA_EXCEEDED_ERR", 22.0));
        def.add_constant(ConstantDef::number("TIMEOUT_ERR", 23.0));
        def.add_constant(ConstantDef::number("INVALID_NODE_TYPE_ERR", 24.0));
        def.add_constant(ConstantDef::number("DATA_CLONE_ERR", 25.0));
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
