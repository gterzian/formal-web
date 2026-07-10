/// <https://testutils.spec.whatwg.org/#the-testutils-namespace>
///
/// Bindings layer: defines which members the namespace exposes.
/// All algorithm logic lives in `content/src/testutils/`.
use crate::testutils::TestUtils;
use crate::webidl::bindings::{
    InterfaceDefinition, OperationDef, WebIdlNamespace, register_namespace_spec,
};
use js_engine::{Completion, ExecutionContext, JsEngine, JsTypes};

/// Marker type for the `TestUtils` namespace.
struct TestUtilsNamespace;

impl WebIdlNamespace<crate::js::Types> for TestUtilsNamespace {
    const NAME: &'static str = "TestUtils";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // <https://testutils.spec.whatwg.org/#dom-testutils-gc>
        def.add_operation(OperationDef {
            id: "gc",
            length: 0,
            method: gc_fn,
            static_: false,
            unforgeable: false,
            promise_type: true,
        });
    }
}

/// <https://testutils.spec.whatwg.org/#the-testutils-namespace>
pub(crate) fn install_testutils_namespace<E>(engine: &mut E) -> Completion<(), crate::js::Types>
where
    E: JsEngine<crate::js::Types> + ExecutionContext<crate::js::Types>,
{
    register_namespace_spec::<crate::js::Types, TestUtilsNamespace, E>(engine)
}

/// <https://testutils.spec.whatwg.org/#dom-testutils-gc>
fn gc_fn(
    _this: &<crate::js::Types as JsTypes>::JsValue,
    _args: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    TestUtils::gc(ec)
}
