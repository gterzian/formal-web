use boa_engine::{
    Context, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    property::Attribute,
    realm::Realm,
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
    /// type internally.  The signature matches `NativeFunctionPointer` so
    /// it can be used directly with `NativeFunction::from_fn_ptr`.
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

/// Define the regular attributes on the target (interface prototype object).
///
/// https://webidl.spec.whatwg.org/#define-the-regular-attributes
pub(crate) fn define_regular_attributes(
    target: &mut boa_engine::class::ClassBuilder<'_>,
    realm: &Realm,
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
    define_attributes_on_target(target, realm, &regular)
}

/// Define the static attributes on the target (interface object).
///
/// https://webidl.spec.whatwg.org/#define-the-static-attributes
pub(crate) fn define_static_attributes(
    _target: &mut boa_engine::class::ClassBuilder<'_>,
    _realm: &Realm,
    _attributes: &[AttributeDef],
) -> JsResult<()> {
    Ok(())
}

/// Define the unforgeable regular attributes on the target.
///
/// https://webidl.spec.whatwg.org/#define-the-unforgeable-regular-attributes
pub(crate) fn define_unforgeable_regular_attributes(
    _target: &mut boa_engine::class::ClassBuilder<'_>,
    _realm: &Realm,
    _attributes: &[AttributeDef],
) -> JsResult<()> {
    Ok(())
}

/// Define the attributes on target per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#define-the-attributes
fn define_attributes_on_target(
    target: &mut boa_engine::class::ClassBuilder<'_>,
    realm: &Realm,
    attributes: &[&AttributeDef],
) -> JsResult<()> {
    for attr in attributes {
        // Step 1.2: "Let getter be the result of creating an attribute
        //            getter given attr, definition, and realm."
        let getter_native = create_attribute_getter_native(attr);

        // Step 1.3: "Let setter be the result of creating an attribute
        //            setter given attr, definition, and realm."
        let setter_native = create_attribute_setter_native(attr);

        // Step 1.4: "Let configurable be false if attr is unforgeable
        //            and true otherwise."
        let configurable = !attr.unforgeable;

        // Step 1.5: "Let desc be the PropertyDescriptor{[[Get]]: getter,
        //            [[Set]]: setter, [[Enumerable]]: true,
        //            [[Configurable]]: configurable}."
        let mut attrs = Attribute::ENUMERABLE;
        if configurable {
            attrs |= Attribute::CONFIGURABLE;
        }
        attrs |= Attribute::WRITABLE;

        // Step 1.6: "Let id be attr's identifier."
        // Step 1.7: "Perform ! DefinePropertyOrThrow(target, id, desc)."
        target.accessor(
            js_string!(attr.id),
            Some(getter_native.to_js_function(realm)),
            setter_native.map(|f| f.to_js_function(realm)),
            attrs,
        );
    }

    Ok(())
}

/// Create an attribute getter NativeFunction per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#attribute-getter
///
/// Returns a `NativeFunction` that implements the getter algorithm.
/// The getter function pointer from `attr` is used directly.
///
/// Note: Promise-type error wrapping (step 1.2) is not yet implemented
/// here.  Each getter callback is responsible for handling its own
/// promise-type error wrapping until a closure-based approach can store
/// the fn pointer as traceable data.
fn create_attribute_getter_native(attr: &AttributeDef) -> NativeFunction {
    let getter_fn = attr.getter;
    NativeFunction::from_fn_ptr(getter_fn)
}

/// Create an attribute setter NativeFunction per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#attribute-setter
///
/// Returns `None` for read-only attributes (no setter).
fn create_attribute_setter_native(attr: &AttributeDef) -> Option<NativeFunction> {
    // Step 2: "If attribute is read only and does not have a
    //          [LegacyLenientSetter], [PutForwards] or [Replaceable]
    //          extended attribute, return undefined."
    let setter_fn = attr.setter?;

    Some(NativeFunction::from_fn_ptr(setter_fn))
}
