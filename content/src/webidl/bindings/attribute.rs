use std::marker::PhantomData;

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
    pub _phantom: PhantomData<T>,
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
    let realm = engine.current_realm();
    let target_obj = Ty::value_as_object(target)
        .ok_or_else(|| engine.new_type_error("target is not an object in attribute definition"))?;
    for attr in attributes {
        let getter_fn = engine.create_builtin_function(
            Box::new({
                let getter = attr.getter;
                move |args, this, ec| getter(&this, args, ec)
            }),
            0,
            engine.property_key_from_str(attr.id),
            &realm,
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
            let setter_fn = engine.create_builtin_function(
                Box::new({ move |args, this, ec| setter(&this, args, ec) }),
                1,
                engine.property_key_from_str(attr.id),
                &realm,
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
