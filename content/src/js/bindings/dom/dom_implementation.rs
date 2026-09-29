use crate::dom::DOMImplementation;
use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, OperationDef, WebIdlInterface};
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
