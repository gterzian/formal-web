use std::marker::PhantomData;
use boa_engine::{Context, JsNativeError, JsResult, JsString, JsValue};

use crate::dom::DOMException;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for DOMException {
    const NAME: &'static str = "DOMException";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let message = args
            .get(0)
            .map(|value| {
                value
                    .to_string(context)
                    .map(|value| value.to_std_string_escaped())
            })
            .transpose()?
            .unwrap_or_default();
        let name = args
            .get(1)
            .map(|value| {
                value
                    .to_string(context)
                    .map(|value| value.to_std_string_escaped())
            })
            .transpose()?
            .unwrap_or_else(|| String::from("Error"));
        Ok(DOMException::new(message, name))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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

fn get_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_dom_exception_ref(this, |exception| {
        JsValue::from(JsString::from(exception.name_value()))
    })
}

fn get_message(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_dom_exception_ref(this, |exception| {
        JsValue::from(JsString::from(exception.message_value()))
    })
}

fn get_code(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_dom_exception_ref(this, |exception| JsValue::from(exception.code_value()))
}

fn with_dom_exception_ref<R>(this: &JsValue, f: impl FnOnce(&DOMException) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("DOMException receiver is not an object")
    })?;
    let exception = object
        .downcast_ref::<DOMException>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not a DOMException"))?;
    Ok(f(&exception))
}
