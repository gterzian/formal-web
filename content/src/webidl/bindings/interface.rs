use boa_engine::{
    builtins::object::OrdinaryObject,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, NativeObject},
    property::PropertyDescriptor,
    Context, JsError, JsNativeError, JsObject, JsResult, JsValue,
};

use js_engine::{JsTypes, JsTypesWithRealm};

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
        Self { attributes: Vec::new(), operations: Vec::new(), constants: Vec::new() }
    }
    pub(crate) fn add_attribute(&mut self, attr: AttributeDef<T>) { self.attributes.push(attr); }
    pub(crate) fn add_operation(&mut self, op: OperationDef<T>) { self.operations.push(op); }
    pub(crate) fn add_constant(&mut self, const_: ConstantDef<T>) { self.constants.push(const_); }
}

/// Trait for Web IDL platform objects.
///
/// https://webidl.spec.whatwg.org/#js-interfaces
pub(crate) trait WebIdlInterface<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;
    fn parent_name() -> Option<&'static str> { None }
    fn is_global() -> bool { false }
    fn no_interface_object() -> bool { false }
    fn legacy_namespace() -> Option<&'static str> { None }
    fn constructor_length() -> usize { 0 }
    fn immutable_prototype() -> bool { Self::is_global() }

    fn create_platform_object(
        _new_target: &JsValue, _args: &[JsValue], _context: &mut Context,
    ) -> JsResult<Self> where Self: Sized {
        Err(JsNativeError::typ().with_message("Illegal constructor").into())
    }

    fn define_members(def: &mut InterfaceDefinition<T>) where Self: Sized;
}

// ── Generic helpers ──

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
///
/// Uses the generic registry (which stores `Ty::JsObject` via EC host data store).
pub(crate) fn create_interface_instance<T>(data: T, context: &mut Context) -> JsResult<JsObject>
where T: NativeObject + 'static {
    let prototype = super::registry::get_prototype_from_host_defined::<js_engine::boa::BoaTypes, T>(
        crate::js::context_as_ec_ref(context),
    ).ok_or_else(|| JsError::from(JsNativeError::typ().with_message(
        format!("interface not registered: {}", std::any::type_name::<T>())
    )))?;
    Ok(JsObject::from_proto_and_data(Some(prototype), data))
}

// ── Concrete registration ──

pub(crate) fn register_interface_spec<T: WebIdlInterface<js_engine::boa::BoaTypes> + NativeObject>(
    context: &mut Context,
) -> JsResult<()> {
    let realm = context.realm().clone();
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()), OrdinaryObject,
    );
    let mut def = InterfaceDefinition::<js_engine::boa::BoaTypes>::new();
    T::define_members(&mut def);
    super::attribute::define_regular_attributes(&JsValue::from(proto.clone()), context, &def.attributes)?;
    super::operation::define_regular_operations(&JsValue::from(proto.clone()), context, &def.operations)?;
    super::constant::define_constants(&proto, context, &def.constants)?;

    let constructor = {
        let f = FunctionObjectBuilder::new(&realm, NativeFunction::from_fn_ptr(
            |new_target: &JsValue, args: &[JsValue], ctx: &mut Context| {
                let obj = T::create_platform_object(new_target, args, ctx)?;
                let instance_proto = (|| {
                    let nt = new_target.as_object()?;
                    let proto_val = nt.get(js_string!("prototype"), ctx).ok()?;
                    proto_val.as_object().map(|o| o.clone())
                })();
                let instance = match instance_proto {
                    Some(p) => JsObject::from_proto_and_data(Some(p), obj),
                    None => create_interface_instance(obj, ctx)?,
                };
                Ok(JsValue::from(instance))
            },
        )).name(T::NAME).length(T::constructor_length()).constructor(true).build();
        let f_obj: JsObject = f.clone().into();
        let proto_desc = PropertyDescriptor::builder()
            .value(proto.clone()).writable(false).enumerable(false).configurable(false).build();
        f_obj.define_property_or_throw(js_string!("prototype"), proto_desc, context)?;
        let ctor_desc = PropertyDescriptor::builder()
            .value(f_obj.clone()).writable(true).enumerable(false).configurable(true).build();
        proto.define_property_or_throw(js_string!("constructor"), ctor_desc, context)?;
        super::constant::define_constants(&f_obj, context, &def.constants)?;
        super::attribute::define_static_attributes(&JsValue::from(f_obj.clone()), context, &def.attributes)?;
        super::operation::define_static_operations(&JsValue::from(f_obj.clone()), context, &def.operations)?;
        f_obj
    };

    // Uses generic registry
    super::registry::register_in_host_defined::<js_engine::boa::BoaTypes, T>(
        crate::js::context_as_ec(context), proto, constructor.clone(),
    );

    let desc = PropertyDescriptor::builder()
        .value(constructor).writable(true).enumerable(false).configurable(true).build();
    if let Some(ns_name) = T::legacy_namespace() {
        let ns_val = context.global_object().get(js_string!(ns_name), context)?;
        let ns_obj = ns_val.as_object().ok_or_else(|| JsNativeError::typ().with_message(
            format!("interface {}: namespace '{}' not found", T::NAME, ns_name)
        ))?;
        ns_obj.define_property_or_throw(js_string!(T::NAME), desc, context)?;
    } else {
        context.global_object().define_property_or_throw(js_string!(T::NAME), desc, context)?;
    }
    Ok(())
}

pub(crate) fn resolve_this_value(this: &JsValue, context: &Context) -> JsResult<JsValue> {
    if this.is_null_or_undefined() { Ok(JsValue::from(context.global_object())) }
    else { Ok(this.clone()) }
}

pub(crate) fn define_global_property_references(_context: &mut Context) -> JsResult<()> { Ok(()) }

// ── Namespace trait + registration ──

pub(crate) trait WebIdlNamespace<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;
    fn define_members(def: &mut InterfaceDefinition<T>) where Self: Sized;
}

pub(crate) fn register_namespace_spec<T: WebIdlNamespace<js_engine::boa::BoaTypes>>(
    context: &mut Context,
) -> JsResult<()> {
    let namespace = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()), OrdinaryObject,
    );
    let mut def = InterfaceDefinition::<js_engine::boa::BoaTypes>::new();
    T::define_members(&mut def);
    super::attribute::define_regular_attributes(&JsValue::from(namespace.clone()), context, &def.attributes)?;
    super::operation::define_regular_operations(&JsValue::from(namespace.clone()), context, &def.operations)?;
    let desc = PropertyDescriptor::builder()
        .value(namespace).writable(true).enumerable(false).configurable(true).build();
    context.global_object().define_property_or_throw(js_string!(T::NAME), desc, context)?;
    Ok(())
}
