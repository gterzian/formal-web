use boa_engine::{
    builtins::promise::ResolvingFunctions,
    job::PromiseJob,
    js_string,
    object::{
        builtins::{JsFunction, JsPromise},
        JsObject, ObjectInitializer,
    },
    property::Attribute,
    Context, JsNativeError, JsResult, JsString, JsValue,
};
use js_engine::boa::BoaTypes;

use boa_gc::{Finalize, Gc, GcRefCell, Trace};
use log::error;

use crate::webidl::{
    invoke_callback_function, mark_promise_as_handled, rejected_promise, Callback, EcmascriptHost,
    ExceptionBehavior,
};

use super::readablebytestreamcontroller::ReadableByteStreamController;
use super::readablestream::{
    readable_byte_stream_tee_default_reader_chunk_steps,
    readable_byte_stream_tee_default_reader_close_steps,
    readable_byte_stream_tee_default_reader_error_steps,
    readable_stream_default_tee_read_request_chunk_steps,
    readable_stream_default_tee_read_request_close_steps,
    readable_stream_default_tee_read_request_error_steps, ByteTeeState, PipeToState, TeeState,
};
use super::readablestreambyobreader::ReadableStreamBYOBReader;
use super::readablestreamdefaultcontroller::ReadableStreamDefaultController;
use super::readablestreamdefaultreader::ReadableStreamDefaultReader;

/// <https://streams.spec.whatwg.org/#readablestream-state>
#[derive(Clone, Debug, Eq, PartialEq, Trace, Finalize)]
pub(crate) enum ReadableStreamState {
    Readable,
    Closed,
    Errored,
}
/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller-from-underlying-source>
// Note: This struct stores an underlying source method together with its callback `this` value so the surrounding Streams algorithms can later invoke it through Web IDL callback invocation.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct SourceMethod {
    this_value: JsObject,

    /// <https://webidl.spec.whatwg.org/#idl-callback-function>
    callback: Callback,
}

impl SourceMethod {
    pub(crate) fn new(this_value: JsObject, callback: Callback) -> Self {
        Self {
            this_value,
            callback,
        }
    }

    /// <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
    // Note: The local CtxHost adapter wraps &mut Context to implement EcmascriptHost.
    // This is a migration artifact — Phase 4 will thread Engine directly.
    pub(crate) fn call(&self, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        struct CtxHost<'a>(&'a mut Context);
        impl EcmascriptHost<BoaTypes> for CtxHost<'_> {
            fn get(
                &mut self,
                object: &JsObject,
                property: &str,
            ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes> {
                object
                    .get(JsString::from(property), self.0)
                    .map_err(|e| e.into_opaque(self.0).unwrap_or(JsValue::undefined()))
            }
            fn is_callable(&self, value: &JsValue) -> bool {
                value.as_object().is_some_and(|o| o.is_callable())
            }
            fn call(
                &mut self,
                callable: &JsObject,
                this_arg: &JsValue,
                args: &[JsValue],
            ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes> {
                let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
                    JsValue::from(
                        JsNativeError::typ()
                            .with_message("callback is not callable")
                            .into_opaque(self.0),
                    )
                })?;
                function
                    .call(this_arg, args, self.0)
                    .map_err(|e| e.into_opaque(self.0).unwrap_or(JsValue::undefined()))
            }
            fn perform_a_microtask_checkpoint(
                &mut self,
            ) -> js_engine::Completion<(), js_engine::boa::BoaTypes> {
                let _ = self.0.run_jobs();
                Ok(())
            }
            fn report_exception(&mut self, error: JsValue) {
                log::error!("uncaught callback error: {error:?}");
            }
            fn value_undefined(&mut self) -> JsValue { JsValue::undefined() }
            fn value_null(&mut self) -> JsValue { JsValue::null() }
            fn value_from_bool(&mut self, b: bool) -> JsValue { JsValue::from(b) }
            fn value_from_number(&mut self, n: f64) -> JsValue { JsValue::from(n) }
            fn value_from_string(&mut self, s: boa_engine::JsString) -> JsValue { JsValue::from(s) }
            fn js_string_from_str(&self, s: &str) -> boa_engine::JsString { boa_engine::js_string!(s) }
        }
        let mut host = CtxHost(context);
        let this_value = JsValue::from(self.this_value.clone());
        invoke_callback_function(
            &mut host,
            &self.callback,
            args,
            ExceptionBehavior::Rethrow,
            Some(&this_value),
        )
    }
}
// A read request either produces a chunk, closes, or errors.
#[derive(Clone, Trace, Finalize)]
pub(crate) enum ReadRequest {
    DefaultReaderRead {
        resolvers: ResolvingFunctions,
    },
    ReadableStreamDefaultTee {
        tee_state: Gc<GcRefCell<TeeState>>,
        clone_for_branch2: bool,
    },
    ReadableByteStreamTee {
        tee_state: Gc<GcRefCell<ByteTeeState>>,
    },
    ReadableStreamPipeTo {
        state: PipeToState,
    },
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadIntoRequest {
    resolvers: ResolvingFunctions,
}

impl ReadIntoRequest {
    pub(crate) fn new(context: &mut Context) -> (Self, JsObject) {
        let (promise, resolvers) = JsPromise::new_pending(context);
        (Self { resolvers }, promise.into())
    }

