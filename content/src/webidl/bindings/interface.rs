use boa_engine::{
    Context, JsError, JsNativeError, JsObject, JsResult, JsValue,
    builtins::object::OrdinaryObject,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, NativeObject},
    property::PropertyDescriptor,
};

use js_engine::{
    Completion, ExecutionContext, JsEngine, JsTypes, JsTypesWithRealm,
    PropertyDescriptor as JsPropertyDescriptor,
};

use super::attribute::AttributeDef;
use super::constant::ConstantDef;
use super::operation::OperationDef;

/// A buildable definition of an interface's members.
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

/// Trait for Web IDL platform objects.
///
/// https://webidl.spec.whatwg.org/#js-interfaces
pub(crate) trait WebIdlInterface<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;
    fn parent_name() -> Option<&'static str> {
        None
    }
    fn is_global() -> bool {
        false
    }
    fn no_interface_object() -> bool {
        false
    }
    fn legacy_namespace() -> Option<&'static str> {
        None
    }
    fn constructor_length() -> usize {
        0
    }
    fn immutable_prototype() -> bool {
        Self::is_global()
    }

    fn create_platform_object(
        _new_target: &T::JsValue,
        _args: &[T::JsValue],
        ec: &mut dyn ExecutionContext<T>,
    ) -> Completion<Self, T>
    where
        Self: Sized,
    {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<T>)
    where
        Self: Sized;
}

// ── Generic helpers ──

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
pub(crate) fn create_interface_instance<Ty, T>(
    data: T,
    ec: &mut dyn ExecutionContext<Ty>,
) -> Completion<Ty::JsObject, Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    T: 'static,
{
    let prototype =
        super::registry::get_prototype_from_host_defined::<Ty, T>(ec).ok_or_else(|| {
            ec.new_type_error(&format!(
                "interface not registered: {}",
                std::any::type_name::<T>()
            ))
        })?;
    Ok(ec.create_object_with_any(prototype, Box::new(data)))
}

// ── Concrete registration ──

pub(crate) fn register_interface_spec<Ty, I, E>(engine: &mut E) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: WebIdlInterface<Ty> + 'static,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let realm = engine.current_realm();
    let intrinsics = engine.realm_intrinsics(&realm);
    let proto = engine.create_object_with_any(intrinsics.object_prototype.clone(), Box::new(()));
    let mut def = InterfaceDefinition::<Ty>::new();
    I::define_members(&mut def);
    let proto_val = Ty::value_from_object(proto.clone());
    super::attribute::define_regular_attributes::<Ty, E>(engine, &proto_val, &def.attributes)?;
    super::operation::define_regular_operations::<Ty, E>(engine, &proto_val, &def.operations)?;
    let op_prototype = intrinsics.object_prototype.clone();
    let constructor_fn = engine.create_builtin_function(
        Box::new(
            move |args: &[Ty::JsValue],
                  new_target: Ty::JsValue,
                  ec: &mut dyn ExecutionContext<Ty>| {
                let obj = I::create_platform_object(&new_target, args, ec)?;
                let instance = ec.create_object_with_any(op_prototype.clone(), Box::new(obj));
                Ok(Ty::value_from_object(instance))
            },
        ),
        I::constructor_length() as u32,
        engine.property_key_from_str(I::NAME),
        &realm,
    );
    let f_obj = Ty::object_from_function(constructor_fn);
    let proto_desc = JsPropertyDescriptor {
        value: Some(Ty::value_from_object(proto.clone())),
        writable: Some(false),
        get: None,
        set: None,
        enumerable: Some(false),
        configurable: Some(false),
    };
    engine.define_property_or_throw(
        f_obj.clone(),
        engine.property_key_from_str("prototype"),
        proto_desc,
    )?;
    let ctor_ref = JsPropertyDescriptor {
        value: Some(Ty::value_from_object(f_obj.clone())),
        writable: Some(true),
        get: None,
        set: None,
        enumerable: Some(false),
        configurable: Some(true),
    };
    engine.define_property_or_throw(
        proto.clone(),
        engine.property_key_from_str("constructor"),
        ctor_ref,
    )?;
    let f_val = Ty::value_from_object(f_obj.clone());
    super::attribute::define_static_attributes::<Ty, E>(engine, &f_val, &def.attributes)?;
    super::operation::define_static_operations::<Ty, E>(engine, &f_val, &def.operations)?;
    super::registry::register_in_host_defined::<Ty, I>(engine, proto.clone(), f_obj.clone());
    let install_desc = JsPropertyDescriptor {
        value: Some(Ty::value_from_object(f_obj)),
        writable: Some(true),
        get: None,
        set: None,
        enumerable: Some(false),
        configurable: Some(true),
    };
    if let Some(ns_name) = I::legacy_namespace() {
        let go = engine.global_object();
        let key = engine.property_key_from_str(ns_name);
        let ns_val = ExecutionContext::get(&mut *engine, go, key)?;
        let ns_obj = Ty::value_as_object(&ns_val).ok_or_else(|| {
            engine.new_type_error(&format!(
                "interface {}: namespace '{}' not found",
                I::NAME,
                ns_name
            ))
        })?;
        engine.define_property_or_throw(
            ns_obj,
            engine.property_key_from_str(I::NAME),
            install_desc,
        )?;
    } else {
        engine.define_property_or_throw(
            engine.global_object(),
            engine.property_key_from_str(I::NAME),
            install_desc,
        )?;
    }
    Ok(())
}

pub(crate) fn define_global_property_references<Ty: JsTypes>(
    _ec: &mut dyn ExecutionContext<Ty>,
) -> Completion<(), Ty> {
    Ok(())
}

// ── Namespace trait + registration ──

pub(crate) trait WebIdlNamespace<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;
    fn define_members(def: &mut InterfaceDefinition<T>)
    where
        Self: Sized;
}

pub(crate) fn register_namespace_spec<Ty, I, E>(engine: &mut E) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: WebIdlNamespace<Ty> + 'static,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let realm = engine.current_realm();
    let intrinsics = engine.realm_intrinsics(&realm);
    let ns_obj = engine.create_object_with_any(intrinsics.object_prototype, Box::new(()));
    let mut def = InterfaceDefinition::<Ty>::new();
    I::define_members(&mut def);
    let ns_val = Ty::value_from_object(ns_obj.clone());
    super::attribute::define_regular_attributes::<Ty, E>(engine, &ns_val, &def.attributes)?;
    super::operation::define_regular_operations::<Ty, E>(engine, &ns_val, &def.operations)?;
    let desc = JsPropertyDescriptor {
        value: Some(ns_val),
        writable: Some(true),
        get: None,
        set: None,
        enumerable: Some(false),
        configurable: Some(true),
    };
    engine.define_property_or_throw(
        engine.global_object(),
        engine.property_key_from_str(I::NAME),
        desc,
    )?;
    Ok(())
}
