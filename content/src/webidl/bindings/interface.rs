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
///
/// Create a platform object implementing the interface identified by type `T`,
/// wrapping the domain Rust data `T` into a JS object.
///
/// This function implements the Web IDL spec algorithm and separates the GC
/// concern: the Boa backend wraps platform data in `TraceableBox` before
/// type-erasing through `Box<dyn Any>`, preserving GC trace/finalize function
/// pointers.  The `create_interface_instance` function carries the `Trace +
/// Finalize + JsData` bounds because all platform object types use `#[gc_struct]`
/// which derives them.
///
/// Use `create_interface_instance` for domain-created platform objects.
/// The constructor path in `register_interface_spec` follows a similar pattern.
#[cfg(feature = "boa")]
pub(crate) fn create_interface_instance<Ty, T>(
    data: T,
    ec: &mut dyn ExecutionContext<Ty>,
) -> Completion<Ty::JsObject, Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    T: std::any::Any + boa_gc::Trace + boa_gc::Finalize + boa_engine::JsData + 'static,
{
    // <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>

    // Step 1: "Assert: interface is exposed in realm."
    let prototype =
        super::registry::get_prototype_from_host_defined::<Ty, T>(ec).ok_or_else(|| {
            ec.new_type_error(&format!(
                "interface not registered: {}",
                std::any::type_name::<T>()
            ))
        })?;

    // Step 2: "If newTarget is undefined, then:"
    //   Domain callers (not constructors) always use the standard prototype.

    // Steps 3-8: newTarget handling, MakeBasicObject — handled below.

    // Step 9: "Set instance.[[Prototype]] to prototype."
    //   Wrap data in TraceableBox for GC root tracing, then type-erase
    //   through create_object_with_any.  The Boa backend recovers the
    //   trace/finalize function pointers from the TraceableBox.
    let boxed = js_engine::boa::TraceableBox::new(data);
    let instance = ec.create_object_with_any(prototype, Box::new(boxed));

    // Steps 10-11: "Let interfaces be the inclusive inherited interfaces..."
    //   TODO: Unforgeable property copying from ancestor interface objects.

    // Step 12: "If interface is declared with the [Global] extended attribute..."
    //   [Global] handling is done during registration (see register_interface_spec).

    // Step 13: "Otherwise, if interfaces contains an interface which supports
    //   indexed properties, named properties, or both:"
    //   Not yet implemented.

    // Step 14: "Return instance."
    Ok(instance)
}

/// Non-Boa backend (JSC): no Boa GC concerns.  Store data as `Box<dyn Any>`.
#[cfg(not(feature = "boa"))]
pub(crate) fn create_interface_instance<Ty, T>(
    data: T,
    ec: &mut dyn ExecutionContext<Ty>,
) -> Completion<Ty::JsObject, Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    T: 'static,
{
    // <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>

    // Step 1: Assert interface is exposed.
    let prototype =
        super::registry::get_prototype_from_host_defined::<Ty, T>(ec).ok_or_else(|| {
            ec.new_type_error(&format!(
                "interface not registered: {}",
                std::any::type_name::<T>()
            ))
        })?;

    // Step 2-3: newTarget undefined for domain callers — standard prototype.

    // Steps 4-9: MakeBasicObject with prototype.  JSC stores Box<dyn Any>.
    let instance = ec.create_object_with_any(prototype, Box::new(data));

    // Steps 10-13: TODO — unforgeables, [Global], indexed/named properties.

    // Step 14: Return instance.
    Ok(instance)
}

// ── Concrete registration ──

