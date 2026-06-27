use std::marker::PhantomData;

use boa_engine::{
    js_string, native_function::NativeFunction, property::PropertyDescriptor, Context, JsResult,
    JsValue,
};

use js_engine::JsTypes;

/// Describes a single operation (method) on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-operation
///
/// Generic over `T: JsTypes` so that the bindings infrastructure can
/// verify type consistency at compile time.  The fn pointer remains
/// Boa-concrete for now; it will receive `&mut dyn ExecutionContext<T>`
/// in Phase 3 (domain threading).
pub(crate) struct OperationDef<T: JsTypes> {
    pub id: &'static str,
    pub length: usize,
    pub method: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
    pub static_: bool,
    pub unforgeable: bool,
    pub promise_type: bool,
    pub _phantom: PhantomData<T>,
}

/// <https://webidl.spec.whatwg.org/#define-the-regular-operations>
pub(crate) fn define_regular_operations(
    proto: &JsValue,
    context: &mut Context,
    operations: &[OperationDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let regular: Vec<&OperationDef<js_engine::boa::BoaTypes>> = operations
        .iter()
        .filter(|o| !o.static_ && !o.unforgeable)
        .collect();
    define_operations_on_target(proto, context, &regular)
}

/// <https://webidl.spec.whatwg.org/#define-the-unforgeable-regular-operations>
pub(crate) fn define_unforgeable_regular_operations(
    proto: &JsValue,
    context: &mut Context,
    operations: &[OperationDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let unforgeable: Vec<&OperationDef<js_engine::boa::BoaTypes>> = operations
        .iter()
        .filter(|o| o.unforgeable && !o.static_)
        .collect();
    define_operations_on_target(proto, context, &unforgeable)
}

/// <https://webidl.spec.whatwg.org/#define-the-static-operations>
pub(crate) fn define_static_operations(
    constructor: &JsValue,
    context: &mut Context,
    operations: &[OperationDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let static_ops: Vec<&OperationDef<js_engine::boa::BoaTypes>> =
        operations.iter().filter(|o| o.static_).collect();
    define_operations_on_target(constructor, context, &static_ops)
}

/// <https://webidl.spec.whatwg.org/#define-the-operations>
fn define_operations_on_target(
    proto: &JsValue,
    context: &mut Context,
    operations: &[&OperationDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let realm = context.realm().clone();

    for op in operations {
        let method = NativeFunction::from_fn_ptr(op.method).to_js_function(&realm);

        let proto_obj = proto.as_object().ok_or_else(|| {
            boa_engine::JsNativeError::typ()
                .with_message("target is not an object in operation definition")
        })?;

        let modifiable = !op.unforgeable;

        let desc = PropertyDescriptor::builder()
            .value(method)
            .writable(modifiable)
            .enumerable(true)
            .configurable(modifiable)
            .build();

        proto_obj.define_property_or_throw(js_string!(op.id), desc, context)?;
    }

    Ok(())
}
