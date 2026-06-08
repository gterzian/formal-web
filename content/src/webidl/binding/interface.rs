use boa_engine::{
    Context, JsError, JsNativeError, JsObject, JsResult, JsValue,
    builtins::object::OrdinaryObject,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, NativeObject, builtins::JsFunction},
    property::PropertyDescriptor,
};

use crate::js::JsEngine;

use super::attribute::AttributeDef;
use super::constant::ConstantDef;
use super::operation::OperationDef;

/// A buildable definition of an interface's members, collected by
/// `WebIdlInterface::define_members`.
///
/// Collects the interface's attributes, operations, and constants.  Call `add_attribute`,
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
    ///
    /// `new_target` is the `new.target` value from the constructor call
    /// (§3.7.1 step 1.2), or `undefined` if called as a function.
    fn create_platform_object(
        _new_target: &JsValue,
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
    // Step 1: "Let proto be null."
    let mut proto: Option<JsObject> = None;

    // Step 2: "If interface is declared with the [Global] extended attribute,
    //          and interface supports named properties,"
    // Note: Named properties object creation is not yet implemented.

    // Step 3: "Otherwise, if interface is declared to inherit from another
    //          interface, then set proto to the interface prototype object
    //          in realm of that inherited interface."
    if T::parent_name().is_some() {
        // Note: Falls through to %Object.prototype% until parent lookup is implemented.
        proto = None;
    }

    // Step 4: "Otherwise, if interface is the DOMException interface,
    //          then set proto to realm.[[Intrinsics]].[[%Error.prototype%]]."
    // Note: DOMException is handled in the DOMException binding directly.

    // Step 5: "Otherwise, set proto to realm.[[Intrinsics]].[[%Object.prototype%]]."
    let proto = proto.unwrap_or_else(|| {
        context.intrinsics().constructors().object().prototype()
    });

    // Step 6: "Assert: proto is an Object."
    debug_assert!(true, "proto was set in steps 1-5");

    // Step 7: "Let interfaceProtoObj be null."
    let interface_proto_obj = if T::immutable_prototype() {
        // Step 9: "If interface is declared with the [Global] extended attribute, ..."
        // Step 9.1: "Set interfaceProtoObj to MakeBasicObject(« [[Prototype]], [[Extensible]] »)."
        JsObject::from_proto_and_data(Some(proto.clone()), OrdinaryObject)
        // TODO: Step 9.3: Set [[SetPrototypeOf]] to SetImmutablePrototype behavior.
    } else {
        // Step 8: "If realm's is global prototype chain mutable is true, then:"
        // Step 8.1: "Set interfaceProtoObj to OrdinaryObjectCreate(proto)."
        // Step 10: "Otherwise, set interfaceProtoObj to OrdinaryObjectCreate(proto)."
        JsObject::from_proto_and_data(Some(proto), OrdinaryObject)
    };

    // Step 11: "If interface has any member declared with the [Unscopable]
    //          extended attribute, then:"
    // Note: [Unscopable] is not yet implemented.

    // Step 12: "If interface is not declared with the [Global] extended attribute, then:"
    if !T::is_global() {
        // Step 12.1: "Define the regular attributes of interface on
        //             interfaceProtoObj given realm."
        // Step 12.2: "Define the regular operations of interface on
        //             interfaceProtoObj given realm."
        // Step 12.3: "Define the iteration methods of interface on
        //             interfaceProtoObj given realm."
        // Step 12.4: "Define the asynchronous iteration methods of interface on
        //             interfaceProtoObj given realm."
        // Note: Members are defined by `register_interface_spec` after calling
        // this function, via `define_regular_attributes`, `define_regular_operations`,
        // and `define_constants`. Iteration methods are not yet implemented.
    }

    // Step 13: "Define the constants of interface on interfaceProtoObj given realm."
    // Note: Constants are defined by `register_interface_spec` after this function.

    // Step 14: "If the [LegacyNoInterfaceObject] extended attribute was not specified
    //          on interface, then:"
    // Step 14.1: "Let constructor be the interface object of interface in realm."
    // Step 14.2-3: Wire `constructor` property.
    // Note: The constructor property is wired by `register_interface_spec`.

    // Step 15: "Return interfaceProtoObj."
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

    // Step 1: "Let steps be I's overridden constructor steps if they exist, or
    //          the following steps otherwise:"
    let steps = create_default_constructor_steps::<T>();
    // Note: The default steps (1.1-1.13) throw a TypeError.  Overridden
    // constructor steps are provided via WebIdlInterface::create_platform_object
    // and are wrapped in the NativeFunction closure passed to FunctionObjectBuilder.

    // Step 2: "Let constructorProto be realm.[[Intrinsics]].[[%Function.prototype%]]."
    let _constructor_proto = context.intrinsics().constructors().function().prototype();

    // Step 3: "If I inherits from some other interface P, then set constructorProto
    //          to the interface object of P in realm."
    // Note: Parent interface object lookup is not yet implemented.

    // Step 4: "Let unforgeables be OrdinaryObjectCreate(null)."
    let _unforgeables = JsObject::from_proto_and_data(None, OrdinaryObject);

    // Step 5: "Define the unforgeable regular operations of I on unforgeables, given realm."
    // Note: Unforgeable operations are not yet defined on the unforgeables object.

    // Step 6: "Define the unforgeable regular attributes of I on unforgeables, given realm."
    // Note: Unforgeable attributes are not yet defined on the unforgeables object.

    // Step 7: "Set F.[[Unforgeables]] to unforgeables."
    // Note: The [[Unforgeables]] slot is set implicitly by FunctionObjectBuilder.

    // Step 8: "Let length be 0."
    // Step 9: "If I was declared with a constructor operation, then ... Set length to the
    //          length of the shortest argument list of the entries in S."
    let length: usize = 0;
    // Note: Overload-set length computation is not yet implemented; defaults to 0.

    // Step 10: "Let F be CreateBuiltinFunction(steps, length, id, « [[Unforgeables]] »,
    //           realm, constructorProto)."
    let f = FunctionObjectBuilder::new(&realm, steps)
        .name(T::NAME)
        .length(length)
        .constructor(true)
        .build();

    let f_obj: JsObject = f.clone().into();

    // Step 11: "Let proto be the result of creating an interface prototype object
    //          of interface I in realm."
    let proto = create_interface_prototype_object::<T>(context)?;

    // Step 12: "Perform ! DefinePropertyOrThrow(F, "prototype",
    //           PropertyDescriptor{[[Value]]: proto, [[Writable]]: false,
    //           [[Enumerable]]: false, [[Configurable]]: false})."
    let prototype_desc = PropertyDescriptor::builder()
        .value(proto.clone())
        .writable(false)
        .enumerable(false)
        .configurable(false)
        .build();
    f_obj.define_property_or_throw(js_string!("prototype"), prototype_desc, context)?;

    // Step 13: "Define the constants of interface I on F given realm."
    // Note: Constants are defined by `register_interface_spec` after this function.

    // Step 14: "Define the static attributes of interface I on F given realm."
    // Note: Static attributes are not yet implemented.

    // Step 15: "Define the static operations of interface I on F given realm."
    // Note: Static operations are not yet implemented.

    // Step 16: "Return F."
    // Note: The caller (register_interface_spec) also wires proto.constructor = F.
    Ok(f)
}