    pub(crate) fn chunk_steps(self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        let result = create_read_result(chunk, false, context)?;
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[result], context)?;
        Ok(())
    }

    pub(crate) fn close_steps(self, chunk: Option<JsValue>, context: &mut Context) -> JsResult<()> {
        let result = create_read_result(chunk.unwrap_or(JsValue::undefined()), true, context)?;
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[result], context)?;
        Ok(())
    }

    pub(crate) fn error_steps(self, error: JsValue, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .reject
            .call(&JsValue::undefined(), &[error], context)?;
        Ok(())
    }
}

impl ReadRequest {
    pub(crate) fn chunk_steps(self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                let result = create_read_result(chunk, false, context)?;
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[result], context)?;
                Ok(())
            }
            Self::ReadableStreamDefaultTee {
                tee_state,
                clone_for_branch2,
            } => readable_stream_default_tee_read_request_chunk_steps(
                tee_state.clone(),
                *clone_for_branch2,
                chunk,
                context,
            ),
            Self::ReadableByteStreamTee { tee_state } => {
                readable_byte_stream_tee_default_reader_chunk_steps(
                    tee_state.clone(),
                    chunk,
                    context,
                )
            }
            Self::ReadableStreamPipeTo { state } => {
                let result = create_read_result(chunk, false, context)?;
                let state = state.clone();
                queue_internal_stream_microtask(
                    move |context| state.on_read_request_settled(result, context),
                    context,
                )
            }
        }
    }

    /// closure.
    pub(crate) fn close_steps(self, context: &mut Context) -> JsResult<()> {
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                let result = create_read_result(JsValue::undefined(), true, context)?;
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[result], context)?;
                Ok(())
            }
            Self::ReadableStreamDefaultTee { tee_state, .. } => {
                readable_stream_default_tee_read_request_close_steps(tee_state.clone(), context)
            }
            Self::ReadableByteStreamTee { tee_state } => {
                readable_byte_stream_tee_default_reader_close_steps(tee_state.clone(), context)
            }
            Self::ReadableStreamPipeTo { state } => {
                let result = create_read_result(JsValue::undefined(), true, context)?;
                let state = state.clone();
                queue_internal_stream_microtask(
                    move |context| state.on_read_request_settled(result, context),
                    context,
                )
            }
        }
    }

    pub(crate) fn error_steps(self, error: JsValue, context: &mut Context) -> JsResult<()> {
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[error], context)?;
                Ok(())
            }
            Self::ReadableStreamDefaultTee { tee_state, .. } => {
                readable_stream_default_tee_read_request_error_steps(tee_state.clone(), context)
            }
            Self::ReadableByteStreamTee { tee_state } => {
                readable_byte_stream_tee_default_reader_error_steps(tee_state.clone(), context)
            }
            Self::ReadableStreamPipeTo { state } => {
                let state = state.clone();
                queue_internal_stream_microtask(
                    move |context| state.on_read_request_settled(error, context),
                    context,
                )
            }
        }
    }
}

