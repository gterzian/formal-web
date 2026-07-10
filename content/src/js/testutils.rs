/// <https://testutils.spec.whatwg.org/#the-testutils-namespace>
///
/// Installs the `TestUtils` namespace on the global object using only the
/// generic [`ExecutionContext`] trait — no engine-specific APIs.
pub(crate) fn install_testutils_namespace(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: Create the TestUtils namespace object.
    let testutils_obj = ec.create_plain_object(None);

    // Step 2: Install the gc() method.
    // <https://testutils.spec.whatwg.org/#dom-testutils-gc>
    let gc_fn = ec.create_builtin_fn(
        Box::new(
            move |_args: &[<Types as js_engine::JsTypes>::JsValue],
                  _this: <Types as js_engine::JsTypes>::JsValue,
                  ec: &mut dyn ExecutionContext<Types>|
                  -> Completion<<Types as js_engine::JsTypes>::JsValue, Types> {
                // Step 1: "Let p be a new promise."
                // Note: In a single-threaded content process, garbage collection
                // runs synchronously.  The spec's "in parallel" is approximated
                // by triggering GC immediately and returning a resolved promise.
                //
                // Step 2: "Run the following in parallel:"
                // Step 2.1: "Run implementation-defined steps to perform a
                // garbage collection covering at least the entry Realm."
                ec.gc();

                // Step 2.2: "Resolve p."
                ec.evaluate_script("Promise.resolve()")
            },
        ),
        0,
        ec.property_key_from_str("gc"),
    );

    ec.set(
        testutils_obj.clone(),
        ec.property_key_from_str("gc"),
        <Types as js_engine::JsTypes>::value_from_object(
            <Types as js_engine::JsTypes>::object_from_function(gc_fn),
        ),
        false,
    )?;

    // Step 3: Register on global as `TestUtils`.
    let global = ec.realm_global_object();
    ec.set(
        global,
        ec.property_key_from_str("TestUtils"),
        <Types as js_engine::JsTypes>::value_from_object(testutils_obj),
        false,
    )
}

use js_engine::{Completion, ExecutionContext};

use crate::js::Types;
