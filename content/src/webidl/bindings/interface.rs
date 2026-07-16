use js_engine::{
    Completion, EcmascriptHost, ExecutionContext, JsEngine, JsTypes, JsTypesWithRealm,
    PropertyDescriptor as JsPropertyDescriptor,
};

/// Trait for setting platform-object reflectors after creation.
/// Implemented in the content crate for ::js::Types.
pub(crate) trait PostCreateReflector<Ty: JsTypes> {
    fn set_reflector(obj: &Ty::JsObject, ec: &mut dyn ExecutionContext<Ty>);
}

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
/// <https://webidl.spec.whatwg.org/#js-interfaces>
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

    /// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
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

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
#[cfg(feature = "boa")]
pub(crate) fn create_interface_instance<Ty, T>(
    data: T,
    ec: &mut dyn ExecutionContext<Ty>,
) -> Completion<Ty::JsObject, Ty>
where
    Ty: JsTypes + JsTypesWithRealm + PostCreateReflector<Ty>,
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
    //   through create_object_with_any. The Boa backend recovers the
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

    // Set the EventTarget's reflector automatically through the
    // PostCreateReflector trait. The Web IDL layer handles this
    // transparently for all types containing EventTarget.
    <Ty as PostCreateReflector<Ty>>::set_reflector(&instance, ec);

    Ok(instance)
}

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
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
    let instance = ec.create_object_with_any(prototype, Box::new(data));

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