pub(crate) fn queue_internal_stream_microtask<F>(task: F, context: &mut Context) -> JsResult<()>
where
    F: FnOnce(&mut Context) -> JsResult<()> + 'static,
{
    let realm = context.realm().clone();
    context.enqueue_job(
        PromiseJob::with_realm(
            move |context| {
                if let Err(error) = task(context) {
                    let reason = error
                        .into_opaque(context)
                        .unwrap_or_else(|_| JsValue::undefined());
                    if let Ok(rejected) = rejected_promise(reason, context) {
                        if let Err(error) = mark_promise_as_handled(&rejected, context) {
                            error!("[readable-stream] failed to mark promise as handled: {error}");
                        }
                    }
                }
                Ok(JsValue::undefined())
            },
            realm,
        )
        .into(),
    );
    Ok(())
}
#[derive(Clone, Trace, Finalize)]
pub(crate) enum ReadableStreamController {
    Default(ReadableStreamDefaultController),
    Byte(ReadableByteStreamController),
}

impl ReadableStreamController {
    pub(crate) fn cancel_steps(
        &self,
        reason: JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        match self {
            Self::Default(controller) => controller.cancel_steps(reason, context),
            Self::Byte(controller) => controller.cancel_steps(reason, context),
        }
    }
    pub(crate) fn pull_steps(
        &self,
        read_request: ReadRequest,
        context: &mut Context,
    ) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.pull_steps(read_request, context),
            Self::Byte(controller) => controller.pull_steps(read_request, context),
        }
    }
    pub(crate) fn release_steps(&self, context: &mut Context) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.release_steps(context),
            Self::Byte(controller) => controller.release_steps(context),
        }
    }
    /// readable-stream implementation.
    pub(crate) fn as_default_controller(&self) -> ReadableStreamDefaultController {
        match self {
            Self::Default(controller) => controller.clone(),
            Self::Byte(_) => panic!("byte controller cannot be used as a default controller"),
        }
    }

    pub(crate) fn as_byte_controller(&self) -> Option<ReadableByteStreamController> {
        match self {
            Self::Default(_) => None,
            Self::Byte(controller) => Some(controller.clone()),
        }
    }
}
#[derive(Clone, Trace, Finalize)]
pub(crate) enum ReadableStreamReader {
    Default(ReadableStreamDefaultReader),
    BYOB(ReadableStreamBYOBReader),
}

impl ReadableStreamReader {
    pub(crate) fn is_default_reader(&self) -> bool {
        matches!(self, Self::Default(_))
    }
    /// readable-stream implementation.
    pub(crate) fn as_default_reader(&self) -> Option<ReadableStreamDefaultReader> {
        match self {
            Self::Default(reader) => Some(reader.clone()),
            Self::BYOB(_) => None,
        }
    }

    pub(crate) fn as_byob_reader(&self) -> Option<ReadableStreamBYOBReader> {
        match self {
            Self::Default(_) => None,
            Self::BYOB(reader) => Some(reader.clone()),
        }
    }
}
fn create_read_result(value: JsValue, done: bool, context: &mut Context) -> JsResult<JsValue> {
    let mut initializer = ObjectInitializer::new(context);
    initializer.property(js_string!("value"), value, Attribute::all());
    initializer.property(js_string!("done"), done, Attribute::all());
    Ok(JsValue::from(initializer.build()))
}

pub(crate) fn rejected_type_error_promise(
    message: &'static str,
    context: &mut Context,
) -> JsResult<JsObject> {
    crate::webidl::rejected_promise(type_error_value(message, context)?, context)
}
pub(crate) fn type_error_value(message: &'static str, context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(
        JsNativeError::typ()
            .with_message(message)
            .into_opaque(context),
    ))
}
pub(crate) fn range_error_value(message: &'static str, context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(
        JsNativeError::range()
            .with_message(message)
            .into_opaque(context),
    ))
}
