use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    builtins::object::OrdinaryObject,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, builtins::JsFunction},
    property::PropertyDescriptor,
};

use super::attribute::{
    AttributeDef, define_regular_attributes, define_static_attributes,
    define_unforgeable_regular_attributes,
};
use super::constant::{ConstantDef, define_constants};
use super::operation::{
    OperationDef, define_regular_operations, define_static_operations,
};

/// A buildable definition of an interface's members, collected by
/// `WebIdlInterface::define_members`.
///
/// This is the Web IDL analog of boa's `ClassBuilder`.  Call `add_attribute`,
/// `add_operation`, and `add_constant` inside `define_members` implementations.
///
/// https://webidl.spec.whatwg.org/#dfn-interface
pub(crate) struct InterfaceDefinition {
    pub(crate) attributes: Vec<AttributeDef>,
    pub(crate) operations: Vec<OperationDef>,
    pub(crate) constants: Vec<ConstantDef>,
}

impl InterfaceDefinition {
    pub(crate) fn new() -> Self {
        Self {
            attributes: Vec::new(),
            operations: Vec::new(),
            constants: Vec::new(),
        }
    }

    /// Add an attribute to this interface definition.
    ///
    /// https://webidl.spec.whatwg.org/#dfn-attribute
    pub(crate) fn add_attribute(&mut self, attr: AttributeDef) {
        self.attributes.push(attr);
    }

    /// Add an operation (method) to this interface definition.
    ///
    /// https://webidl.spec.whatwg.org/#dfn-operation
    pub(crate) fn add_operation(&mut self, op: OperationDef) {
        self.operations.push(op);
    }

    /// Add a constant to this interface definition.
    ///
    /// https://webidl.spec.whatwg.org/#dfn-constant
    pub(crate) fn add_constant(&mut self, const_: ConstantDef) {
        self.constants.push(const_);
    }
}

