use crate::js::Types;
use crate::js::bindings::this_as;
use crate::resize_observer::ResizeObserverSize;
use crate::webidl::bindings::{AttributeDef, BindingFn, InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for ResizeObserverSize {
    const NAME: &'static str = "ResizeObserverSize";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [
            ("inlineSize", get_inline_size as BindingFn<Types>),
            ("blockSize", get_block_size),
        ] {
            def.add_attribute(AttributeDef {
                id,
                getter,
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
}

fn get_inline_size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = this_as::<ResizeObserverSize>(this, "ResizeObserverSize", ec)?.inline_size();
    Ok(ec.value_from_number(value))
}

fn get_block_size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = this_as::<ResizeObserverSize>(this, "ResizeObserverSize", ec)?.block_size();
    Ok(ec.value_from_number(value))
}
