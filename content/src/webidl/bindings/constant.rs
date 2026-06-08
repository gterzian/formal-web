use boa_engine::{JsObject, JsValue, js_string, property::PropertyDescriptor};

/// Describes a constant on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-constant
pub(crate) struct ConstantDef {
    /// The constant's identifier.
    pub id: &'static str,

    /// The constant value as a JavaScript value.
    ///
    /// Per https://webidl.spec.whatwg.org/#define-the-constants step 1.2:
    /// "Let value be the result of converting const's IDL value to a
    /// JavaScript value."  Pre-compute this conversion when building the
    /// definition.
    pub value: JsValue,
}

/// <https://webidl.spec.whatwg.org/#define-the-constants>
///
/// The spec says constants are exposed on both the interface prototype
/// object and the interface object (constructor).  Call this once for
/// each target.
pub(crate) fn define_constants(
    target: &JsObject,
    context: &mut boa_engine::Context,
    constants: &[ConstantDef],
) -> boa_engine::JsResult<()> {
    // Step 1: "For each constant const that is a member of definition:"
    for constant in constants {
        // Step 1.1: "If const is not exposed in realm, then continue."
        // Note: Exposure checks are not yet implemented.

        // Step 1.2: "Let value be the result of converting const's IDL
        //            value to a JavaScript value."
        // Note: Done at ConstantDef construction time.

        // Step 1.3: "Let desc be the PropertyDescriptor{[[Writable]]: false,
        //            [[Enumerable]]: true, [[Configurable]]: false,
        //            [[Value]]: value}."
        let desc = PropertyDescriptor::builder()
            .value(constant.value.clone())
            .writable(false)
            .enumerable(true)
            .configurable(false)
            .build();

        // Step 1.4: "Let id be const's identifier."
        // Step 1.5: "Perform ! DefinePropertyOrThrow(target, id, desc)."
        target.define_property_or_throw(js_string!(constant.id), desc, context)?;
    }

    Ok(())
}
