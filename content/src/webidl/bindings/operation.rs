use std::marker::PhantomData;

use js_engine::{
    Completion, ExecutionContext, JsEngine, JsTypes, JsTypesWithRealm, PropertyDescriptor,
};

/// Describes a single operation (method) on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-operation
///
/// Generic fn pointer: receives `&Ty::JsValue`, `&[Ty::JsValue]`, and
/// `&mut dyn ExecutionContext<Ty>`.  Binding functions use
/// `Ty::value_as_object` and `ec.with_platform_data` for upcast/downcast,
/// avoiding engine-specific dependencies.
pub(crate) struct OperationDef<T: JsTypes> {
    pub id: &'static str,
    pub length: usize,
    pub method:
        fn(&T::JsValue, &[T::JsValue], &mut dyn ExecutionContext<T>) -> Completion<T::JsValue, T>,
    pub static_: bool,
    pub unforgeable: bool,
    pub promise_type: bool,
    pub _phantom: PhantomData<T>,
}

/// <https://webidl.spec.whatwg.org/#define-the-regular-operations>
pub(crate) fn define_regular_operations<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    operations: &[OperationDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let regular: Vec<&OperationDef<Ty>> = operations
        .iter()
        .filter(|o| !o.static_ && !o.unforgeable)
        .collect();
    define_operations_on_target(engine, target, &regular)
}

/// <https://webidl.spec.whatwg.org/#define-the-static-operations>
pub(crate) fn define_static_operations<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    operations: &[OperationDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let static_ops: Vec<&OperationDef<Ty>> = operations.iter().filter(|o| o.static_).collect();
    define_operations_on_target(engine, target, &static_ops)
}

fn define_operations_on_target<Ty, E>(
    engine: &mut E,
    target: &Ty::JsValue,
    operations: &[&OperationDef<Ty>],
) -> Completion<(), Ty>
where
    Ty: JsTypes + JsTypesWithRealm,
    E: JsEngine<Ty> + ExecutionContext<Ty>,
{
    let target_obj = Ty::value_as_object(target)
        .ok_or_else(|| engine.new_type_error("target is not an object in operation definition"))?;

    for op in operations {
        let method = engine.create_builtin_fn(
            Box::new({
                let op_method = op.method;
                move |args, this, ec| op_method(&this, args, ec)
            }),
            op.length as u32,
            engine.property_key_from_str(op.id),
        );
        let modifiable = !op.unforgeable;
        let desc = PropertyDescriptor {
            value: Some(Ty::value_from_object(Ty::object_from_function(method))),
            get: None,
            set: None,
            writable: Some(modifiable),
            enumerable: Some(true),
            configurable: Some(modifiable),
        };
        engine.define_property_or_throw(
            target_obj.clone(),
            engine.property_key_from_str(op.id),
            desc,
        )?;
    }
    Ok(())
}
