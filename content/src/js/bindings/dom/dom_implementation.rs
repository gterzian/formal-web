use crate::dom::{DOMImplementation, Document};
use crate::js::Types;
use crate::webidl::bindings::{
    InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::this_as;

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for DOMImplementation {
    const NAME: &'static str = "DOMImplementation";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_operation(OperationDef {
            id: "hasFeature",
            length: 0,
            method: has_feature,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createHTMLDocument",
            length: 0,
            method: create_html_document,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn has_feature(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = this_as::<DOMImplementation>(this, "DOMImplementation", ec)?.has_feature();
    Ok(ec.value_from_bool(value))
}

fn create_html_document(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let title = match args.first() {
        Some(value) if !Types::value_is_undefined(value) => Some(ec.to_rust_string(value.clone())?),
        _ => None,
    };
    let implementation = this_as::<DOMImplementation>(this, "DOMImplementation", ec)?;
    let document = implementation.create_html_document(title, ec);
    let object = create_interface_instance::<Types, Document>(document, ec)?;
    Ok(Types::value_from_object(object))
}