/// <https://webidl.spec.whatwg.org/#create-an-interface-object>
/// <https://webidl.spec.whatwg.org/#create-an-interface-prototype-object>
///
/// Registers a Web IDL interface in the HostDefined registry.
/// Creates the interface prototype object (§3.7.3) via `OrdinaryObjectCreate`,
/// defines members on it (§3.7.5–3.7.7), creates the constructor (§3.7.1) via
/// `CreateBuiltinFunction`, wires `F.prototype = proto`, stores both in the
/// registry, and defines the constructor on the global object.
pub(crate) fn register_interface_spec<T>(context: &mut Context) -> JsResult<()>
where
    T: WebIdlInterface + NativeObject,
{
    let realm = context.realm().clone();

    // ── §3.7.3: Create interface prototype object ──
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        OrdinaryObject,
    );

    // Define members on the prototype per §3.7.6, §3.7.7, §3.7.5
    let mut def = InterfaceDefinition::new();
    T::define_members(&mut def);

    super::attribute::define_regular_attributes(&proto, context, &def.attributes)?;
    super::operation::define_regular_operations(&proto, context, &def.operations)?;
    super::constant::define_constants(&proto, context, &def.constants)?;

    // ── §3.7.1: Create interface object (constructor) ──
    // https://webidl.spec.whatwg.org/#create-an-interface-object
    //
    // §3.8: "internally create a new object implementing the interface"
    //   Step: "Let prototype be ? Get(newTarget, "prototype")."
    let constructor = {
        let f = FunctionObjectBuilder::new(
            &realm,
            NativeFunction::from_fn_ptr(|new_target: &JsValue, args: &[JsValue], ctx: &mut Context| {
                let obj = T::create_platform_object(new_target, args, ctx)?;
                // §3.8 step: Get(newTarget, "prototype")
                let proto = resolve_instance_prototype(new_target, ctx);
                let instance = match proto {
                    Some(p) => JsObject::from_proto_and_data(Some(p), obj),
                    None => create_interface_instance_ctx(obj, ctx)?,
                };
                Ok(JsValue::from(instance))
            }),
        )
        .name(T::NAME)
        .length(1)
        .constructor(true)
        .build();
        let f_obj: JsObject = f.clone().into();

        // Wire F.prototype = proto
        let proto_desc = PropertyDescriptor::builder()
            .value(proto.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false)
            .build();
        f_obj.define_property_or_throw(js_string!("prototype"), proto_desc, context)?;

        // Wire proto.constructor = F
        let ctor_desc = PropertyDescriptor::builder()
            .value(f_obj.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build();
        proto.define_property_or_throw(js_string!("constructor"), ctor_desc, context)?;

        // §3.7.5: Constants on the constructor too
        super::constant::define_constants(&f_obj, context, &def.constants)?;

        f_obj
    };

    // Store in HostDefined registry
    super::registry::register_in_host_defined::<T>(context, proto, constructor.clone());

    // Define on global object
    let desc = PropertyDescriptor::builder()
        .value(constructor)
        .writable(true)
        .enumerable(false)
        .configurable(true)
        .build();
    context
        .global_object()
        .define_property_or_throw(js_string!(T::NAME), desc, context)?;

    Ok(())
}

