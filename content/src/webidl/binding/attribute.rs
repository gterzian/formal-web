use boa_engine::{
    Context, JsObject, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    property::PropertyDescriptor,
};

/// Describes a single attribute on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-attribute
pub(crate) struct AttributeDef {
    /// The attribute's identifier.
    pub id: &'static str,

    /// The getter steps: given `this` as a `JsValue`, returns the attribute
    /// value as a `JsValue`.
    ///
    /// This function pointer must downcast `this` to the platform object
    /// type internally.  The signature matches `NativeFunction::from_fn_ptr`
    /// so it can be used directly.
    pub getter: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,

    /// Optional setter steps.  `None` for read-only attributes.
    pub setter: Option<fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>>,

    /// Whether the attribute is static (exposed on the interface object).
    pub static_: bool,

    /// Whether the attribute is unforgeable.
    pub unforgeable: bool,

    /// Whether the attribute's type is a promise type.
    pub promise_type: bool,

    /// Whether the attribute is declared with [LegacyLenientThis].
    pub legacy_lenient_this: bool,

    /// Whether the attribute is declared with [Replaceable].
    pub replaceable: bool,

    /// Whether the attribute is declared with [PutForwards].
    pub put_forwards: Option<&'static str>,

    /// Whether the attribute is declared with [LegacyLenientSetter].
    pub legacy_lenient_setter: bool,
}

/// <https://webidl.spec.whatwg.org/#define-the-regular-attributes>
pub(crate) fn define_regular_attributes(
    proto: &JsObject,
    context: &mut Context,
    attributes: &[AttributeDef],
) -> JsResult<()> {
    // Step 1: "Let attributes be the list of regular attributes that are
    //          members of definition."
    // Step 2: "Remove from attributes all the attributes that are
    //          unforgeable."
    let regular: Vec<&AttributeDef> = attributes
        .iter()
        .filter(|a| !a.static_ && !a.unforgeable)
        .collect();

    // Step 3: "Define the attributes attributes of definition on target
    //          given realm."
    define_attributes_on_target(proto, context, &regular)
}

/// Define the static attributes on the interface object.
///
/// https://webidl.spec.whatwg.org/#define-the-static-attributes
pub(crate) fn define_static_attributes(
    _constructor: &JsObject,
    _context: &mut Context,
    _attributes: &[AttributeDef],
) -> JsResult<()> {
    Ok(())
}

/// Define the unforgeable regular attributes on the target.
///
/// https://webidl.spec.whatwg.org/#define-the-unforgeable-regular-attributes
pub(crate) fn define_unforgeable_regular_attributes(
    _proto: &JsObject,
    _context: &mut Context,
    _attributes: &[AttributeDef],
) -> JsResult<()> {
    Ok(())
}

/// <https://webidl.spec.whatwg.org/#define-the-attributes>
fn define_attributes_on_target(
    proto: &JsObject,
    context: &mut Context,
    attributes: &[&AttributeDef],
) -> JsResult<()> {
    let realm = context.realm().clone();

    // Step 1: "For each attribute attr of attributes:"
    for attr in attributes {
        // Step 1.1: "If attr is not exposed in realm, then continue."
        // Note: Exposure checks are not yet implemented.

        // Step 1.2: "Let getter be the result of creating an attribute
        //            getter given attr, definition, and realm."
        let getter_fn = NativeFunction::from_fn_ptr(attr.getter).to_js_function(&realm);

        // Step 1.3: "Let setter be the result of creating an attribute
        //            setter given attr, definition, and realm."
        // Note: The algorithm returns undefined if attr is read only.
        let setter_fn = attr.setter.map(|s| {
            NativeFunction::from_fn_ptr(s).to_js_function(&realm)
        });

        // Step 1.4: "Let configurable be false if attr is unforgeable
        //            and true otherwise."
        let configurable = !attr.unforgeable;

        // Step 1.5: "Let desc be the PropertyDescriptor{[[Get]]: getter,
        //            [[Set]]: setter, [[Enumerable]]: true,
        //            [[Configurable]]: configurable}."
        let mut desc = PropertyDescriptor::builder()
            .get(getter_fn)
            .enumerable(true)
            .configurable(configurable);
        if let Some(setter) = setter_fn {
            desc = desc.set(setter);
        }

        // Step 1.6: "Let id be attr's identifier."
        // Step 1.7: "Perform ! DefinePropertyOrThrow(target, id, desc)."
        proto.define_property_or_throw(js_string!(attr.id), desc.build(), context)?;

        // Step 1.8: "If attr's type is an observable array type with type
        //            argument T, then ..."
        // Note: Observable array types are not yet implemented.
    }

    Ok(())
}
