use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    job::PromiseJob,
    js_string,
    object::{JsObject, ObjectInitializer, builtins::JsPromise},
    property::Attribute,
};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

use crate::webidl::{
    Callback, ExceptionBehavior, invoke_callback_function, mark_promise_as_handled,
    rejected_promise,
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

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
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsValue, BoaTypes> {
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

    pub(crate) fn chunk_steps(
        self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        let result = create_read_result(chunk, false, context)?;
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[result], context)
            .map(|_| ())
            .map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })
    }

    pub(crate) fn close_steps(
        self,
        chunk: Option<JsValue>,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        let result = create_read_result(chunk.unwrap_or(JsValue::undefined()), true, context)?;
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[result], context)
            .map(|_| ())
            .map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })
    }

    pub(crate) fn error_steps(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        self.resolvers
            .reject
            .call(&JsValue::undefined(), &[error], context)
            .map(|_| ())
            .map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })
    }
}

impl ReadRequest {
    pub(crate) fn chunk_steps(
        self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                let result = create_read_result(chunk, false, context)?;
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[result], context)
                    .map(|_| ())
                    .map_err(|e| {
                        e.into_opaque(context)
                            .unwrap_or_else(|_| JsValue::undefined())
                    })
            }
            Self::ReadableStreamDefaultTee {
                tee_state,
                clone_for_branch2,
            } => crate::js::js_result_to_completion(
                readable_stream_default_tee_read_request_chunk_steps(
                    tee_state.clone(),
                    *clone_for_branch2,
                    chunk,
                    context,
                ),
                context,
            ),
            Self::ReadableByteStreamTee { tee_state } => {
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
                let result = create_read_result(chunk, false, context)?;
                let state = state.clone();
                queue_internal_stream_microtask(
                    move |context| state.on_read_request_settled(result, context),
                    context,
                )
                .map_err(|e| {
                    e.into_opaque(context)
                        .unwrap_or_else(|_| JsValue::undefined())
                })
            }
        }
    }

    pub(crate) fn close_steps(
        self,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                let result = create_read_result(JsValue::undefined(), true, context)?;
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[result], context)
                    .map(|_| ())
                    .map_err(|e| {
                        e.into_opaque(context)
                            .unwrap_or_else(|_| JsValue::undefined())
                    })
            }
            Self::ReadableStreamDefaultTee { tee_state, .. } => {
                crate::js::js_result_to_completion(
                    readable_stream_default_tee_read_request_close_steps(
                        tee_state.clone(),
                        context,
                    ),
                    context,
                )
            }
            Self::ReadableByteStreamTee { tee_state } => {
                crate::js::js_result_to_completion(
                    readable_byte_stream_tee_default_reader_close_steps(
                        tee_state.clone(),
                        context,
                    ),
                    context,
                )
            }
            Self::ReadableStreamPipeTo { state } => {
                let result = create_read_result(JsValue::undefined(), true, context)?;
                let state = state.clone();
                queue_internal_stream_microtask(
                    move |context| state.on_read_request_settled(result, context),
                    context,
                )
                .map_err(|e| {
                    e.into_opaque(context)
                        .unwrap_or_else(|_| JsValue::undefined())
                })
            }
        }
    }

    pub(crate) fn error_steps(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        match &self {
            Self::DefaultReaderRead { resolvers } => {
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[error], context)
                    .map(|_| ())
                    .map_err(|e| {
                        e.into_opaque(context)
                            .unwrap_or_else(|_| JsValue::undefined())
                    })
            }
            Self::ReadableStreamDefaultTee { tee_state, .. } => {
                crate::js::js_result_to_completion(
                    readable_stream_default_tee_read_request_error_steps(
                        tee_state.clone(),
                        context,
                    ),
                    context,
                )
            }
            Self::ReadableByteStreamTee { tee_state } => {
                crate::js::js_result_to_completion(
                    readable_byte_stream_tee_default_reader_error_steps(
                        tee_state.clone(),
                        context,
                    ),
                    context,
                )
            }
            Self::ReadableStreamPipeTo { state } => {
                let state = state.clone();
                queue_internal_stream_microtask(
                    move |context| state.on_read_request_settled(error, context),
                    context,
                )
                .map_err(|e| {
                    e.into_opaque(context)
                        .unwrap_or_else(|_| JsValue::undefined())
                })
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
                        rejected_promise(reason, crate::js::context_as_ec(context))
                    {
                        if let Err(error) =
                            mark_promise_as_handled(&rejected, crate::js::context_as_ec(context))
                        {
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
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsObject, BoaTypes> {
        match self {
            Self::Default(controller) => controller.cancel_steps(reason, ec),
            Self::Byte(controller) => {
                let context = unsafe { crate::js::ec_to_ctx(ec) };
                crate::js::js_result_to_completion(
                    controller.cancel_steps(reason, context),
                    context,
                )
            }
        }
    }
    pub(crate) fn pull_steps(
        &self,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        match self {
            Self::Default(controller) => controller.pull_steps(read_request, ec),
            Self::Byte(controller) => {
                let context = unsafe { crate::js::ec_to_ctx(ec) };
                crate::js::js_result_to_completion(
                    controller.pull_steps(read_request, context),
                    context,
                )
            }
        }
    }
    pub(crate) fn release_steps(
        &self,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        match self {
            Self::Default(controller) => controller.release_steps(ec),
            Self::Byte(controller) => {
                let context = unsafe { crate::js::ec_to_ctx(ec) };
                crate::js::js_result_to_completion(controller.release_steps(context), context)
            }
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    let mut initializer = ObjectInitializer::new(context);
    initializer.property(js_string!("value"), value, Attribute::all());
    initializer.property(js_string!("done"), done, Attribute::all());
    Ok(JsValue::from(initializer.build()))
}

pub(crate) fn rejected_type_error_promise(
    message: &'static str,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    let reason = type_error_value(message, ec)?;
    crate::webidl::rejected_promise(reason, ec)
}

pub(crate) fn type_error_value(
    message: &'static str,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    Ok(JsValue::from(
        JsNativeError::typ()
            .with_message(message)
            .into_opaque(context),
    ))
}

pub(crate) fn range_error_value(
    message: &'static str,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    Ok(JsValue::from(
        JsNativeError::range()
            .with_message(message)
            .into_opaque(context),
    ))
}