/// Resolve the instance prototype per §3.8 step "Get(newTarget, "prototype")".
///
/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
fn resolve_instance_prototype(
    new_target: &JsValue,
    context: &mut Context,
) -> Option<JsObject> {
    let nt = new_target.as_object()?;
    let proto_val = nt.get(js_string!("prototype"), context).ok()?;
    proto_val.as_object().map(|o| o.clone())
}

/// Default constructor steps per §3.7.1 step 1.
fn create_default_constructor_steps<T: WebIdlInterface>() -> NativeFunction {
    NativeFunction::from_fn_ptr(|_this, _args, _context| {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    })
}

/// Create a JsObject instance of interface T from Rust data.
///
/// https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface
///
/// ECMAScript → Boa: `MakeBasicObject(« [[Prototype]], … »)` →
/// `JsObject::from_proto_and_data(prototype, data)`
pub(crate) fn create_interface_instance<T>(data: T, engine: &mut impl JsEngine) -> JsResult<JsObject>
where
    T: NativeObject + 'static,
{
    let prototype = super::registry::get_prototype_from_host_defined_engine::<T>(engine)
        .ok_or_else(|| {
            JsError::from(JsNativeError::typ()
                .with_message(format!("interface not registered: {}", std::any::type_name::<T>())))
        })?;
    Ok(JsObject::from_proto_and_data(Some(prototype), data))
}

/// Context-based variant for use inside legacy `js/bindings/` code and
/// NativeFunction closures that receive `&mut Context`.
pub(crate) fn create_interface_instance_ctx<T>(data: T, context: &mut Context) -> JsResult<JsObject>
where
    T: NativeObject + 'static,
{
    let prototype = super::registry::get_prototype_from_host_defined::<T>(context)
        .ok_or_else(|| {
            JsError::from(JsNativeError::typ()
                .with_message(format!("interface not registered: {}", std::any::type_name::<T>())))
        })?;
    Ok(JsObject::from_proto_and_data(Some(prototype), data))
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

/// Implements the `this`-value resolution step shared by the attribute getter
/// and operation function creation algorithms.
///
/// <https://webidl.spec.whatwg.org/#js-attributes> — attribute getter Step 1.1.2.1
/// <https://webidl.spec.whatwg.org/#js-operations> — creating an operation function Step 2.1.2.1
///
/// Both algorithms say:
/// "Let jsValue be the this value, if it is not null or undefined, or realm's
/// global object otherwise."
pub(crate) fn resolve_this_value(this: &JsValue, engine: &impl JsEngine) -> JsResult<JsValue> {
    if this.is_null_or_undefined() {
        return Ok(JsValue::from(engine.global_object()));
    }

    Ok(this.clone())
}

/// Define global property references per the Web IDL spec.
///
/// https://webidl.spec.whatwg.org/#define-the-global-property-references
pub(crate) fn define_global_property_references(_context: &mut Context) -> JsResult<()> {
    Ok(())
}