/// Trait for Web IDL platform objects that wish to expose a JavaScript
/// binding following the Web IDL specification.
///
/// https://webidl.spec.whatwg.org/#js-interfaces
pub(crate) trait WebIdlInterface: 'static {
    /// The interface identifier as used in IDL.
    const NAME: &'static str;

    /// The NAME of the parent interface, if this interface inherits from another.
    fn parent_name() -> Option<&'static str> {
        None
    }

    /// Whether this interface is declared with the [Global] extended attribute.
    fn is_global() -> bool {
        false
    }

    /// Whether this interface is declared with [LegacyNoInterfaceObject].
    fn no_interface_object() -> bool {
        false
    }

    /// Whether this interface supports named properties.
    fn supports_named_properties() -> bool {
        false
    }

    /// Whether this interface supports indexed properties.
    fn supports_indexed_properties() -> bool {
        false
    }

    /// Whether this interface uses immutable prototype exotic objects.
    fn immutable_prototype() -> bool {
        Self::is_global()
    }

    /// Create an instance of the platform object.
    ///
    /// https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface
    fn create_platform_object(
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self>
    where
        Self: Sized,
    {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    /// Define the interface members (attributes, operations, constants).
    fn define_members(def: &mut InterfaceDefinition)
    where
        Self: Sized;
}

// ─────────────────────────────────────────────────────────────────────────
//  Spec-Aligned Registration: §3.7.1 + §3.7.3
// ─────────────────────────────────────────────────────────────────────────

/// Create an interface prototype object per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#create-an-interface-prototype-object
///
/// ECMAScript → Boa mapping:
/// - `OrdinaryObjectCreate(proto)` → `JsObject::from_proto_and_data(proto, OrdinaryObject)`
/// - `DefinePropertyOrThrow(target, id, desc)` → `target.define_property_or_throw(id, desc, ctx)`
/// - `realm.[[Intrinsics]].[[%Object.prototype%]]` →
///     `context.intrinsics().constructors().object().prototype()`
pub(crate) fn create_interface_prototype_object<T: WebIdlInterface>(
    context: &mut Context,
) -> JsResult<JsObject> {
    let _realm = context.realm().clone();

    // Step 1: "Let proto be null."
    let mut proto: Option<JsObject> = None;

    // Step 2: [Global] + named properties → named properties object (not yet implemented)

    // Step 3: "If interface is declared to inherit from another interface,
    //          then set proto to the interface prototype object in realm
    //          of that inherited interface."
    if T::parent_name().is_some() {
        // Falls through to %Object.prototype% below until parent lookup is implemented.
        proto = None;
    }

    // Step 4: DOMException → %Error.prototype% (handled in DOMException binding)

    // Step 5: "Otherwise, set proto to realm.[[Intrinsics]].[[%Object.prototype%]]."
    let proto = proto.unwrap_or_else(|| {
        // ≡ OrdinaryObjectCreate(%Object.prototype%)
        context.intrinsics().constructors().object().prototype()
    });

    // Step 7-10: Create interfaceProtoObj.
    let interface_proto_obj = if T::immutable_prototype() {
        // Step 9: "MakeBasicObject(« [[Prototype]], [[Extensible]] »)" +
        //         immutable prototype exotic object.
        JsObject::from_proto_and_data(Some(proto.clone()), OrdinaryObject)
        // TODO: Set [[SetPrototypeOf]] to SetImmutablePrototype behavior.
    } else {
        // Step 8/10: ≡ OrdinaryObjectCreate(proto)
        JsObject::from_proto_and_data(Some(proto), OrdinaryObject)
    };

    // Step 11: [Unscopable] — not yet implemented.

    // Step 12: "If interface is not declared with the [Global] extended attribute:"
    if !T::is_global() {
        // Step 12.1: "Define the regular attributes of interface on
        //             interfaceProtoObj given realm."
        // Step 12.2: "Define the regular operations of interface on
        //             interfaceProtoObj given realm."
        // (These are handled via ClassBuilder in the current incremental
        //  migration path — see `register_interface`.)
    }

    // Step 13: "Define the constants of interface on interfaceProtoObj given realm."
    // (Handled via ClassBuilder in the incremental path.)

    Ok(interface_proto_obj)
}

/// Create an interface object per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#create-an-interface-object
///
/// ECMAScript → Boa mapping:
/// - `CreateBuiltinFunction(steps, length, id, internalSlots, realm, proto)` →
///     `FunctionObjectBuilder::new(realm, steps).name(id).length(length).build()`
/// - `OrdinaryObjectCreate(null)` → `JsObject::from_proto_and_data(None, OrdinaryObject)`
/// - `DefinePropertyOrThrow(F, "prototype", desc)` →
///     `F.define_property_or_throw(key, desc, context)`
pub(crate) fn create_interface_object<T: WebIdlInterface>(
    context: &mut Context,
) -> JsResult<JsFunction> {
    let realm = context.realm().clone();

    // Step 1: "Let steps be I's overridden constructor steps..."
    let steps = create_default_constructor_steps::<T>();

    // Step 2: "Let constructorProto be realm.[[Intrinsics]].[[%Function.prototype%]]."
    let _constructor_proto = context.intrinsics().constructors().function().prototype();

    // Step 4: "Let unforgeables be OrdinaryObjectCreate(null)."
    let _unforgeables = JsObject::from_proto_and_data(None, OrdinaryObject);

    // Step 8-9: "Let length be 0." (or compute from overload set)
    let length: usize = 0;

    // Step 10: "Let F be CreateBuiltinFunction(steps, length, id,
    //           « [[Unforgeables]] », realm, constructorProto)."
    let f = FunctionObjectBuilder::new(&realm, steps)
        .name(T::NAME)
        .length(length)
        .constructor(true)
        .build();

    let f_obj: JsObject = f.clone().into();

    // Step 11: "Let proto be the result of creating an interface prototype object."
    let proto = create_interface_prototype_object::<T>(context)?;

    // Step 12: "Perform ! DefinePropertyOrThrow(F, "prototype", ...)"
    let prototype_desc = PropertyDescriptor::builder()
        .value(proto.clone())
        .writable(false)
        .enumerable(false)
        .configurable(false)
        .build();
    f_obj.define_property_or_throw(js_string!("prototype"), prototype_desc, context)?;

    // Wire prototype.constructor back to F.
    let constructor_desc = PropertyDescriptor::builder()
        .value(f_obj.clone())
        .writable(true)
        .enumerable(false)
        .configurable(true)
        .build();
    proto.define_property_or_throw(js_string!("constructor"), constructor_desc, context)?;

    Ok(f)
}

/// Register a Web IDL interface using spec-aligned object creation.
///
/// Directly creates the interface object (§3.7.1) and interface prototype
/// object (§3.7.3) using:
/// - `FunctionObjectBuilder` → `CreateBuiltinFunction`
/// - `JsObject::from_proto_and_data(proto, OrdinaryObject)` → `OrdinaryObjectCreate`
/// - `JsObject::define_property_or_throw` → `DefinePropertyOrThrow`
///
/// Then defines the interface name on the global object per
/// "define the global property references" (step 3.1.3).
pub(crate) fn register_interface_spec<T: WebIdlInterface>(
    context: &mut Context,
) -> JsResult<()> {
    // §3.7.1: Create interface object (constructor function)
    let interface_obj = create_interface_object::<T>(context)?;

    // "define the global property references", Step 3.1.3:
    // "Perform DefineMethodProperty(target, id, interfaceObject, false)."
    // ≡ DefinePropertyOrThrow(global, id, PropertyDescriptor{...})
    let desc = PropertyDescriptor::builder()
        .value(interface_obj)
        .writable(true)
        .enumerable(false)
        .configurable(true)
        .build();
    context
        .global_object()
        .define_property_or_throw(js_string!(T::NAME), desc, context)?;

    Ok(())
}

/// Default constructor steps per §3.7.1 step 1.
fn create_default_constructor_steps<T: WebIdlInterface>() -> NativeFunction {
    NativeFunction::from_fn_ptr(|_this, _args, _context| {
        // Step 1.1: "If I was not declared with a constructor operation,
        //            then throw a TypeError."
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    })
}

// ─────────────────────────────────────────────────────────────────────────
//  Legacy: Registration via ClassBuilder (for incremental migration)
// ─────────────────────────────────────────────────────────────────────────

/// Register a Web IDL interface using the existing ClassBuilder system.
///
/// This is the compatibility path for incremental migration.  Once all
/// platform objects are migrated, this function should be replaced by
/// `register_interface_spec`.
pub(crate) fn register_interface<T: WebIdlInterface>(
    class: &mut boa_engine::class::ClassBuilder<'_>,
) -> JsResult<()> {
    let mut def = InterfaceDefinition::new();
    T::define_members(&mut def);

    let realm = class.context().realm().clone();

    // ── §3.7.5: Define the constants ──
    define_constants(class, &def.constants);

    // ── §3.7.6: Define the regular attributes ──
    define_regular_attributes(class, &realm, &def.attributes)?;

    // ── §3.7.6: Define the static attributes ──
    define_static_attributes(class, &realm, &def.attributes)?;

    // ── §3.7.6: Define the unforgeable regular attributes ──
    define_unforgeable_regular_attributes(class, &realm, &def.attributes)?;

    // ── §3.7.7: Define the regular operations ──
    define_regular_operations(class, &def.operations)?;

    // ── §3.7.7: Define the static operations ──
    define_static_operations(class, &def.operations)?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

/// Resolve the `this` value per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#attribute-getter step 1.1.2
/// https://webidl.spec.whatwg.org/#create-an-operation-function step 2.1.2
///
/// Uses `ToObject(V)` → `V.to_object(context)` per ECMA-262.
pub(crate) fn resolve_this_value(this: &JsValue, context: &Context) -> JsResult<JsValue> {
    // Step 1.1.2.1: "Let jsValue be the this value, if it is not null or
    //                undefined, or realm's global object otherwise."
    if this.is_null_or_undefined() {
        return Ok(JsValue::from(context.global_object()));
    }

    Ok(this.clone())
}

/// Define global property references per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#define-the-global-property-references
pub(crate) fn define_global_property_references(_context: &mut Context) -> JsResult<()> {
    Ok(())
}
