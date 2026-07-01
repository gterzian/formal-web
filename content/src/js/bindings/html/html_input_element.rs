// ── HTMLInputElement JS bindings ──

use boa_engine::JsValue;
use std::marker::PhantomData;

use crate::html::HTMLInputElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLInputElement {
    const NAME: &'static str = "HTMLInputElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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

fn get_value(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let input = obj
        .downcast_ref::<HTMLInputElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLInputElement"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&input.value())))
}

fn focus_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    // Note: focus() is a no-op — element focus management not yet implemented.
    Ok(ec.value_undefined())
}

fn set_value(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let input = obj
        .downcast_ref::<HTMLInputElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLInputElement"))?;
    let value = if let Some(v) = args.first() {
        ec.to_rust_string(v.clone())?
    } else {
        String::default()
    };
    input.set_value(&value);
    Ok(ec.value_undefined())
}
