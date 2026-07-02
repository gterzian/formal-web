use boa_engine::{
    Context, JsResult, JsValue,
    job::PromiseJob,
    js_string,
    object::{JsObject, ObjectInitializer},
    property::Attribute,
};

use js_engine::{Completion, ExecutionContext, JsTypes, PromiseResolvers};

use crate::webidl::{
    Callback, ExceptionBehavior, invoke_callback_function, mark_promise_as_handled,
    rejected_promise,
};
use boa_gc::{Finalize, Trace};

use super::readablebytestreamcontroller::ReadableByteStreamController;
use super::readablestream::{
    ByteTeeState, PipeToState, TeeState, readable_byte_stream_tee_default_reader_chunk_steps,
    readable_byte_stream_tee_default_reader_close_steps,
    readable_byte_stream_tee_default_reader_error_steps,
    readable_stream_default_tee_read_request_chunk_steps,
    readable_stream_default_tee_read_request_close_steps,
    readable_stream_default_tee_read_request_error_steps,
};
use super::readablestreambyobreader::ReadableStreamBYOBReader;
use super::readablestreamdefaultcontroller::ReadableStreamDefaultController;
use super::readablestreamdefaultreader::ReadableStreamDefaultReader;
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;

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
    pub(crate) fn call(
        &self,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let this_value = JsValue::from(self.this_value.clone());
        invoke_callback_function(
            ec,
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
        resolvers: PromiseResolvers<crate::js::Types>,
    },
    ReadableStreamDefaultTee {
        tee_state: GcCell<TeeState>,
        clone_for_branch2: bool,
    },
    ReadableByteStreamTee {
        tee_state: GcCell<ByteTeeState>,
    },
    ReadableStreamPipeTo {
        state: PipeToState,
    },
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadIntoRequest {
    resolvers: PromiseResolvers<crate::js::Types>,
}

impl ReadIntoRequest {
    pub(crate) fn new(
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(Self, JsObject), crate::js::Types> {
        let (promise, resolvers) = ec.new_promise_pending()?;
        let promise_obj = promise
            .as_object()
            .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
        Ok((Self { resolvers }, promise_obj))
    }

    pub(crate) fn chunk_steps(
        self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let result = create_read_result(chunk, false, ec)?;
        ec.call(&self.resolvers.resolve, &JsValue::undefined(), &[result])
            .map(|_| ())
    }

    pub(crate) fn close_steps(
        self,
        chunk: Option<JsValue>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let result = create_read_result(chunk.unwrap_or(JsValue::undefined()), true, ec)?;
        ec.call(&self.resolvers.resolve, &JsValue::undefined(), &[result])
            .map(|_| ())
    }

    pub(crate) fn error_steps(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        ec.call(&self.resolvers.reject, &JsValue::undefined(), &[error])
            .map(|_| ())
    }
}

impl ReadRequest {
    pub(crate) fn chunk_steps(
        self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                let result = create_read_result(chunk, false, ec)?;
                ec.call(&resolvers.resolve, &JsValue::undefined(), &[result])
                    .map(|_| ())
            }
            Self::ReadableStreamDefaultTee {
                tee_state,
                clone_for_branch2,
            } => readable_stream_default_tee_read_request_chunk_steps(
                tee_state.clone(),
                *clone_for_branch2,
                chunk,
                ec,
            ),
            Self::ReadableByteStreamTee { tee_state } => {
                // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
                // Tee algorithms still take Boa's Context.
                let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
                crate::js::js_result_to_completion(
                    readable_byte_stream_tee_default_reader_chunk_steps(
                        tee_state.clone(),
                        chunk,
                        context,
                    ),
                    context,
                )
            }
            Self::ReadableStreamPipeTo { state } => {
                let result = create_read_result(chunk, false, ec)?;
                let state = state.clone();
                let realm = ec.current_realm();
                ec.enqueue_job_with_realm(
                    realm,
                    Box::new(move |job_ec: &mut dyn ExecutionContext<crate::js::Types>| {
                        let _ = state.on_read_request_settled(result, job_ec);
                    }),
                );
                Ok(())
            }
        }
    }

    pub(crate) fn close_steps(
        self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                let result = create_read_result(JsValue::undefined(), true, ec)?;
                ec.call(&resolvers.resolve, &JsValue::undefined(), &[result])
                    .map(|_| ())
            }
            Self::ReadableStreamDefaultTee { tee_state, .. } => {
                readable_stream_default_tee_read_request_close_steps(tee_state.clone(), ec)
            }
            Self::ReadableByteStreamTee { tee_state } => {
                readable_byte_stream_tee_default_reader_close_steps(tee_state.clone(), ec)
            }
            Self::ReadableStreamPipeTo { state } => {
                let result = create_read_result(JsValue::undefined(), true, ec)?;
                let state = state.clone();
                let realm = ec.current_realm();
                ec.enqueue_job_with_realm(
                    realm,
                    Box::new(move |job_ec: &mut dyn ExecutionContext<crate::js::Types>| {
                        let _ = state.on_read_request_settled(result, job_ec);
                    }),
                );
                Ok(())
            }
        }
    }

    pub(crate) fn error_steps(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self {
            Self::DefaultReaderRead { resolvers } => ec
                .call(&resolvers.reject, &JsValue::undefined(), &[error])
                .map(|_| ()),
            Self::ReadableStreamDefaultTee { tee_state, .. } => {
                readable_stream_default_tee_read_request_error_steps(tee_state.clone());
                Ok(())
            }
            Self::ReadableByteStreamTee { tee_state } => {
                readable_byte_stream_tee_default_reader_error_steps(tee_state.clone());
                Ok(())
            }
            Self::ReadableStreamPipeTo { state } => {
                let state = state.clone();
                let realm = ec.current_realm();
                ec.enqueue_job_with_realm(
                    realm,
                    Box::new(move |job_ec: &mut dyn ExecutionContext<crate::js::Types>| {
                        let _ = state.on_read_request_settled(error, job_ec);
                    }),
                );
                Ok(())
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
                    if let Ok(rejected) =
                        rejected_promise(reason, js_engine::boa::context_as_ec(context))
                    {
                        if let Err(error) = mark_promise_as_handled(
                            &rejected,
                            js_engine::boa::context_as_ec(context),
                        ) {
                            log::warn!(
                                "[readable-stream] failed to mark promise as handled: {:?}",
                                error
                            );
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
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        match self {
            Self::Default(controller) => controller.cancel_steps(reason, ec),
            Self::Byte(controller) => controller.cancel_steps(reason, ec),
        }
    }
    pub(crate) fn pull_steps(
        &self,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match self {
            Self::Default(controller) => controller.pull_steps(read_request, ec),
            Self::Byte(controller) => controller.pull_steps(read_request, ec),
        }
    }
    pub(crate) fn release_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match self {
            Self::Default(controller) => controller.release_steps(ec),
            Self::Byte(controller) => controller.release_steps(ec),
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
fn create_read_result(
    value: JsValue,
    done: bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = ec.create_plain_object(None);
    let done_val = ec.value_from_bool(done);
    ec.object_set_property(obj.clone(), "value", value)?;
    ec.object_set_property(obj.clone(), "done", done_val)?;
    Ok(<crate::js::Types as JsTypes>::value_from_object(obj))
}

pub(crate) fn rejected_type_error_promise(
    message: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let reason = type_error_value(message, ec)?;
    crate::webidl::rejected_promise(reason, ec)
}

pub(crate) fn type_error_value(
    message: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(ec.new_type_error(message))
}

pub(crate) fn range_error_value(
    message: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(ec.new_range_error(message))
}
