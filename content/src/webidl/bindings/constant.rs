use boa_engine::{JsObject, JsValue, js_string, property::PropertyDescriptor};
use std::marker::PhantomData;

use js_engine::JsTypes;

/// Describes a constant on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-constant
pub(crate) struct ConstantDef<T: JsTypes> {
    pub id: &'static str,
    pub value: JsValue,
    pub _phantom: PhantomData<T>,
}

/// <https://webidl.spec.whatwg.org/#define-the-constants>
pub(crate) fn define_constants(
    target: &JsObject,
    context: &mut boa_engine::Context,
    constants: &[ConstantDef<crate::js::Types>],
) -> boa_engine::JsResult<()> {
    for constant in constants {
        let desc = PropertyDescriptor::builder()
            .value(constant.value.clone())
            .writable(false)
            .enumerable(true)
            .configurable(false)
            .build();
        target.define_property_or_throw(js_string!(constant.id), desc, context)?;
    }
    Ok(())
}
