use js_engine::gc_struct;
use js_engine::{
    Completion, ExecutionContext, JsEngine, JsTypes, JsTypesWithRealm, PropertyDescriptor,
};

/// Describes a single attribute on an interface.
/// https://webidl.spec.whatwg.org/#dfn-attribute
pub(crate) struct AttributeDef<T: JsTypes> {
    pub id: &'static str,
    pub getter:
        fn(&T::JsValue, &[T::JsValue], &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>,
    pub setter: Option<
        fn(&T::JsValue, &[T::JsValue], &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>,
    >,
    pub static_: bool,
    pub unforgeable: bool,
    pub promise_type: bool,
    pub legacy_lenient_this: bool,
    pub replaceable: bool,
    pub put_forwards: Option<&'static str>,
    pub legacy_lenient_setter: bool,
    /// Optional exposure restriction, e.g. "Window" or "Window,Worker".
    /// `None` means exposed in all realms (the common case).
    /// Implements Step 1.1 of <https://webidl.spec.whatwg.org/#define-the-attributes>.
    pub exposed: Option<&'static str>,
}

/// <https://webidl.spec.whatwg.org/#define-the-regular-attributes>
pub(crate) fn define_regular_attributes<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    attributes: &[AttributeDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let regular: Vec<&AttributeDef<Ty>> = attributes
        .iter()
        .filter(|a| !a.static_ && !a.unforgeable)
        .collect();
    define_attributes_on_target(engine, target, &regular)
}

/// <https://webidl.spec.whatwg.org/#define-the-unforgeable-regular-attributes>
pub(crate) fn define_unforgeable_regular_attributes<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    attributes: &[AttributeDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let unforgeable: Vec<&AttributeDef<Ty>> = attributes
        .iter()
        .filter(|a| !a.static_ && a.unforgeable)
        .collect();
    define_attributes_on_target(engine, target, &unforgeable)
}

/// <https://webidl.spec.whatwg.org/#define-the-static-attributes>
pub(crate) fn define_static_attributes<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    attributes: &[AttributeDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let static_attrs: Vec<&AttributeDef<Ty>> = attributes.iter().filter(|a| a.static_).collect();
    define_attributes_on_target(engine, target, &static_attrs)
}

fn define_attributes_on_target<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    attributes: &[&AttributeDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let target_obj = Ty::value_as_object(target)
        .ok_or_else(|| engine.new_type_error("target is not an object in attribute definition"))?;
    for attr in attributes {
        // Step 1.1: "If attr is not exposed in realm, then continue."
        if let Some(exposed_globals) = attr.exposed {
            // Note: For now, "Window" is the only supported global type.
            // When Worker/other globals are supported, this will check
            // the current realm's global type against the list.
            if exposed_globals != "Window" {
                continue;
            }
        }
        #[gc_struct]
        struct AttrCapture<T: JsTypes> {
            #[ignore_trace]
            func: fn(
                &T::JsValue,
                &[T::JsValue],
                &mut dyn ExecutionContext<T>,
            ) -> Completion<T::JsValue, T>,
        }

        fn attr_fn<T: JsTypes>(
            args: &[T::JsValue],
            this: T::JsValue,
            captures: &AttrCapture<T>,
            ec: &mut dyn ExecutionContext<T>,
        ) -> Completion<T::JsValue, T> {
            (captures.func)(&this, args, ec)
        }

        let name_key = engine.property_key_from_str(attr.id);
        let getter_fn = crate::js::create_builtin_fn_with_traced_captures(
            engine,
            AttrCapture { func: attr.getter },
            attr_fn::<Ty>,
            0,
            name_key.clone(),
            false,
        );
        let mut desc = PropertyDescriptor {
            value: None,
            get: Some(getter_fn),
            set: None,
            writable: None,
            enumerable: Some(true),
            configurable: Some(!attr.unforgeable),
        };
        if let Some(setter) = attr.setter {
            let setter_fn = crate::js::create_builtin_fn_with_traced_captures(
                engine,
                AttrCapture { func: setter },
                attr_fn::<Ty>,
                1,
                name_key,
                false,
            );
            desc.set = Some(setter_fn);
        } else if attr.replaceable {
            // <https://webidl.spec.whatwg.org/#dfn-attribute-setter>
            // The [Replaceable] extended attribute turns the assignment into
            // an own data property that shadows the accessor (step 4.5.7).
            #[gc_struct]
            struct ReplaceableCapture<T: JsTypes> {
                #[ignore_trace]
                attr_id: &'static str,
                #[ignore_trace]
                marker: std::marker::PhantomData<T>,
            }

            fn replaceable_behaviour<T: JsTypes + JsTypesWithRealm>(
                args: &[T::JsValue],
                this: T::JsValue,
                captures: &ReplaceableCapture<T>,
                ec: &mut dyn ExecutionContext<T>,
            ) -> Completion<T::JsValue, T> {
                let undefined = ec.value_undefined();
                let value = args.first().cloned().unwrap_or_else(|| undefined.clone());

                // Step 4.5.7.1: "Perform ? CreateDataProperty(esValue, id, V)."
                let Some(this_object) = T::value_as_object(&this) else {
                    return Err(ec.new_type_error(&format!(
                        "cannot replace attribute '{}' on a non-object",
                        captures.attr_id
                    )));
                };
                let attr_key = ec.property_key_from_str(captures.attr_id);
                ec.create_data_property(this_object, attr_key, value)?;

                // Step 4.5.7.2: "Return undefined."
                Ok(ec.value_undefined())
            }

            let setter_fn = crate::js::create_builtin_fn_with_traced_captures(
                engine,
                ReplaceableCapture {
                    attr_id: attr.id,
                    marker: std::marker::PhantomData,
                },
                replaceable_behaviour::<Ty>,
                1,
                name_key,
                false,
            );
            desc.set = Some(setter_fn);
        } else if let Some(forward_id) = attr.put_forwards {
            // <https://webidl.spec.whatwg.org/#dfn-attribute-setter>
            // The [PutForwards] extended attribute turns the assignment into
            // an assignment of the forwarded attribute on the object the
            // attribute currently references (step 4.5.8).
            #[gc_struct]
            struct PutForwardsCapture<T: JsTypes> {
                #[ignore_trace]
                attr_id: &'static str,
                #[ignore_trace]
                forward_id: &'static str,
                #[ignore_trace]
                marker: std::marker::PhantomData<T>,
            }

            fn put_forwards_behaviour<T: JsTypes + JsTypesWithRealm>(
                args: &[T::JsValue],
                this: T::JsValue,
                captures: &PutForwardsCapture<T>,
                ec: &mut dyn ExecutionContext<T>,
            ) -> Completion<T::JsValue, T> {
                let undefined = ec.value_undefined();
                let value = args.first().cloned().unwrap_or_else(|| undefined.clone());

                // Step 4.5.8.1: "Let Q be ? Get(jsValue, id)."
                let attr_key = ec.property_key_from_str(captures.attr_id);
                let q = ec.get_v(this, attr_key)?;

                // Step 4.5.8.2: "If Q is not an Object, then throw a
                //               TypeError."
                let Some(q_object) = T::value_as_object(&q) else {
                    return Err(ec.new_type_error(&format!(
                        "cannot forward assignment: attribute '{}' is not an object",
                        captures.attr_id
                    )));
                };

                // Step 4.5.8.3: "Let forwardId be the identifier argument of
                //               the [PutForwards] extended attribute."
                // Step 4.5.8.4: "Perform ? Set(Q, forwardId, V, false)."
                let forward_key = ec.property_key_from_str(captures.forward_id);
                ec.set(q_object, forward_key, value, false)?;

                // Step 4.5.8.5: "Return undefined."
                Ok(ec.value_undefined())
            }

            let setter_fn = crate::js::create_builtin_fn_with_traced_captures(
                engine,
                PutForwardsCapture {
                    attr_id: attr.id,
                    forward_id,
                    marker: std::marker::PhantomData,
                },
                put_forwards_behaviour::<Ty>,
                1,
                name_key,
                false,
            );
            desc.set = Some(setter_fn);
        }
        engine.define_property_or_throw(
            target_obj.clone(),
            engine.property_key_from_str(attr.id),
            desc,
        )?;
    }
    Ok(())
}
