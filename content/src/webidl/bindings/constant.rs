use std::marker::PhantomData;

use js_engine::{Completion, ExecutionContext, JsTypes, PropertyDescriptor};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

/// Describes a constant on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-constant
pub(crate) struct ConstantDef<T: JsTypes> {
    pub id: &'static str,
    pub value: <T as JsTypes>::JsValue,
    pub _phantom: PhantomData<T>,
}

/// <https://webidl.spec.whatwg.org/#define-the-constants>
pub(crate) fn define_constants(
    target: <Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    constants: &[ConstantDef<Types>],
) -> Completion<(), Types> {
    for constant in constants {
        let key = ec.property_key_from_str(constant.id);
        let desc = PropertyDescriptor {
            value: Some(constant.value.clone()),
            writable: Some(false),
            enumerable: Some(true),
            configurable: Some(false),
            get: None,
            set: None,
        };
        ec.define_property_or_throw(target.clone(), key, desc)?;
    }
    Ok(())
}
