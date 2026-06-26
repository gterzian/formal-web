//! Engine adapter — provides the bridge between `js_engine` and `content/`.
//!
//! # Migration
//!
//! During the migration from `boa_engine::Context` to `JsEngine<T>`, this module
//! provides the bridge.  Domain code should import from here rather than from
//! `js_engine` directly, so that the import path stays fixed when the engine
//! becomes generic.
//!
//! ## Current state
//!
//! Phase 2 of the migration (see `scratchpad/js-engine-abstraction.md`).
//! `BoaEngineHost` wraps `&mut BoaEngine` and implements `EcmascriptHost`,
//! allowing incremental replacement of `ContextCallbackHost` at call sites.

use boa_engine::{
    Context, JsError, JsResult, JsValue,
    object::{JsObject, builtins::JsFunction},
};
use js_engine::{BoaEngine, JsEngine};

use crate::webidl::{Callback, EcmascriptHost};

/// A host that wraps `&mut BoaEngine` and implements `EcmascriptHost`.
///
/// Replaces `ContextCallbackHost` at migrated call sites.  The `BoaEngine`
/// wraps a `boa_engine::Context` and implements `JsEngine<js_engine::BoaTypes>`,
/// so all ECMA-262 operations go through the generic trait.
pub(crate) struct BoaEngineHost<'a> {
    engine: &'a mut BoaEngine,
    exception_context: &'static str,
}

impl<'a> BoaEngineHost<'a> {
    pub(crate) fn new(engine: &'a mut BoaEngine, exception_context: &'static str) -> Self {
        Self {
            engine,
            exception_context,
        }
    }

    /// Access the underlying Boa context for operations that are not yet
    /// abstracted through `JsEngine`.
    pub(crate) fn context(&mut self) -> &mut Context {
        self.engine.context()
    }
}

impl EcmascriptHost for BoaEngineHost<'_> {
    fn context(&mut self) -> &mut Context {
        self.engine.context()
    }

    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        object.get(boa_engine::js_string!(property), self.engine.context())
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        self.engine.is_callable(value)
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsError::from(
                boa_engine::JsNativeError::typ()
                    .with_message("callback is not callable"),
            )
        })?;
        function.call(this_arg, args, self.engine.context())
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        self.engine.run_jobs();
        Ok(())
    }

    fn report_exception(&mut self, error: JsError, _callback: &Callback) {
        log::error!("uncaught {} error: {error}", self.exception_context);
    }
}
