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
        }
        engine.define_property_or_throw(
            target_obj.clone(),
            engine.property_key_from_str(attr.id),
            desc,
        )?;
    }
    Ok(())
}
