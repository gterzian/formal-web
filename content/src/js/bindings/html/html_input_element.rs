// ── HTMLInputElement JS bindings ──

use std::marker::PhantomData;
use boa_engine::{Context, JsNativeError, JsResult, JsString, JsValue};

use crate::html::HTMLInputElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

impl WebIdlInterface<js_engine::boa::BoaTypes> for HTMLInputElement {
    const NAME: &'static str = "HTMLInputElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "value",
            getter: get_value,
            setter: Some(set_value),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "focus",
            length: 0,
            method: focus_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

fn get_value(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let input = obj
        .downcast_ref::<HTMLInputElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("expected HTMLInputElement"))?;
    Ok(JsValue::from(JsString::from(input.value())))
}

fn focus_method(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let _obj = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    // Note: focus() is a no-op — element focus management not yet implemented.
    Ok(JsValue::undefined())
}

fn set_value(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let input = obj
        .downcast_ref::<HTMLInputElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("expected HTMLInputElement"))?;
    let value = args
        .first()
        .map(|v| v.to_string(context))
        .transpose()?
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    input.set_value(&value);
    Ok(JsValue::undefined())
}