/// <https://webidl.spec.whatwg.org/#create-an-interface-object>
#[cfg(feature = "boa")]
pub(crate) fn register_interface_spec<Ty, I, E>(engine: &mut E) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm + PostCreateReflector<Ty>,
    I: WebIdlInterface<Ty> + boa_gc::Trace + boa_gc::Finalize + boa_engine::JsData + 'static,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    // <https://webidl.spec.whatwg.org/#create-an-interface-object>

    let realm = engine.current_realm();
    let intrinsics = engine.realm_intrinsics(&realm);

    // Step 2: "Let constructorProto be realm.[[Intrinsics]].[[%Function.prototype%]]."
    // Step 3: "If I inherits from some other interface P, then set constructorProto
    //   to the interface object of P in realm."
    //   Note: prototype chain wiring is done explicitly in host_hooks.rs via
    //   `wire_registry_prototype`. The constructorProto is not yet wired — the
    //   default %Function.prototype% is used, and subclass constructors inherit
    //   from their parent interface object via a separate call.

    // Step 11: "Let proto be the result of creating an interface prototype
    //   object of interface I in realm."
    let proto = engine.create_object_with_any(intrinsics.object_prototype.clone(), Box::new(()));

    let mut def = InterfaceDefinition::<Ty>::new();
    I::define_members(&mut def);
    let proto_val = Ty::value_from_object(proto.clone());

    super::attribute::define_regular_attributes::<Ty, E>(engine, &proto_val, &def.attributes)?;
    super::operation::define_regular_operations::<Ty, E>(engine, &proto_val, &def.operations)?;

    // <https://webidl.spec.whatwg.org/#ref-for-define-the-constants①>
    // "Define the constants on the interface prototype object."
    super::constant::define_constants::<Ty>(proto.clone(), engine, &def.constants)?;

    // Step 4: "Let unforgeables be OrdinaryObjectCreate(null)."
    // Step 5: "Define the unforgeable regular operations of I on unforgeables, given realm."
    // Step 6: "Define the unforgeable regular attributes of I on unforgeables, given realm."
    let unforgeables_obj = engine.create_plain_object(None);
    let unforgeables_val = Ty::value_from_object(unforgeables_obj.clone());
    super::operation::define_unforgeable_regular_operations::<Ty, E>(
        engine,
        &unforgeables_val,
        &def.operations,
    )?;
    super::attribute::define_unforgeable_regular_attributes::<Ty, E>(
        engine,
        &unforgeables_val,
        &def.attributes,
    )?;

    let instance_prototype = proto.clone();
    let unforgeables_for_closure = unforgeables_obj.clone();

    let constructor_fn = engine.create_builtin_function(
        Box::new({
            let instance_prototype_for_fn = instance_prototype.clone();
            let _unforgeables_ref = unforgeables_for_closure.clone();
            move |args: &[Ty::JsValue],
                  new_target_or_this: Ty::JsValue,
                  ec: &mut dyn ExecutionContext<Ty>| {
                // <https://webidl.spec.whatwg.org/#create-an-interface-object>
                //
                // Step 1: "Let steps be I's overridden constructor steps if they exist..."
                //
                // Step 1.1: "If I was not declared with a constructor operation,
                //   then throw a TypeError."
                //   Note: handled by I::create_platform_object default impl, which
                //   returns "Illegal constructor".
                //
                // Step 1.2: "If NewTarget is undefined, then throw a TypeError."
                //   Note: Boa's [[Call]] passes `undefined` as `this` for
                //   constructable functions; [[Construct]] passes `new.target`.
                if Ty::value_is_undefined(&new_target_or_this) {
                    return Err(ec.new_type_error(&format!("{} is not a constructor", I::NAME)));
                }

                // Step 1.3: "Let args be the passed arguments."
                // Step 1.4: "Let n be the size of args."
                // Step 1.5: "Let id be the identifier of interface I."
                // Steps 1.6-1.7: Overload resolution (not yet implemented).

                // Step 1.8: "Let object be the result of internally creating a new
                //   object implementing I, with realm and NewTarget."
                //
                // <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
                //
                // Step 4: "If newTarget is not undefined — already checked above.
                //   Step 4.2: "Let prototype be ? Get(newTarget, "prototype")."
                let new_target_obj = Ty::value_as_object(&new_target_or_this).ok_or_else(|| {
                    ec.new_type_error(&format!(
                        "{} constructor called without a valid new.target",
                        I::NAME
                    ))
                })?;
                let prototype_val = EcmascriptHost::get(ec, &new_target_obj, "prototype")?;

                // Step 4.3: "If prototype is not an Object, set prototype to the
                //   interface prototype object for interface in targetRealm."
                //   Note: cross-realm fallback not yet implemented; always falls
                //   back to the current realm's prototype object.
                let resolved_prototype = if Ty::value_as_object(&prototype_val).is_some() {
                    Ty::value_as_object(&prototype_val).ok_or_else(|| {
                        ec.new_type_error("TypeError: new.target.prototype is not an object")
                    })?
                } else {
                    instance_prototype_for_fn.clone()
                };

                // Step 1.8 (cont): call I::create_platform_object.
                let obj = I::create_platform_object(&new_target_or_this, args, ec)?;

                // Step 1.9: "Perform the constructor steps of constructor with
                //   object as this and values as the argument values."
                //   Note: handled inside create_platform_object.

                // Step 1.10: "Let O be object, converted to a JavaScript value."
                //
                // GC tracing for the stored platform data is handled by
                // wrapping in `TraceableBox` before type-erasing through
                // `create_object_with_any`. The Boa backend detects the
                // `TraceableBox` wrapper and uses its trace/finalize fn pointers.
                let traceable = js_engine::boa::TraceableBox::new(obj);
                let instance = ec.create_object_with_any(resolved_prototype, Box::new(traceable));

                // Set the EventTarget's reflector automatically for the newly
                // constructed platform object.
                <Ty as PostCreateReflector<Ty>>::set_reflector(&instance, ec);

                // Step 11: "For every interface ancestor interface in interfaces:"
                // Only copies own interface's [[Unforgeables]]; ancestor iteration
                // deferred until [[PrimaryInterface]] tracking is added.
                //   Step 11.1: "Let unforgeables be the value of the [[Unforgeables]] slot…"
                //   Step 11.2: "Let keys be ! unforgeables.[[OwnPropertyKeys]]()."
                //   Step 11.3: "For each element key of keys:"
                //   Step 11.3.1: "Let descriptor be ! unforgeables.[[GetOwnProperty]](key)."
                //   Step 11.3.2: "Perform ! DefinePropertyOrThrow(instance, key, descriptor)."
                if let Some(entry) =
                    super::registry::get_unforgeables_from_host_defined::<Ty, I>(ec)
                {
                    let own_keys = ec.own_property_keys(entry.clone())?;
                    for key in own_keys {
                        if let Some(d) = ec.get_own_property(entry.clone(), key.clone())? {
                            ec.define_property_or_throw(instance.clone(), key, d)?;
                        }
                    }
                }

                // Steps 1.11-1.13: Assert and return O.
                Ok(Ty::value_from_object(instance))
            }
        }),
        I::constructor_length() as u32,
        engine.property_key_from_str(I::NAME),
        true,
    );

    // Step 7: "Set F.[[Unforgeables]] to unforgeables."
    super::registry::set_unforgeables_for_interface::<Ty, I>(engine, unforgeables_obj.clone());

    // Step 10: "Let F be CreateBuiltinFunction(steps, length, id, « [[Unforgeables]] »,
    //   realm, constructorProto)."
    //   Note: is_constructor=true makes a constructable built-in function.
    let f_obj = Ty::object_from_function(constructor_fn);

    // Step 12: "Perform ! DefinePropertyOrThrow(F, "prototype",
    //   PropertyDescriptor{[[Value]]: proto, [[Writable]]: false,
    //   [[Enumerable]]: false, [[Configurable]]: false})."
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

    // Step 13: "Define the constants of interface I on F given realm."
    super::constant::define_constants::<Ty>(f_obj.clone(), engine, &def.constants)?;

    // Step 14: "Define the static attributes of interface I on F given realm."
    super::attribute::define_static_attributes::<Ty, E>(engine, &f_val, &def.attributes)?;

    // Step 15: "Define the static operations of interface I on F given realm."
    super::operation::define_static_operations::<Ty, E>(engine, &f_val, &def.operations)?;

    // Step 16: "Return F."
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

