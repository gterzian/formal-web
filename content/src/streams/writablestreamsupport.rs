use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Trace};

use super::writablestream::WritableStream;
use super::writablestreamdefaultcontroller::WritableStreamDefaultController;
use super::writablestreamdefaultwriter::WritableStreamDefaultWriter;

/// <https://streams.spec.whatwg.org/#writablestream-state>
#[derive(Clone, Debug, Eq, PartialEq, Trace, Finalize)]
pub(crate) enum WritableStreamState {
    Writable,
    Erroring,
    Closed,
    Errored,
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct WriteRequest {
    resolvers: ResolvingFunctions,
}

impl WriteRequest {
    pub(crate) fn new(context: &mut Context) -> (Self, JsObject) {
        let (promise, resolvers) = JsPromise::new_pending(context);
        (Self { resolvers }, promise.into())
    }
    pub(crate) fn resolve(self, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
        Ok(())
    }
    pub(crate) fn reject(self, error: JsValue, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .reject
            .call(&JsValue::undefined(), &[error], context)?;
        Ok(())
    }
}
#[derive(Clone, Trace, Finalize)]
pub(crate) struct PendingAbortRequest {
    promise: JsObject,
    resolvers: ResolvingFunctions,

    /// <https://streams.spec.whatwg.org/#pending-abort-request-reason>
    reason: JsValue,

    /// <https://streams.spec.whatwg.org/#pending-abort-request-was-already-erroring>
    was_already_erroring: bool,
}

impl PendingAbortRequest {
    pub(crate) fn new(
        reason: JsValue,
        was_already_erroring: bool,
        context: &mut Context,
    ) -> Self {
        let (promise, resolvers) = JsPromise::new_pending(context);
        Self {
            promise: promise.into(),
            resolvers,
            reason,
            was_already_erroring,
        }
    }
    pub(crate) fn promise(&self) -> JsObject {
        self.promise.clone()
    }
    pub(crate) fn reason(&self) -> JsValue {
        self.reason.clone()
    }
    pub(crate) fn was_already_erroring(&self) -> bool {
        self.was_already_erroring
    }
    pub(crate) fn resolve(&self, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
        Ok(())
    }
    pub(crate) fn reject(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .reject
            .call(&JsValue::undefined(), &[error], context)?;
        Ok(())
    }
}
#[derive(Clone, Trace, Finalize)]
pub(crate) enum WritableStreamController {
    Default(WritableStreamDefaultController),
}

impl WritableStreamController {
    pub(crate) fn abort_steps(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        match self {
            Self::Default(controller) => controller.abort_steps(reason, context),
        }
    }
    pub(crate) fn error_steps(&self) {
        match self {
            Self::Default(controller) => controller.error_steps(),
        }
    }
    pub(crate) fn signal_abort(&self, reason: JsValue, context: &mut Context) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.signal_abort(reason, context),
        }
    }
    pub(crate) fn as_default_controller(&self) -> WritableStreamDefaultController {
        match self {
            Self::Default(controller) => controller.clone(),
        }
    }
}
#[derive(Clone, Trace, Finalize)]
pub(crate) enum WritableStreamWriter {
    Default(WritableStreamDefaultWriter),
}

impl WritableStreamWriter {
    pub(crate) fn as_default_writer(&self) -> Option<WritableStreamDefaultWriter> {
        match self {
            Self::Default(writer) => Some(writer.clone()),
        }
    }
}