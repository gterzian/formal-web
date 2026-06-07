use boa_engine::{JsValue, js_string, property::Attribute};

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

/// Define the constants on the interface prototype object AND the
/// interface object per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#define-the-constants
///
/// The spec says constants are exposed on:
/// - interface objects (constructors)
/// - interface prototype objects
/// - every object that implements the interface (when [Global])
///
/// We use `class.property()` for the prototype and `class.static_property()`
/// for the constructor (interface object).
pub(crate) fn define_constants(
    target: &mut boa_engine::class::ClassBuilder<'_>,
    constants: &[ConstantDef],
) {
    for constant in constants {
        // Step 1.1: "If const is not exposed in realm, then continue."
        // Note: Exposure checks are not yet implemented.

        // Step 1.2: "Let value be the result of converting const's IDL
        //            value to a JavaScript value."
        // (Done at ConstantDef construction time.)

        // Step 1.3: "Let desc be the PropertyDescriptor{[[Writable]]: false,
        //            [[Enumerable]]: true, [[Configurable]]: false,
        //            [[Value]]: value}."
        let attrs = Attribute::ENUMERABLE;

        // Step 1.4: "Let id be const's identifier."
        // Step 1.5: "Perform ! DefinePropertyOrThrow(target, id, desc)."
        // target = interface prototype object
        target.property(js_string!(constant.id), constant.value.clone(), attrs);

        // Also define on the interface object (constructor).
        // https://webidl.spec.whatwg.org/#create-an-interface-object step 13:
        // "Define the constants of interface I on F given realm."
        target.static_property(
            js_string!(constant.id),
            constant.value.clone(),
            attrs,
        );
    }
}