/// <https://webidl.spec.whatwg.org/#create-an-interface-object>
#[cfg(not(feature = "boa"))]
pub(crate) fn register_interface_spec<Ty, I, E>(engine: &mut E) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: WebIdlInterface<Ty> + 'static,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    // <https://webidl.spec.whatwg.org/#create-an-interface-object>

    let realm = engine.current_realm();
    let intrinsics = engine.realm_intrinsics(&realm);

    // Step 2: "Let constructorProto be realm.[[Intrinsics]].[[%Function.prototype%]]."
    // Step 3: "If I inherits from some other interface P, then set constructorProto
    //   to the interface object of P in realm."
    //   Note: prototype chain wiring is done explicitly in host_hooks.rs via
    //   `wire_registry_prototype`. The constructorProto is not yet wired — the
    //   default %Function.prototype% is used, and subclass constructors inherit
    //   from their parent interface object via a separate call.

    // Step 11: "Let proto be the result of creating an interface prototype
    //   object of interface I in realm."
    let proto = engine.create_object_with_any(intrinsics.object_prototype.clone(), Box::new(()));

    let mut def = InterfaceDefinition::<Ty>::new();
    I::define_members(&mut def);
    let proto_val = Ty::value_from_object(proto.clone());

    super::attribute::define_regular_attributes::<Ty, E>(engine, &proto_val, &def.attributes)?;
    super::operation::define_regular_operations::<Ty, E>(engine, &proto_val, &def.operations)?;

    // <https://webidl.spec.whatwg.org/#ref-for-define-the-constants①>
    // "Define the constants on the interface prototype object."
    super::constant::define_constants::<Ty>(proto.clone(), engine, &def.constants)?;

    // Step 4: "Let unforgeables be OrdinaryObjectCreate(null)."
    // Step 5: "Define the unforgeable regular operations of I on unforgeables, given realm."
    // Step 6: "Define the unforgeable regular attributes of I on unforgeables, given realm."
    let unforgeables_obj = engine.create_plain_object(None);
    let unforgeables_val = Ty::value_from_object(unforgeables_obj.clone());
    super::operation::define_unforgeable_regular_operations::<Ty, E>(
        engine,
        &unforgeables_val,
        &def.operations,
    )?;
    super::attribute::define_unforgeable_regular_attributes::<Ty, E>(
        engine,
        &unforgeables_val,
        &def.attributes,
    )?;

    let instance_prototype = proto.clone();
    let unforgeables_for_closure = unforgeables_obj.clone();

    let constructor_fn = engine.create_builtin_function(
        Box::new({
            let instance_prototype_for_fn = instance_prototype.clone();
            let _unforgeables_ref = unforgeables_for_closure.clone();
            move |args: &[Ty::JsValue],
                  new_target_or_this: Ty::JsValue,
                  ec: &mut dyn ExecutionContext<Ty>| {
                // <https://webidl.spec.whatwg.org/#create-an-interface-object>
                //
                // Step 1: "Let steps be I's overridden constructor steps if they exist..."
                //
                // Step 1.1: "If I was not declared with a constructor operation,
                //   then throw a TypeError."
                //   Note: handled by I::create_platform_object default impl, which
                //   returns "Illegal constructor".
                //
                // Step 1.2: "If NewTarget is undefined, then throw a TypeError."
                //   Note: Boa's [[Call]] passes `undefined` as `this` for
                //   constructable functions; [[Construct]] passes `new.target`.
                if Ty::value_is_undefined(&new_target_or_this) {
                    return Err(ec.new_type_error(&format!("{} is not a constructor", I::NAME)));
                }

                // Step 1.3: "Let args be the passed arguments."
                // Step 1.4: "Let n be the size of args."
                // Step 1.5: "Let id be the identifier of interface I."
                // Steps 1.6-1.7: Overload resolution (not yet implemented).

                // Step 1.8: "Let object be the result of internally creating a new
                //   object implementing I, with realm and NewTarget."
                //
                // <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
                //
                // Step 4: "If newTarget is not undefined — already checked above.
                //   Step 4.2: "Let prototype be ? Get(newTarget, "prototype")."
                let new_target_obj = Ty::value_as_object(&new_target_or_this).ok_or_else(|| {
                    ec.new_type_error(&format!(
                        "{} constructor called without a valid new.target",
                        I::NAME
                    ))
                })?;
                let prototype_val = EcmascriptHost::get(ec, &new_target_obj, "prototype")?;

                // Step 4.3: "If prototype is not an Object, set prototype to the
                //   interface prototype object for interface in targetRealm."
                //   Note: cross-realm fallback not yet implemented; always falls
                //   back to the current realm's prototype object.
                let resolved_prototype = if Ty::value_as_object(&prototype_val).is_some() {
                    Ty::value_as_object(&prototype_val).ok_or_else(|| {
                        ec.new_type_error("TypeError: new.target.prototype is not an object")
                    })?
                } else {
                    instance_prototype_for_fn.clone()
                };

                // Step 1.8 (cont): call I::create_platform_object.
                let obj = I::create_platform_object(&new_target_or_this, args, ec)?;

                // Step 1.9: "Perform the constructor steps of constructor with
                //   object as this and values as the argument values."
                //   Note: handled inside create_platform_object.

                // Step 1.10: "Let O be object, converted to a JavaScript value."
                let instance = ec.create_object_with_any(resolved_prototype, Box::new(obj));

                // Step 11: "For every interface ancestor interface in interfaces:"
                // Only copies own interface's [[Unforgeables]]; ancestor iteration
                // deferred until [[PrimaryInterface]] tracking is added.
                if let Some(entry) =
                    super::registry::get_unforgeables_from_host_defined::<Ty, I>(ec)
                {
                    if let Ok(own_keys) = ec.own_property_keys(entry.clone()) {
                        for key in own_keys {
                            if let Ok(Some(d)) = ec.get_own_property(entry.clone(), key.clone()) {
                                let _ = ec.define_property_or_throw(instance.clone(), key, d);
                            }
                        }
                    }
                }

                // Steps 1.11-1.13: Assert and return O.
                Ok(Ty::value_from_object(instance))
            }
        }),
        I::constructor_length() as u32,
        engine.property_key_from_str(I::NAME),
        true,
    );

    // Step 7: "Set F.[[Unforgeables]] to unforgeables."
    super::registry::set_unforgeables_for_interface::<Ty, I>(engine, unforgeables_obj.clone());

    // Step 10: "Let F be CreateBuiltinFunction(steps, length, id, « [[Unforgeables]] »,
    //   realm, constructorProto)."
    //   Note: is_constructor=true makes a constructable built-in function.
    let f_obj = Ty::object_from_function(constructor_fn);

    // Step 12: "Perform ! DefinePropertyOrThrow(F, "prototype",
    //   PropertyDescriptor{[[Value]]: proto, [[Writable]]: false,
    //   [[Enumerable]]: false, [[Configurable]]: false})."
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

    // Step 13: "Define the constants of interface I on F given realm."
    super::constant::define_constants::<Ty>(f_obj.clone(), engine, &def.constants)?;

    // Step 14: "Define the static attributes of interface I on F given realm."
    super::attribute::define_static_attributes::<Ty, E>(engine, &f_val, &def.attributes)?;

    // Step 15: "Define the static operations of interface I on F given realm."
    super::operation::define_static_operations::<Ty, E>(engine, &f_val, &def.operations)?;

    // Step 16: "Return F."
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

/// <https://webidl.spec.whatwg.org/#namespace-object>
pub(crate) trait WebIdlNamespace<T: JsTypes + JsTypesWithRealm>: 'static {
    const NAME: &'static str;
    fn define_members(def: &mut InterfaceDefinition<T>)
    where
        Self: Sized;
}

/// <https://webidl.spec.whatwg.org/#create-a-namespace-object>
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

// ── PostCreateReflector implementation for crate::js::Types ────────────

impl PostCreateReflector<crate::js::Types> for crate::js::Types {
    fn set_reflector(
        obj: &<crate::js::Types as JsTypes>::JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {
        let value = <crate::js::Types as JsTypes>::value_from_object(obj.clone());
        crate::js::try_set_event_target_reflector(&value, ec);
    }
}