/// Boa backend: wrap platform-object data in `TraceableBox` so the GC
/// can trace through type-erased storage, preventing premature collection
/// of `JsObject` references inside constructor-created instances.
///
/// All WebIdlInterface implementors use `#[gc_struct]` which derives
/// `Trace + Finalize + JsData`, satisfying the extra bounds.
#[cfg(feature = "boa")]
pub(crate) fn register_interface_spec<Ty, I, E>(engine: &mut E) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: WebIdlInterface<Ty> + boa_gc::Trace + boa_gc::Finalize + boa_engine::JsData + 'static,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    // <https://webidl.spec.whatwg.org/#create-an-interface-object>
    // Step 2: Let constructorProto be realm.[[Intrinsics]].[[%Function.prototype%]].
    // Step 3: If I inherits ... (handled via constructorProto wiring).
    let realm = engine.current_realm();
    let intrinsics = engine.realm_intrinsics(&realm);
    // Step 11: Let proto be the result of creating an interface prototype
    //   object of interface I in realm.
    let proto = engine.create_object_with_any(intrinsics.object_prototype.clone(), Box::new(()));
    let mut def = InterfaceDefinition::<Ty>::new();
    I::define_members(&mut def);
    let proto_val = Ty::value_from_object(proto.clone());
    super::attribute::define_regular_attributes::<Ty, E>(engine, &proto_val, &def.attributes)?;
    super::operation::define_regular_operations::<Ty, E>(engine, &proto_val, &def.operations)?;
    // The constructor's instances should have the interface's prototype
    // object (proto) in their prototype chain, not Object.prototype.
    // Clone proto now before it's moved into the descriptor assignments.
    // <https://webidl.spec.whatwg.org/#ref-for-define-the-constants①>
    // Define the constants on the interface prototype object as well.
    super::constant::define_constants::<Ty>(proto.clone(), engine, &def.constants)?;
    let instance_prototype = proto.clone();
    let constructor_fn = engine.create_constructor(
        Box::new(
            move |args: &[Ty::JsValue],
                  new_target_or_this: Ty::JsValue,
                  ec: &mut dyn ExecutionContext<Ty>| {
                // https://webidl.spec.whatwg.org/#create-an-interface-object
                // Step 1: Let steps be I's overridden constructor steps...
                // Step 1.1: If I was not declared with a constructor
                //   operation, then throw a TypeError.
                //   Note: handled by I::create_platform_object default impl.
                // Step 1.2: If NewTarget is undefined, then throw a TypeError.
                //   Note: Boa's [[Call]] passes `undefined` as `this` for
                //   constructable functions; [[Construct]] passes `new.target`.
                if Ty::value_is_undefined(&new_target_or_this) {
                    return Err(ec.new_type_error(&format!("{} is not a constructor", I::NAME)));
                }
                // Step 1.3: Let args be the passed arguments.
                // Step 1.4: Let n be the size of args.
                // Step 1.5-1.7: Overload resolution (not yet implemented).
                // Step 1.8: Let object be the result of internally creating
                //   a new object implementing I, with realm and NewTarget.
                let obj = I::create_platform_object(&new_target_or_this, args, ec)?;
                // Step 1.9: Perform the constructor steps of constructor
                //   with object as this and values as the argument values.
                //   (Handled inside create_platform_object.)
                // Step 1.10: Let O be object, converted to a JS value.
                //
                // Note: GC tracing for the stored platform data is handled
                // by wrapping in `TraceableBox` before type-erasing through
                // `create_object_with_any`.  The Boa backend detects the
                // `TraceableBox` wrapper and uses its trace/finalize function
                // pointers instead of no-op tracing.
                let traceable = js_engine::boa::TraceableBox::new(obj);
                let instance =
                    ec.create_object_with_any(instance_prototype.clone(), Box::new(traceable));
                // Step 1.11-1.13: Assert and return O.
                Ok(Ty::value_from_object(instance))
            },
        ),
        I::constructor_length() as u32,
        engine.property_key_from_str(I::NAME),
    );
    // Step 10: Let F be CreateBuiltinFunction(steps, length, id,
    //   « [[Unforgeables]] », realm, constructorProto).
    //   Note: create_constructor creates a constructable built-in function.
    let f_obj = Ty::object_from_function(constructor_fn);
    // Step 12: Perform ! DefinePropertyOrThrow(F, "prototype",
    //   PropertyDescriptor{[[Value]]: proto, [[Writable]]: false, ...}).
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
    // Set proto.constructor to F (spec implicit in OrdinaryCreateFromConstructor).
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
    // Step 13: Define the constants of interface I on F given realm.
    super::constant::define_constants::<Ty>(f_obj.clone(), engine, &def.constants)?;
    // Step 14-15: Define static attributes and static operations on F.
    super::attribute::define_static_attributes::<Ty, E>(engine, &f_val, &def.attributes)?;
    super::operation::define_static_operations::<Ty, E>(engine, &f_val, &def.operations)?;
    // Step 16: Return F.
    //   Note: store in registry so create_interface_instance can find F's prototype.
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

