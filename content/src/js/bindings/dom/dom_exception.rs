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

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

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
