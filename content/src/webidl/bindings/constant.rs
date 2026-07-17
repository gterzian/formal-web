use js_engine::{
    Completion, EcmascriptHost, ExecutionContext, JsTypes, JsTypesWithRealm, PropertyDescriptor,
};

/// The value of a Web IDL constant.
///
/// https://webidl.spec.whatwg.org/#dfn-constant
pub(crate) enum ConstValue<T: JsTypes> {
    Number(f64),
    Raw(<T as JsTypes>::JsValue),
}

/// Describes a constant on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-constant
pub(crate) struct ConstantDef<T: JsTypes> {
    pub id: &'static str,
    pub value: ConstValue<T>,
    /// Optional exposure restriction, e.g. "Window" or "Window,Worker".
    pub exposed: Option<&'static str>,
}

impl<T: JsTypes> ConstantDef<T> {
    pub fn number(id: &'static str, n: f64) -> Self {
        Self {
            id,
            value: ConstValue::Number(n),
            exposed: None,
        }
    }
}

/// <https://webidl.spec.whatwg.org/#define-the-constants>
pub(crate) fn define_constants<Ty>(
    target: Ty::JsObject,
    ec: &mut dyn ExecutionContext<Ty>,
    constants: &[ConstantDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
{
    for constant in constants {
        // Step 1.1: "If const is not exposed in realm, then continue."
        if let Some(exposed_globals) = constant.exposed {
            if exposed_globals != "Window" {
                continue;
            }
        }
        let key = ec.property_key_from_str(constant.id);
        let value = match &constant.value {
            ConstValue::Number(n) => EcmascriptHost::value_from_number(ec, *n),
            ConstValue::Raw(v) => v.clone(),
        };
        let desc = PropertyDescriptor {
            value: Some(value),
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
