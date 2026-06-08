use boa_engine::{
    Context, JsObject, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    property::PropertyDescriptor,
};

/// Describes a single operation (method) on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-operation
pub(crate) struct OperationDef {
    /// The operation's identifier.
    pub id: &'static str,

    /// The `length` property value: length of the shortest argument list
    /// in the effective overload set for argument count 0.
    pub length: usize,

    /// The method steps: given `this`, the JavaScript argument values, and
    /// the context, returns the converted result.
    ///
    /// This function pointer must downcast `this` to the platform object
    /// type internally.
    pub method: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,

    /// Whether the operation is static.
    pub static_: bool,

    /// Whether the operation is unforgeable.
    pub unforgeable: bool,

    /// Whether the return type is a promise type.
    pub promise_type: bool,
}

/// <https://webidl.spec.whatwg.org/#define-the-regular-operations>
pub(crate) fn define_regular_operations(
    proto: &JsObject,
    context: &mut Context,
    operations: &[OperationDef],
) -> JsResult<()> {
    // Step 1: "Let operations be the list of regular operations that are
    //          members of definition."
    // Step 2: "Remove from operations all the operations that are
    //          unforgeable."
    let regular: Vec<&OperationDef> = operations
        .iter()
        .filter(|o| !o.static_ && !o.unforgeable)
        .collect();

    // Step 3: "Define the operations operations of definition on target
    //          given realm."
    define_operations_on_target(proto, context, &regular)
}

/// Define the static operations on the interface object.
///
/// https://webidl.spec.whatwg.org/#define-the-static-operations
pub(crate) fn define_static_operations(
    _constructor: &JsObject,
    _context: &mut Context,
    _operations: &[OperationDef],
) -> JsResult<()> {
    Ok(())
}

/// <https://webidl.spec.whatwg.org/#define-the-operations>
fn define_operations_on_target(
    proto: &JsObject,
    context: &mut Context,
    operations: &[&OperationDef],
) -> JsResult<()> {
    let realm = context.realm().clone();

    // Step 1: "For each operation op of operations:"
    for op in operations {
        // Step 1.1: "If op is not exposed in realm, then continue."
        // Note: Exposure checks are not yet implemented.

        // Step 1.2: "Let method be the result of creating an operation
        //            function given op, definition, and realm."
        let method = NativeFunction::from_fn_ptr(op.method).to_js_function(&realm);

        // Step 1.3: "Let modifiable be false if op is unforgeable
        //            and true otherwise."
        let modifiable = !op.unforgeable;

        // Step 1.4: "Let desc be the PropertyDescriptor{[[Value]]: method,
        //            [[Writable]]: modifiable, [[Enumerable]]: true,
        //            [[Configurable]]: modifiable}."
        let desc = PropertyDescriptor::builder()
            .value(method)
            .writable(modifiable)
            .enumerable(true)
            .configurable(modifiable)
            .build();

        // Step 1.5: "Let id be op's identifier."
        // Step 1.6: "Perform ! DefinePropertyOrThrow(target, id, desc)."
        proto.define_property_or_throw(js_string!(op.id), desc, context)?;
    }

    Ok(())
}
