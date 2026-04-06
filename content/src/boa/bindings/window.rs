use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
};

use crate::dom::Window;

use super::event_target::register_event_target_methods;

impl Class for Window {
    const NAME: &'static str = "Window";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ().with_message("Illegal constructor").into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)
    }
}