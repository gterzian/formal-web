use js_engine::gc::{Finalize, Trace};
use js_engine::records::PropertyDescriptor;
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;
use crate::js::create_builtin_fn_with_traced_captures;

use super::array_index::is_array_index_key;
use super::bindings::{
    PostCreateReflector, WebIdlInterface, create_interface_instance,
    get_legacy_platform_object_handler_from_host_defined,
    set_legacy_platform_object_handler_for_interface,
};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://webidl.spec.whatwg.org/#dfn-legacy-platform-object>
pub(crate) trait LegacyPlatformObject:
    WebIdlInterface<Types> + Clone + Trace + Finalize + 'static
{
    /// <https://webidl.spec.whatwg.org/#dfn-support-indexed-properties>
    const SUPPORTS_INDEXED_PROPERTIES: bool;

    /// <https://webidl.spec.whatwg.org/#dfn-support-named-properties>
    const SUPPORTS_NAMED_PROPERTIES: bool;

    /// <https://webidl.spec.whatwg.org/#LegacyOverrideBuiltIns>
    const LEGACY_OVERRIDE_BUILT_INS: bool = false;

    /// <https://webidl.spec.whatwg.org/#LegacyUnenumerableNamedProperties>
    const LEGACY_UNENUMERABLE_NAMED_PROPERTIES: bool = false;

    /// <https://webidl.spec.whatwg.org/#dfn-named-property-setter>
    const HAS_NAMED_PROPERTY_SETTER: bool = false;

    /// <https://webidl.spec.whatwg.org/#dfn-named-property-deleter>
    const HAS_NAMED_PROPERTY_DELETER: bool = false;

    /// <https://webidl.spec.whatwg.org/#invoke-a-named-property-setter>
    fn invoke_a_named_property_setter(
        &self,
        _name: &str,
        _value: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        Err(ec.new_type_error("the interface has no named property setter"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-named-property-deleter>
    // Note: the method steps of the deleter operation; the return value is
    // whether the deletion succeeded.
    fn invoke_a_named_property_deleter(
        &self,
        _name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<bool, Types> {
        Err(ec.new_type_error("the interface has no named property deleter"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-supported-property-indices>
    // Note: the supported property indices are always the range from zero up
    // to a count; the count is returned.
    fn supported_property_indices(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<u32, Types>;

    /// <https://webidl.spec.whatwg.org/#dfn-determine-the-value-of-an-indexed-property>
    fn determine_the_value_of_an_indexed_property(
        &self,
        index: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types>;

    /// <https://webidl.spec.whatwg.org/#dfn-supported-property-names>
    fn supported_property_names(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<String>, Types>;

    /// <https://webidl.spec.whatwg.org/#dfn-determine-the-value-of-a-named-property>
    fn determine_the_value_of_a_named_property(
        &self,
        name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types>;
}

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
// Note: step 13 only.  The platform object created by
// `create_interface_instance` becomes the target of a proxy whose traps are
// the legacy platform object internal methods, and the proxy is the object's
// reflector; the engine resolves the proxy to the target's platform data.
pub(crate) fn create_legacy_platform_object<T: LegacyPlatformObject>(
    data: T,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let target = create_interface_instance::<Types, T>(data, ec)?;
    // Step 13: "Otherwise, if interfaces contains an interface which supports indexed properties, named properties, or both:"
    // Step 13.1: "Set instance.[[GetOwnProperty]] as defined in § 3.9.1 [[GetOwnProperty]]."
    // Step 13.2: "Set instance.[[Set]] as defined in § 3.9.2 [[Set]]."
    // Step 13.3: "Set instance.[[DefineOwnProperty]] as defined in § 3.9.3 [[DefineOwnProperty]]."
    // Step 13.4: "Set instance.[[Delete]] as defined in § 3.9.4 [[Delete]]."
    // Step 13.5: "Set instance.[[PreventExtensions]] as defined in § 3.9.5 [[PreventExtensions]]."
    // Step 13.6: "Set instance.[[OwnPropertyKeys]] as defined in § 3.9.6 [[OwnPropertyKeys]]."
    let handler = legacy_platform_object_handler::<T>(ec)?;
    let proxy = ec.create_platform_object_proxy(target, handler)?;
    <Types as PostCreateReflector<Types>>::set_reflector(&proxy, ec);
    Ok(proxy)
}

type TrapFn = fn(&[JsValue], &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>;

#[gc_struct]
struct TrapCapture {
    #[ignore_trace]
    func: TrapFn,
}

fn trap_behaviour(
    args: &[JsValue],
    _this: JsValue,
    captures: &TrapCapture,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    (captures.func)(args, ec)
}

/// The proxy handler for an interface's legacy platform objects, one per
/// interface and realm.
fn legacy_platform_object_handler<T: LegacyPlatformObject>(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    if let Some(handler) = get_legacy_platform_object_handler_from_host_defined::<Types, T>(ec) {
        return Ok(handler);
    }
    let handler = ec.create_plain_object(None::<&JsObject>);
    let traps: [(TrapFn, u32, &str); 8] = [
        (
            trap_get_own_property_descriptor::<T>,
            2,
            "getOwnPropertyDescriptor",
        ),
        (trap_define_property::<T>, 3, "defineProperty"),
        (trap_delete_property::<T>, 2, "deleteProperty"),
        (trap_prevent_extensions::<T>, 1, "preventExtensions"),
        (trap_own_keys::<T>, 1, "ownKeys"),
        (trap_get::<T>, 3, "get"),
        (trap_set::<T>, 4, "set"),
        (trap_has::<T>, 2, "has"),
    ];
    for (trap, length, name) in traps {
        let name_key = ec.property_key_from_str(name);
        let function = create_builtin_fn_with_traced_captures(
            ec,
            TrapCapture { func: trap },
            trap_behaviour,
            length,
            name_key,
            false,
        );
        let function_object = <Types as JsTypes>::object_from_function(function);
        let key = ec.property_key_from_str(name);
        ec.set(
            handler.clone(),
            key,
            <Types as JsTypes>::value_from_object(function_object),
            false,
        )?;
    }
    set_legacy_platform_object_handler_for_interface::<Types, T>(ec, handler.clone());
    Ok(handler)
}

fn trap_target<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(JsObject, T), Types> {
    let target = args
        .first()
        .and_then(<Types as JsTypes>::value_as_object)
        .ok_or_else(|| ec.new_type_error("legacy platform object trap without its target"))?;
    let object = ec
        .with_object_any(&target)
        .and_then(|data| data.downcast_ref::<T>().cloned())
        .ok_or_else(|| {
            ec.new_type_error(&format!(
                "legacy platform object target is not a {}",
                T::NAME
            ))
        })?;
    Ok((target, object))
}

fn trap_key(args: &[JsValue], ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    args.get(1).cloned().unwrap_or_else(|| ec.value_undefined())
}

fn string_key(key: &JsValue, ec: &dyn ExecutionContext<Types>) -> Option<String> {
    <Types as JsTypes>::value_as_string(key).map(|string| ec.js_string_to_rust_string(&string))
}

fn is_data_descriptor(descriptor: &PropertyDescriptor<Types>) -> bool {
    descriptor.value.is_some() || descriptor.writable.is_some()
}

fn is_accessor_descriptor(descriptor: &PropertyDescriptor<Types>) -> bool {
    descriptor.get.is_some() || descriptor.set.is_some()
}

/// <https://tc39.es/ecma262/#sec-frompropertydescriptor>
fn from_property_descriptor(
    descriptor: Option<PropertyDescriptor<Types>>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: "If propertyDesc is undefined, return undefined."
    let Some(descriptor) = descriptor else {
        return Ok(ec.value_undefined());
    };

    // Step 2: "Let obj be OrdinaryObjectCreate(%Object.prototype%)."
    let object = ec.create_plain_object(None::<&JsObject>);

    // Step 3: "Assert: obj is an extensible ordinary object with no own properties."
    // Step 4: "If propertyDesc has a [[Value]] field, then Perform ! CreateDataPropertyOrThrow(obj, "value", propertyDesc.[[Value]])."
    if let Some(value) = descriptor.value {
        let key = ec.property_key_from_str("value");
        ec.create_data_property(object.clone(), key, value)?;
    }

    // Step 5: "If propertyDesc has a [[Writable]] field, then Perform ! CreateDataPropertyOrThrow(obj, "writable", propertyDesc.[[Writable]])."
    if let Some(writable) = descriptor.writable {
        let key = ec.property_key_from_str("writable");
        let value = ec.value_from_bool(writable);
        ec.create_data_property(object.clone(), key, value)?;
    }

    // Step 6: "If propertyDesc has a [[Getter]] field, then Perform ! CreateDataPropertyOrThrow(obj, "get", propertyDesc.[[Getter]])."
    if let Some(getter) = descriptor.get {
        let key = ec.property_key_from_str("get");
        let value =
            <Types as JsTypes>::value_from_object(<Types as JsTypes>::object_from_function(getter));
        ec.create_data_property(object.clone(), key, value)?;
    }

    // Step 7: "If propertyDesc has a [[Setter]] field, then Perform ! CreateDataPropertyOrThrow(obj, "set", propertyDesc.[[Setter]])."
    if let Some(setter) = descriptor.set {
        let key = ec.property_key_from_str("set");
        let value =
            <Types as JsTypes>::value_from_object(<Types as JsTypes>::object_from_function(setter));
        ec.create_data_property(object.clone(), key, value)?;
    }

    // Step 8: "If propertyDesc has an [[Enumerable]] field, then Perform ! CreateDataPropertyOrThrow(obj, "enumerable", propertyDesc.[[Enumerable]])."
    if let Some(enumerable) = descriptor.enumerable {
        let key = ec.property_key_from_str("enumerable");
        let value = ec.value_from_bool(enumerable);
        ec.create_data_property(object.clone(), key, value)?;
    }

    // Step 9: "If propertyDesc has a [[Configurable]] field, then Perform ! CreateDataPropertyOrThrow(obj, "configurable", propertyDesc.[[Configurable]])."
    if let Some(configurable) = descriptor.configurable {
        let key = ec.property_key_from_str("configurable");
        let value = ec.value_from_bool(configurable);
        ec.create_data_property(object.clone(), key, value)?;
    }

    // Step 10: "Return obj."
    Ok(<Types as JsTypes>::value_from_object(object))
}

/// <https://webidl.spec.whatwg.org/#dfn-named-property-visibility>
fn named_property_visibility<T: LegacyPlatformObject>(
    object: &T,
    target: &JsObject,
    name: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types> {
    // Step 1: "If P is not a supported property name of O, then return false."
    if !object
        .supported_property_names(ec)?
        .iter()
        .any(|supported| supported == name)
    {
        return Ok(false);
    }

    // Step 2: "If O has an own property named P, then return false."
    let key = ec.property_key_from_str(name);
    if ec.has_own_property(target.clone(), key.clone())? {
        return Ok(false);
    }

    // Step 3: "If O implements an interface that has the [LegacyOverrideBuiltIns] extended attribute, then return true."
    if T::LEGACY_OVERRIDE_BUILT_INS {
        return Ok(true);
    }

    // Step 4: "Let prototype be O.[[GetPrototypeOf]]()."
    let mut prototype = ec.get_prototype_of(target.clone())?;

    // Step 5: "While prototype is not null:"
    while let Some(current) = prototype {
        // Step 5.1: "If prototype is not a named properties object, and prototype has an own property named P, then return false."
        // Note: no named properties object exists in this implementation.
        if ec.has_own_property(current.clone(), key.clone())? {
            return Ok(false);
        }

        // Step 5.2: "Set prototype to prototype.[[GetPrototypeOf]]()."
        prototype = ec.get_prototype_of(current)?;
    }

    // Step 6: "Return true."
    Ok(true)
}

/// <https://webidl.spec.whatwg.org/#LegacyPlatformObjectGetOwnProperty>
fn legacy_platform_object_get_own_property<T: LegacyPlatformObject>(
    object: &T,
    target: &JsObject,
    key: &JsValue,
    ignore_named_props: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<PropertyDescriptor<Types>>, Types> {
    let mut ignore_named_props = ignore_named_props;

    // Step 1: "If O supports indexed properties and P is an array index, then:"
    if T::SUPPORTS_INDEXED_PROPERTIES && is_array_index_key(key, ec) {
        // Step 1.1: "Let index be the result of calling ToUint32(P)."
        let index = ec.to_uint32(key.clone())?;

        // Step 1.2: "If index is a supported property index, then:"
        if index < object.supported_property_indices(ec)? {
            // Step 1.2.1: "Let operation be the operation used to declare the indexed property getter."
            // Step 1.2.2: "Let value be an uninitialized variable."
            // Step 1.2.3: "If operation was defined without an identifier, then set value to the result of performing the steps listed in the interface description to determine the value of an indexed property with index as the index."
            // Step 1.2.4: "Otherwise, operation was defined with an identifier. Set value to the result of performing the method steps of operation with O as this and « index » as the argument values."
            let value = object.determine_the_value_of_an_indexed_property(index, ec)?;

            // Step 1.2.5: "Let desc be a newly created Property Descriptor with no fields."
            // Step 1.2.6: "Set desc.[[Value]] to the result of converting value to a JavaScript value."
            // Step 1.2.7: "If O implements an interface with an indexed property setter, then set desc.[[Writable]] to true, otherwise false."
            // Step 1.2.8: "Set desc.[[Enumerable]] and desc.[[Configurable]] to true."
            // Step 1.2.9: "Return desc."
            return Ok(Some(PropertyDescriptor {
                value: Some(value),
                writable: Some(false),
                get: None,
                set: None,
                enumerable: Some(true),
                configurable: Some(true),
            }));
        }

        // Step 1.3: "Set ignoreNamedProps to true."
        ignore_named_props = true;
    }

    // Step 2: "If O supports named properties and ignoreNamedProps is false, then:"
    if T::SUPPORTS_NAMED_PROPERTIES && !ignore_named_props {
        // Step 2.1: "If the result of running the named property visibility algorithm with property name P and object O is true, then:"
        if let Some(name) = string_key(key, ec)
            && named_property_visibility(object, target, &name, ec)?
        {
            // Step 2.1.1: "Let operation be the operation used to declare the named property getter."
            // Step 2.1.2: "Let value be an uninitialized variable."
            // Step 2.1.3: "If operation was defined without an identifier, then set value to the result of performing the steps listed in the interface description to determine the value of a named property with P as the name."
            // Step 2.1.4: "Otherwise, operation was defined with an identifier. Set value to the result of performing the method steps of operation with O as this and « P » as the argument values."
            let value = object.determine_the_value_of_a_named_property(&name, ec)?;

            // Step 2.1.5: "Let desc be a newly created Property Descriptor with no fields."
            // Step 2.1.6: "Set desc.[[Value]] to the result of converting value to a JavaScript value."
            // Step 2.1.7: "If O implements an interface with a named property setter, then set desc.[[Writable]] to true, otherwise false."
            // Step 2.1.8: "If O implements an interface with the [LegacyUnenumerableNamedProperties] extended attribute, then set desc.[[Enumerable]] to false, otherwise true."
            // Step 2.1.9: "Set desc.[[Configurable]] to true."
            // Step 2.1.10: "Return desc."
            return Ok(Some(PropertyDescriptor {
                value: Some(value),
                writable: Some(T::HAS_NAMED_PROPERTY_SETTER),
                get: None,
                set: None,
                enumerable: Some(!T::LEGACY_UNENUMERABLE_NAMED_PROPERTIES),
                configurable: Some(true),
            }));
        }
    }

    // Step 3: "Return OrdinaryGetOwnProperty(O, P)."
    let property_key = ec.to_property_key(key.clone())?;
    ec.get_own_property(target.clone(), property_key)
}

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-getownproperty>
fn trap_get_own_property_descriptor<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;
    let key = trap_key(args, ec);
    // Step 1: "Return ? LegacyPlatformObjectGetOwnProperty(O, P, false)."
    let descriptor = legacy_platform_object_get_own_property(&object, &target, &key, false, ec)?;
    from_property_descriptor(descriptor, ec)
}

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-defineownproperty>
fn trap_define_property<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;
    let key = trap_key(args, ec);
    let descriptor_object = args
        .get(2)
        .and_then(<Types as JsTypes>::value_as_object)
        .ok_or_else(|| ec.new_type_error("property descriptor is not an object"))?;
    let descriptor = ec.to_property_descriptor(descriptor_object)?;

    // Step 1: "If O supports indexed properties and P is an array index, then:"
    if T::SUPPORTS_INDEXED_PROPERTIES && is_array_index_key(&key, ec) {
        // Step 1.1: "If the result of calling IsDataDescriptor(Desc) is false, then return false."
        // Step 1.2: "If O does not implement an interface with an indexed property setter, then return false."
        // Step 1.3: "Invoke the indexed property setter on O with P and Desc.[[Value]]."
        // Step 1.4: "Return true."
        // Note: no interface here declares an indexed property setter.
        return Ok(ec.value_from_bool(false));
    }

    // Step 2: "If O supports named properties, O does not implement an interface with the [Global] extended attribute, P is a String, and P is not an unforgeable property name of O, then:"
    if T::SUPPORTS_NAMED_PROPERTIES
        && let Some(name) = string_key(&key, ec)
    {
        // Step 2.1: "Let creating be true if P is not a supported property name, and false otherwise."
        let creating = !object.supported_property_names(ec)?.contains(&name);

        // Step 2.2: "If O implements an interface with the [LegacyOverrideBuiltIns] extended attribute or O does not have an own property named P, then:"
        let own_key = ec.property_key_from_str(&name);
        if T::LEGACY_OVERRIDE_BUILT_INS || !ec.has_own_property(target.clone(), own_key)? {
            // Step 2.2.1: "If creating is false and O does not implement an interface with a named property setter, then return false."
            if !creating && !T::HAS_NAMED_PROPERTY_SETTER {
                return Ok(ec.value_from_bool(false));
            }

            // Step 2.2.2: "If O implements an interface with a named property setter, then:"
            if T::HAS_NAMED_PROPERTY_SETTER {
                // Step 2.2.2.1: "If the result of calling IsDataDescriptor(Desc) is false, then return false."
                if !is_data_descriptor(&descriptor) {
                    return Ok(ec.value_from_bool(false));
                }

                // Step 2.2.2.2: "Invoke the named property setter on O with P and Desc.[[Value]]."
                let value = descriptor.value.unwrap_or_else(|| ec.value_undefined());
                object.invoke_a_named_property_setter(&name, value, ec)?;

                // Step 2.2.2.3: "Return true."
                return Ok(ec.value_from_bool(true));
            }
        }
    }

    // Step 3: "Return ! OrdinaryDefineOwnProperty(O, P, Desc)."
    let property_key = ec.to_property_key(key)?;
    let defined = ec
        .define_property_or_throw(target, property_key, descriptor)
        .is_ok();
    Ok(ec.value_from_bool(defined))
}

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-delete>
fn trap_delete_property<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;
    let key = trap_key(args, ec);

    // Step 1: "If O supports indexed properties and P is an array index, then:"
    if T::SUPPORTS_INDEXED_PROPERTIES && is_array_index_key(&key, ec) {
        // Step 1.1: "Let index be the result of calling ToUint32(P)."
        let index = ec.to_uint32(key.clone())?;

        // Step 1.2: "If index is not a supported property index, then return true."
        if index >= object.supported_property_indices(ec)? {
            return Ok(ec.value_from_bool(true));
        }

        // Step 1.3: "Return false."
        return Ok(ec.value_from_bool(false));
    }

    // Step 2: "If O supports named properties, O does not implement an interface with the [Global] extended attribute and the result of calling the named property visibility algorithm with property name P and object O is true, then:"
    if T::SUPPORTS_NAMED_PROPERTIES
        && let Some(name) = string_key(&key, ec)
        && named_property_visibility(&object, &target, &name, ec)?
    {
        // Step 2.1: "If O does not implement an interface with a named property deleter, then return false."
        if !T::HAS_NAMED_PROPERTY_DELETER {
            return Ok(ec.value_from_bool(false));
        }

        // Step 2.2: "Let operation be the operation used to declare the named property deleter."
        // Step 2.3: "If operation was defined without an identifier, then:"
        // Step 2.4: "Otherwise, operation was defined with an identifier:"
        // Step 2.4.1: "Perform the method steps of operation with O as this and « P » as the argument values."
        // Step 2.4.2: "If operation was declared with a return type of boolean and the steps returned false, then return false."
        if !object.invoke_a_named_property_deleter(&name, ec)? {
            return Ok(ec.value_from_bool(false));
        }

        // Step 2.5: "Return true."
        return Ok(ec.value_from_bool(true));
    }

    // Step 3: "If O has an own property with name P, then:"
    let property_key = ec.to_property_key(key)?;
    if let Some(descriptor) = ec.get_own_property(target.clone(), property_key.clone())? {
        // Step 3.1: "If the property is not configurable, then return false."
        if descriptor.configurable != Some(true) {
            return Ok(ec.value_from_bool(false));
        }

        // Step 3.2: "Otherwise, remove the property from O."
        ec.delete_property_or_throw(target, property_key)?;
    }

    // Step 4: "Return true."
    Ok(ec.value_from_bool(true))
}

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-preventextensions>
fn trap_prevent_extensions<T: LegacyPlatformObject>(
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: "Return false."
    Ok(ec.value_from_bool(false))
}

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-ownpropertykeys>
fn trap_own_keys<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;

    // Step 1: "Let keys be a new empty list of JavaScript String and Symbol values."
    let keys = ec.create_empty_array();

    // Step 2: "If O supports indexed properties, then for each index of O’s supported property indices, in ascending numerical order, append ! ToString(index) to keys."
    if T::SUPPORTS_INDEXED_PROPERTIES {
        for index in 0..object.supported_property_indices(ec)? {
            let key = ec.js_string_from_str(&index.to_string());
            let key = ec.value_from_string(key);
            ec.array_push(&keys, key)?;
        }
    }

    // Step 3: "If O supports named properties, then for each P of O’s supported property names that is visible according to the named property visibility algorithm, append P to keys."
    if T::SUPPORTS_NAMED_PROPERTIES {
        for name in object.supported_property_names(ec)? {
            if named_property_visibility(&object, &target, &name, ec)? {
                let key = ec.js_string_from_str(&name);
                let key = ec.value_from_string(key);
                ec.array_push(&keys, key)?;
            }
        }
    }

    // Step 4: "For each P of O’s own property keys that is a String, in ascending chronological order of property creation, append P to keys."
    // Step 5: "For each P of O’s own property keys that is a Symbol, in ascending chronological order of property creation, append P to keys."
    for key in ec.own_property_keys(target)? {
        let key = ec.value_from_property_key(key);
        ec.array_push(&keys, key)?;
    }

    // Step 6: "Assert: keys has no duplicate items."
    // Step 7: "Return keys."
    Ok(<Types as JsTypes>::value_from_object(keys))
}

/// <https://tc39.es/ecma262/#sec-ordinaryget>
// Note: the [[GetOwnProperty]] of step 1 is the legacy platform object's;
// the prototype chain is walked here because a parent's [[Get]] with this
// receiver is not an engine operation.
fn trap_get<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;
    let key = trap_key(args, ec);
    let receiver = args.get(2).cloned().unwrap_or_else(|| ec.value_undefined());

    // Step 1: "Let propertyDesc be ? obj.[[GetOwnProperty]](propertyKey)."
    let mut descriptor =
        legacy_platform_object_get_own_property(&object, &target, &key, false, ec)?;
    let property_key = ec.to_property_key(key)?;
    let mut holder = target;
    loop {
        // Step 2: "If propertyDesc is undefined, then"
        let Some(found) = descriptor else {
            // Step 2.a: "Let parent be ? obj.[[GetPrototypeOf]]()."
            // Step 2.b: "If parent is null, return undefined."
            let Some(parent) = ec.get_prototype_of(holder)? else {
                return Ok(ec.value_undefined());
            };

            // Step 2.c: "Return ? parent.[[Get]](propertyKey, receiver)."
            descriptor = ec.get_own_property(parent.clone(), property_key.clone())?;
            holder = parent;
            continue;
        };

        // Step 3: "If IsDataDescriptor(propertyDesc) is true, return propertyDesc.[[Value]]."
        if is_data_descriptor(&found) {
            return Ok(found.value.unwrap_or_else(|| ec.value_undefined()));
        }

        // Step 4: "Assert: IsAccessorDescriptor(propertyDesc) is true."
        // Step 5: "Let getter be propertyDesc.[[Getter]]."
        // Step 6: "If getter is undefined, return undefined."
        let Some(getter) = found.get else {
            return Ok(ec.value_undefined());
        };

        // Step 7: "Return ? Call(getter, receiver)."
        let getter = <Types as JsTypes>::object_from_function(getter);
        return ec.call(&getter, &receiver, &[]);
    }
}

/// <https://tc39.es/ecma262/#sec-ordinarysetwithowndescriptor>
fn ordinary_set_with_own_descriptor(
    holder: JsObject,
    property_key: <Types as JsTypes>::PropertyKey,
    value: JsValue,
    receiver: &JsValue,
    own_descriptor: Option<PropertyDescriptor<Types>>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types> {
    // Step 1: "If ownDesc is undefined, then"
    let own_descriptor = match own_descriptor {
        Some(own_descriptor) => own_descriptor,
        None => {
            // Step 1.a: "Let parent be ? obj.[[GetPrototypeOf]]()."
            // Step 1.b: "If parent is not null, return ? parent.[[Set]](propertyKey, value, receiver)."
            if let Some(parent) = ec.get_prototype_of(holder)? {
                let parent_descriptor =
                    ec.get_own_property(parent.clone(), property_key.clone())?;
                return ordinary_set_with_own_descriptor(
                    parent,
                    property_key,
                    value,
                    receiver,
                    parent_descriptor,
                    ec,
                );
            }

            // Step 1.c: "Set ownDesc to the PropertyDescriptor { [[Value]]: undefined, [[Writable]]: true, [[Enumerable]]: true, [[Configurable]]: true }."
            PropertyDescriptor {
                value: Some(ec.value_undefined()),
                writable: Some(true),
                get: None,
                set: None,
                enumerable: Some(true),
                configurable: Some(true),
            }
        }
    };

    // Step 2: "If IsDataDescriptor(ownDesc) is true, then"
    if is_data_descriptor(&own_descriptor) {
        // Step 2.a: "If ownDesc.[[Writable]] is false, return false."
        if own_descriptor.writable != Some(true) {
            return Ok(false);
        }

        // Step 2.b: "If receiver is not an Object, return false."
        let Some(receiver_object) = <Types as JsTypes>::value_as_object(receiver) else {
            return Ok(false);
        };

        // Step 2.c: "Let existingDesc be ? receiver.[[GetOwnProperty]](propertyKey)."
        let existing = ec.get_own_property(receiver_object.clone(), property_key.clone())?;

        // Step 2.d: "If existingDesc is undefined, then"
        let Some(existing) = existing else {
            // Step 2.d.i: "Assert: receiver does not currently have a property propertyKey."
            // Step 2.d.ii: "Return ? CreateDataProperty(receiver, propertyKey, value)."
            return ec.create_data_property(receiver_object, property_key, value);
        };

        // Step 2.e: "If IsAccessorDescriptor(existingDesc) is true, return false."
        if is_accessor_descriptor(&existing) {
            return Ok(false);
        }

        // Step 2.f: "If existingDesc.[[Writable]] is false, return false."
        if existing.writable != Some(true) {
            return Ok(false);
        }

        // Step 2.g: "Let valueDesc be the PropertyDescriptor { [[Value]]: value }."
        let value_descriptor = PropertyDescriptor {
            value: Some(value),
            writable: None,
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        };

        // Step 2.h: "Return ? receiver.[[DefineOwnProperty]](propertyKey, valueDesc)."
        return Ok(ec
            .define_property_or_throw(receiver_object, property_key, value_descriptor)
            .is_ok());
    }

    // Step 3: "Assert: IsAccessorDescriptor(ownDesc) is true."
    // Step 4: "Let setter be ownDesc.[[Setter]]."
    // Step 5: "If setter is undefined, return false."
    let Some(setter) = own_descriptor.set else {
        return Ok(false);
    };

    // Step 6: "Perform ? Call(setter, receiver, « value »)."
    let setter = <Types as JsTypes>::object_from_function(setter);
    ec.call(&setter, receiver, &[value])?;

    // Step 7: "Return true."
    Ok(true)
}

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-set>
fn trap_set<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;
    let key = trap_key(args, ec);
    let value = args.get(2).cloned().unwrap_or_else(|| ec.value_undefined());
    let receiver = args.get(3).cloned().unwrap_or_else(|| ec.value_undefined());

    // Step 1: "If O and Receiver are the same object, then:"
    // Note: the trap's O is the proxy target and the receiver is the proxy
    // itself; they are the same platform object when the receiver's platform
    // data is this target's interface.
    let receiver_is_this_object = <Types as JsTypes>::value_as_object(&receiver)
        .and_then(|receiver_object| {
            ec.with_object_any(&receiver_object)
                .map(|data| data.downcast_ref::<T>().is_some())
        })
        .unwrap_or(false);
    if receiver_is_this_object {
        // Step 1.1: "If O implements an interface with an indexed property setter and P is an array index, then:"
        // Note: no interface here declares an indexed property setter.
        // Step 1.2: "If O implements an interface with a named property setter and P is a String, then:"
        if T::HAS_NAMED_PROPERTY_SETTER
            && let Some(name) = string_key(&key, ec)
        {
            // Step 1.2.1: "Invoke the named property setter on O with P and V."
            object.invoke_a_named_property_setter(&name, value, ec)?;

            // Step 1.2.2: "Return true."
            return Ok(ec.value_from_bool(true));
        }
    }

    // Step 2: "Let ownDesc be ? LegacyPlatformObjectGetOwnProperty(O, P, true)."
    let own_descriptor = legacy_platform_object_get_own_property(&object, &target, &key, true, ec)?;

    // Step 3: "Perform ? OrdinarySetWithOwnDescriptor(O, P, V, Receiver, ownDesc)."
    let property_key = ec.to_property_key(key)?;
    let set = ordinary_set_with_own_descriptor(
        target,
        property_key,
        value,
        &receiver,
        own_descriptor,
        ec,
    )?;
    Ok(ec.value_from_bool(set))
}

/// <https://tc39.es/ecma262/#sec-ordinaryhasproperty>
fn trap_has<T: LegacyPlatformObject>(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (target, object) = trap_target::<T>(args, ec)?;
    let key = trap_key(args, ec);

    // Step 1: "Let hasOwn be ? obj.[[GetOwnProperty]](propertyKey)."
    let has_own = legacy_platform_object_get_own_property(&object, &target, &key, false, ec)?;

    // Step 2: "If hasOwn is not undefined, return true."
    if has_own.is_some() {
        return Ok(ec.value_from_bool(true));
    }

    // Step 3: "Let parent be ? obj.[[GetPrototypeOf]]()."
    // Step 4: "If parent is not null, then"
    if let Some(parent) = ec.get_prototype_of(target)? {
        // Step 4.a: "Return ? parent.[[HasProperty]](propertyKey)."
        let property_key = ec.to_property_key(key)?;
        let has = ec.has_property(parent, property_key)?;
        return Ok(ec.value_from_bool(has));
    }

    // Step 5: "Return false."
    Ok(ec.value_from_bool(false))
}
