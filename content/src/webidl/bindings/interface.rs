use boa_engine::{
    builtins::object::OrdinaryObject,
    js_string,
    native_function::NativeFunction,
    object::{builtins::JsFunction, FunctionObjectBuilder, NativeObject},
    property::PropertyDescriptor,
    Context, JsError, JsNativeError, JsObject, JsResult, JsValue,
};

use js_engine::{JsTypes, JsTypesWithRealm};

use super::attribute::AttributeDef;
use super::constant::ConstantDef;
use super::operation::OperationDef;

/// A buildable definition of an interface's members, collected by
/// `WebIdlInterface::define_members`.
///
/// Generic over `T: JsTypes` so that member-defining function pointers use
/// the engine's native types (`T::JsValue`, `T::JsObject`, etc.) rather
/// than Boa-concrete types.
pub(crate) struct InterfaceDefinition<T: JsTypes> {
    pub(crate) attributes: Vec<AttributeDef<T>>,
    pub(crate) operations: Vec<OperationDef<T>>,
    pub(crate) constants: Vec<ConstantDef<T>>,
}

impl<T: JsTypes> InterfaceDefinition<T> {
    pub(crate) fn new() -> Self {
        Self {
            attributes: Vec::new(),
            operations: Vec::new(),
            constants: Vec::new(),
        }
    }

    pub(crate) fn add_attribute(&mut self, attr: AttributeDef<T>) {
        self.attributes.push(attr);
    }

    pub(crate) fn add_operation(&mut self, op: OperationDef<T>) {
        self.operations.push(op);
    }

    pub(crate) fn add_constant(&mut self, const_: ConstantDef<T>) {
        self.constants.push(const_);
    }
}

/// Trait for Web IDL platform objects that wish to expose a JavaScript
/// binding following the Web IDL specification.
///
/// Parameterized over `T: JsTypes` so that `define_members` collects
/// member definitions with engine-generic fn pointer types.
/// `create_platform_object` remains concrete (Boa-specific) for now —
/// it will migrate to `&mut dyn ExecutionContext<T>` in Phase 3.
///
/// https://webidl.spec.whatwg.org/#js-interfaces
pub(crate) trait WebIdlInterface<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;

    fn parent_name() -> Option<&'static str> { None }
    fn is_global() -> bool { false }
    fn no_interface_object() -> bool { false }

    fn legacy_namespace() -> Option<&'static str> { None }
    fn constructor_length() -> usize { 0 }
    fn supports_named_properties() -> bool { false }
    fn supports_indexed_properties() -> bool { false }
    fn immutable_prototype() -> bool { Self::is_global() }

    /// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
    ///
    /// Note: Currently Boa-concrete.  Will accept `&mut dyn ExecutionContext<T>`
    /// in Phase 3 (domain threading).
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

    fn define_members(def: &mut InterfaceDefinition<T>)
    where
        Self: Sized;
}

// ─────────────────────────────────────────────────────────────────────────
//  Spec-Aligned Registration (§3.7.1 + §3.7.3)
//  Monomorphized for BoaTypes.
// ─────────────────────────────────────────────────────────────────────────

/// <https://webidl.spec.whatwg.org/#create-an-interface-prototype-object>
pub(crate) fn create_interface_prototype_object<T: WebIdlInterface<js_engine::boa::BoaTypes>>(
    context: &mut Context,
) -> JsResult<JsObject> {
    let mut proto: Option<JsObject> = None;

    if T::parent_name().is_some() {
        proto = None;
    }

    let proto = proto.unwrap_or_else(|| context.intrinsics().constructors().object().prototype());

    let interface_proto_obj = if T::immutable_prototype() {
        JsObject::from_proto_and_data(Some(proto.clone()), OrdinaryObject)
    } else {
        JsObject::from_proto_and_data(Some(proto), OrdinaryObject)
    };

    Ok(interface_proto_obj)
}

/// <https://webidl.spec.whatwg.org/#create-an-interface-object>
pub(crate) fn create_interface_object<T: WebIdlInterface<js_engine::boa::BoaTypes> + NativeObject>(
    context: &mut Context,
) -> JsResult<JsFunction> {
    let realm = context.realm().clone();
    let steps = create_default_constructor_steps::<T>();
    let length: usize = 0;

    let f = FunctionObjectBuilder::new(&realm, steps)
        .name(T::NAME)
        .length(length)
        .constructor(true)
        .build();

    let f_obj: JsObject = f.clone().into();
    let proto = create_interface_prototype_object::<T>(context)?;

    let prototype_desc = PropertyDescriptor::builder()
        .value(proto.clone())
        .writable(false)
        .enumerable(false)
        .configurable(false)
        .build();
    f_obj.define_property_or_throw(js_string!("prototype"), prototype_desc, context)?;

    Ok(f)
}