/// Non-Boa backend: store platform-object data directly (no GC tracing
/// concerns on JSC).
#[cfg(not(feature = "boa"))]
pub(crate) fn register_interface_spec<Ty, I, E>(engine: &mut E) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: WebIdlInterface<Ty> + 'static,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    // <https://webidl.spec.whatwg.org/#create-an-interface-object>
    // Step 2: Let constructorProto be realm.[[Intrinsics]].[[%Function.prototype%]].
    // Step 3: If I inherits ... (handled via constructorProto wiring).
    let realm = engine.current_realm();
    let intrinsics = engine.realm_intrinsics(&realm);
    // Step 11: Let proto be the result of creating an interface prototype
    //   object of interface I in realm.
    let proto = engine.create_object_with_any(intrinsics.object_prototype.clone(), Box::new(()));
    let mut def = InterfaceDefinition::<Ty>::new();
    I::define_members(&mut def);
    let proto_val = Ty::value_from_object(proto.clone());
    super::attribute::define_regular_attributes::<Ty, E>(engine, &proto_val, &def.attributes)?;
    super::operation::define_regular_operations::<Ty, E>(engine, &proto_val, &def.operations)?;
    // The constructor's instances should have the interface's prototype
    // object (proto) in their prototype chain, not Object.prototype.
    // Clone proto now before it's moved into the descriptor assignments.
    // <https://webidl.spec.whatwg.org/#ref-for-define-the-constants①>
    // Define the constants on the interface prototype object as well.
    super::constant::define_constants::<Ty>(proto.clone(), engine, &def.constants)?;
    let instance_prototype = proto.clone();
    let constructor_fn = engine.create_constructor(
        Box::new(
            move |args: &[Ty::JsValue],
                  new_target_or_this: Ty::JsValue,
                  ec: &mut dyn ExecutionContext<Ty>| {
                // https://webidl.spec.whatwg.org/#create-an-interface-object
                // Step 1: Let steps be I's overridden constructor steps...
                // Step 1.1: If I was not declared with a constructor
                //   operation, then throw a TypeError.
                //   Note: handled by I::create_platform_object default impl.
                // Step 1.2: If NewTarget is undefined, then throw a TypeError.
                //   Note: Boa's [[Call]] passes `undefined` as `this` for
                //   constructable functions; [[Construct]] passes `new.target`.
                if Ty::value_is_undefined(&new_target_or_this) {
                    return Err(ec.new_type_error(&format!("{} is not a constructor", I::NAME)));
                }
                // Step 1.3: Let args be the passed arguments.
                // Step 1.4: Let n be the size of args.
                // Step 1.5-1.7: Overload resolution (not yet implemented).
                // Step 1.8: Let object be the result of internally creating
                //   a new object implementing I, with realm and NewTarget.
                let obj = I::create_platform_object(&new_target_or_this, args, ec)?;
                // Step 1.9: Perform the constructor steps of constructor
                //   with object as this and values as the argument values.
                //   (Handled inside create_platform_object.)
                // Step 1.10: Let O be object, converted to a JS value.
                let instance = ec.create_object_with_any(instance_prototype.clone(), Box::new(obj));
                // Step 1.11-1.13: Assert and return O.
                Ok(Ty::value_from_object(instance))
            },
        ),
        I::constructor_length() as u32,
        engine.property_key_from_str(I::NAME),
    );
    // Step 10: Let F be CreateBuiltinFunction(steps, length, id,
    //   « [[Unforgeables]] », realm, constructorProto).
    //   Note: create_constructor creates a constructable built-in function.
    let f_obj = Ty::object_from_function(constructor_fn);
    // Step 12: Perform ! DefinePropertyOrThrow(F, "prototype",
    //   PropertyDescriptor{[[Value]]: proto, [[Writable]]: false, ...}).
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
    // Set proto.constructor to F (spec implicit in OrdinaryCreateFromConstructor).
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
    // Step 13: Define the constants of interface I on F given realm.
    super::constant::define_constants::<Ty>(f_obj.clone(), engine, &def.constants)?;
    // Step 14-15: Define static attributes and static operations on F.
    super::attribute::define_static_attributes::<Ty, E>(engine, &f_val, &def.attributes)?;
    super::operation::define_static_operations::<Ty, E>(engine, &f_val, &def.operations)?;
    // Step 16: Return F.
    //   Note: store in registry so create_interface_instance can find F's prototype.
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
