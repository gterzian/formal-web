use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes, PromiseResolvers};

use crate::js::Types;

use super::writablestreamdefaultcontroller::WritableStreamDefaultController;
use super::writablestreamdefaultwriter::WritableStreamDefaultWriter;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://streams.spec.whatwg.org/#writablestream-state>
#[gc_struct]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WritableStreamState {
    Writable,
    Erroring,
    Closed,
    Errored,
}

#[gc_struct]
pub(crate) struct WriteRequest {
    resolvers: PromiseResolvers<Types>,
}

impl WriteRequest {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Completion<(Self, JsObject), Types> {
        let (promise, resolvers) = ec.new_promise_pending()?;
        let promise_obj = promise
            .as_object()
            .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
        Ok((Self { resolvers }, promise_obj))
    }
    pub(crate) fn resolve(self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let undefined = ec.value_undefined();
        let args = [undefined];
        ec.call(&self.resolvers.resolve, &args[0], &args)
            .map(|_| ())
    }
    pub(crate) fn reject(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let undefined = ec.value_undefined();
        ec.call(&self.resolvers.reject, &undefined, &[error])
            .map(|_| ())
    }
}
#[gc_struct]
pub(crate) struct PendingAbortRequest {
    promise: JsObject,
    resolvers: PromiseResolvers<Types>,

    /// <https://streams.spec.whatwg.org/#pending-abort-request-reason>
    reason: JsValue,

    /// <https://streams.spec.whatwg.org/#pending-abort-request-was-already-erroring>
    was_already_erroring: bool,
}

impl PendingAbortRequest {
    pub(crate) fn new(
        reason: JsValue,
        was_already_erroring: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let (promise, resolvers) = ec.new_promise_pending()?;
        let promise_obj = promise
            .as_object()
            .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
        Ok(Self {
            promise: promise_obj,
            resolvers,
            reason,
            was_already_erroring,
        })
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
    pub(crate) fn resolve(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let undefined = ec.value_undefined();
        let args = [undefined];
        ec.call(&self.resolvers.resolve, &args[0], &args)
            .map(|_| ())
    }
    pub(crate) fn reject(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let undefined = ec.value_undefined();
        ec.call(&self.resolvers.reject, &undefined, &[error])
            .map(|_| ())
    }
}
#[gc_struct]
pub(crate) enum WritableStreamController {
    Default(WritableStreamDefaultController),
}

impl WritableStreamController {
    pub(crate) fn abort_steps(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        match self {
            Self::Default(controller) => controller.abort_steps(reason, ec),
        }
    }
    pub(crate) fn error_steps(&self) {
        match self {
            Self::Default(controller) => controller.error_steps(),
        }
    }
    pub(crate) fn signal_abort(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        match self {
            Self::Default(controller) => controller.signal_abort(reason, ec),
        }
    }
    pub(crate) fn as_default_controller(&self) -> WritableStreamDefaultController {
        match self {
            Self::Default(controller) => controller.clone(),
        }
    }
}
#[gc_struct]
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