/// Register a Web IDL interface in the HostDefined registry.
///
/// <https://webidl.spec.whatwg.org/#create-an-interface-object>
/// <https://webidl.spec.whatwg.org/#create-an-interface-prototype-object>
///
/// Monomorphized for `BoaTypes`.  The definition is built with
/// `InterfaceDefinition<BoaTypes>` (which carries generic fn pointers),
/// then the member-defining functions convert those to Boa `NativeFunction`
/// via the internal `op_method_to_native` / `attr_getter_to_native` helpers
/// in `operation.rs` and `attribute.rs`.
pub(crate) fn register_interface_spec<T: WebIdlInterface<js_engine::boa::BoaTypes> + NativeObject>(
    context: &mut Context,
) -> JsResult<()> {
    let realm = context.realm().clone();

    // ── §3.7.3: Create interface prototype object ──
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        OrdinaryObject,
    );

    // Define members on the prototype per §3.7.6, §3.7.7, §3.7.5
    let mut def = InterfaceDefinition::<js_engine::boa::BoaTypes>::new();
    T::define_members(&mut def);

    super::attribute::define_regular_attributes(
        &JsValue::from(proto.clone()),
        context,
        &def.attributes,
    )?;
    super::operation::define_regular_operations(
        &JsValue::from(proto.clone()),
        context,
        &def.operations,
    )?;
    super::constant::define_constants(&proto, context, &def.constants)?;

    // ── §3.7.1: Create interface object (constructor) ──
    let constructor = {
        let f = FunctionObjectBuilder::new(
            &realm,
            NativeFunction::from_fn_ptr(
                |new_target: &JsValue, args: &[JsValue], ctx: &mut Context| {
                    let obj = T::create_platform_object(new_target, args, ctx)?;
                    let proto = resolve_instance_prototype(new_target, ctx);
                    let instance = match proto {
                        Some(p) => JsObject::from_proto_and_data(Some(p), obj),
                        None => create_interface_instance(obj, ctx)?,
                    };
                    Ok(JsValue::from(instance))
                },
            ),
        )
        .name(T::NAME)
        .length(T::constructor_length())
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

        // Step 14: "Define the static attributes of interface I on F given realm."
        super::attribute::define_static_attributes(
            &JsValue::from(f_obj.clone()),
            context,
            &def.attributes,
        )?;

        // Step 15: "Define the static operations of interface I on F given realm."
        super::operation::define_static_operations(
            &JsValue::from(f_obj.clone()),
            context,
            &def.operations,
        )?;

        f_obj
    };

    // Store in HostDefined registry
    super::registry::register_in_host_defined::<T>(context, proto, constructor.clone());

    // §3.13.1 Namespace object, Step 5: [LegacyNamespace]
    let desc = PropertyDescriptor::builder()
        .value(constructor)
        .writable(true)
        .enumerable(false)
        .configurable(true)
        .build();
    if let Some(ns_name) = T::legacy_namespace() {
        let ns_val = context.global_object().get(js_string!(ns_name), context)?;
        let ns_obj = ns_val.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message(format!(
                "interface {}: namespace '{}' not found",
                T::NAME,
                ns_name
            ))
        })?;
        ns_obj.define_property_or_throw(js_string!(T::NAME), desc, context)?;
    } else {
        context
            .global_object()
            .define_property_or_throw(js_string!(T::NAME), desc, context)?;
    }

    Ok(())
}

fn resolve_instance_prototype(new_target: &JsValue, context: &mut Context) -> Option<JsObject> {
    let nt = new_target.as_object()?;
    let proto_val = nt.get(js_string!("prototype"), context).ok()?;
    proto_val.as_object().map(|o| o.clone())
}

fn create_default_constructor_steps<T: WebIdlInterface<js_engine::boa::BoaTypes>>() -> NativeFunction {
    NativeFunction::from_fn_ptr(|_this, _args, _context| {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    })
}

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
pub(crate) fn create_interface_instance<T>(data: T, context: &mut Context) -> JsResult<JsObject>
where
    T: NativeObject + 'static,
{
    let prototype =
        super::registry::get_prototype_from_host_defined::<T>(context).ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message(format!(
                "interface not registered: {}",
                std::any::type_name::<T>()
            )))
        })?;
    Ok(JsObject::from_proto_and_data(Some(prototype), data))
}

/// Implements the `this`-value resolution step from attribute getter and
/// operation function creation algorithms.
///
/// <https://webidl.spec.whatwg.org/#js-attributes> — attribute getter Step 1.1.2.1
/// <https://webidl.spec.whatwg.org/#js-operations> — creating an operation function Step 2.1.2.1
pub(crate) fn resolve_this_value(this: &JsValue, context: &Context) -> JsResult<JsValue> {
    if this.is_null_or_undefined() {
        return Ok(JsValue::from(context.global_object()));
    }
    Ok(this.clone())
}

/// <https://webidl.spec.whatwg.org/#define-the-global-property-references>
pub(crate) fn define_global_property_references(_context: &mut Context) -> JsResult<()> {
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
//  Namespace trait + registration
// ─────────────────────────────────────────────────────────────────────────

/// Trait for Web IDL namespace objects.
///
/// https://webidl.spec.whatwg.org/#namespace
pub(crate) trait WebIdlNamespace<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;

    fn define_members(def: &mut InterfaceDefinition<T>)
    where
        Self: Sized;
}

/// <https://webidl.spec.whatwg.org/#create-a-namespace-object>
pub(crate) fn register_namespace_spec<T: WebIdlNamespace<js_engine::boa::BoaTypes>>(
    context: &mut Context,
) -> JsResult<()> {
    let namespace = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        OrdinaryObject,
    );

    let mut def = InterfaceDefinition::<js_engine::boa::BoaTypes>::new();
    T::define_members(&mut def);

    super::attribute::define_regular_attributes(
        &JsValue::from(namespace.clone()),
        context,
        &def.attributes,
    )?;
    super::operation::define_regular_operations(
        &JsValue::from(namespace.clone()),
        context,
        &def.operations,
    )?;

    let desc = PropertyDescriptor::builder()
        .value(namespace)
        .writable(true)
        .enumerable(false)
        .configurable(true)
        .build();
    context
        .global_object()
        .define_property_or_throw(js_string!(T::NAME), desc, context)?;

    Ok(())
}
