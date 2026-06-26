//! Thin `EcmascriptHost<BoaTypes>` adapter over `&mut Context`.
//!
//! Streams algorithms receive `&mut Context` for Boa-specific operations
//! (promise creation, downcast, etc.) but also need to call Web IDL callback
//! operations through `EcmascriptHost<BoaTypes>`.  This adapter bridges
//! from `&mut Context` to a `&mut impl EcmascriptHost<BoaTypes>` without
//! requiring a full `BoaEngine` wrapper.

use boa_engine::{
    object::{builtins::JsFunction, JsObject},
    Context, JsNativeError, JsString, JsValue,
};
use js_engine::{BoaTypes, Completion, EcmascriptHost};

/// Thin adapter: `EcmascriptHost<BoaTypes>` over a `&mut Context`.
pub(crate) struct ContextEcmaHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextEcmaHost<'a> {
    pub(crate) fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl EcmascriptHost<BoaTypes> for ContextEcmaHost<'_> {
    fn get(&mut self, object: &JsObject, property: &str) -> Completion<JsValue, BoaTypes> {
        object
            .get(JsString::from(property), self.context)
            .map_err(|e| e.into_opaque(self.context).unwrap_or(JsValue::undefined()))
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        value.as_object().is_some_and(|o| o.is_callable())
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> Completion<JsValue, BoaTypes> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("callback is not callable")
                    .into_opaque(self.context),
            )
        })?;
        function
            .call(this_arg, args, self.context)
            .map_err(|e| e.into_opaque(self.context).unwrap_or(JsValue::undefined()))
    }

    fn perform_a_microtask_checkpoint(&mut self) -> Completion<(), BoaTypes> {
        let _ = self.context.run_jobs();
        Ok(())
    }

    fn report_exception(&mut self, error: JsValue) {
        log::error!("uncaught callback error: {error:?}");
    }
}
