use boa_engine::{JsResult, JsValue};
use std::marker::PhantomData;

use crate::dom::DOMException;
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
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
            let message = args
                .get(0)
                .map(|value| value.to_string(ctx).map(|v| v.to_std_string_escaped()))
                .transpose()?
                .unwrap_or_default();
            let name = args
                .get(1)
                .map(|value| value.to_string(ctx).map(|v| v.to_std_string_escaped()))
                .transpose()?
                .unwrap_or_else(|| String::from("Error"));
            Ok(DOMException::new(message, name))
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("DOMException receiver is not an object"))?;
    let exception = obj
        .downcast_ref::<DOMException>()
        .ok_or_else(|| ec.new_type_error("receiver is not a DOMException"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(exception.name_value())))
}

fn get_message(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("DOMException receiver is not an object"))?;
    let exception = obj
        .downcast_ref::<DOMException>()
        .ok_or_else(|| ec.new_type_error("receiver is not a DOMException"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(exception.message_value())))
}

fn get_code(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("DOMException receiver is not an object"))?;
    let exception = obj
        .downcast_ref::<DOMException>()
        .ok_or_else(|| ec.new_type_error("receiver is not a DOMException"))?;
    Ok(ec.value_from_number(exception.code_value() as f64))
}
