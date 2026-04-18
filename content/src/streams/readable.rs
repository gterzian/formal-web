mod default_controller;
mod default_reader;
mod stream;

use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    js_string,
    native_function::NativeFunction,
    object::{
        JsObject, ObjectInitializer,
        builtins::{JsFunction, JsPromise},
    },
    property::Attribute,
};
use boa_gc::{Finalize, Trace};

use crate::webidl::{EcmascriptHost, ExceptionBehavior, invoke_callback_function};

pub(crate) use default_controller::{
    CancelAlgorithm, PullAlgorithm, StartAlgorithm, create_readable_stream_default_controller,
    set_up_readable_stream_default_controller,
    set_up_readable_stream_default_controller_from_underlying_source,
    with_readable_stream_default_controller_mut, with_readable_stream_default_controller_ref,
};
pub(crate) use default_reader::{
    ReadableStreamGenericReader, acquire_readable_stream_default_reader,
    construct_readable_stream_default_reader, readable_stream_default_reader_error_read_requests,
    with_readable_stream_default_reader_ref,
};
pub(crate) use stream::{
    construct_readable_stream, with_readable_stream_mut,
};
pub use default_controller::ReadableStreamDefaultController;
pub use default_reader::ReadableStreamDefaultReader;
pub use stream::ReadableStream;

/// <https://streams.spec.whatwg.org/#readablestream-state>
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadableStreamState {
    Readable,
    Closed,
    Errored,
}

/// Note: Stores an underlying-source callback together with the callback this value that
/// `SetUpReadableStreamDefaultControllerFromUnderlyingSource` passes into Web IDL callback
/// invocation.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct SourceMethod {
    /// Note: Stores the callback this value to use when the callback runs.
    this_value: JsObject,

    /// <https://webidl.spec.whatwg.org/#dfn-callback-function>
    callback: JsObject,
}

impl SourceMethod {
    /// <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
    pub(crate) fn call(&self, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let mut host = ContextCallbackHost::new(context);
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

/// Note: Adapts a bare Boa context to the Web IDL callback-invocation helpers used by the
/// readable-stream algorithms.
struct ContextCallbackHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextCallbackHost<'a> {
    /// Note: Wraps the active Boa execution context for a single callback invocation.
    fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl EcmascriptHost for ContextCallbackHost<'_> {
    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        object.get(js_string!(property), self.context)
    }

    fn is_callable(&self, object: &JsObject) -> bool {
        object.is_callable()
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message("callback is not callable"))
        })?;
        function.call(this_arg, args, self.context)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        self.context.run_jobs()
    }

    fn report_exception(&mut self, error: JsError, _callback: &JsObject) {
        eprintln!("uncaught stream callback error: {error}");
    }
}

/// Note: Models the internal request record that a default reader queues until the stream
/// produces a chunk, closes, or errors.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadRequest {
    /// Note: Stores the promise capability that resolves or rejects the JavaScript read promise.
    resolvers: ResolvingFunctions,
}

impl ReadRequest {
    /// Note: Allocates the pending read promise and stores its resolving functions.
    pub(crate) fn new(context: &mut Context) -> (Self, JsObject) {
        let (promise, resolvers) = JsPromise::new_pending(context);
        (Self { resolvers }, promise.into())
    }

    /// Note: Resolves the read request with the standard `{ value, done }` record for a chunk.
    pub(crate) fn chunk_steps(self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        let result = create_read_result(chunk, false, context)?;
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[result], context)?;
        Ok(())
    }

    /// Note: Resolves the read request with the standard `{ value, done }` record for stream
    /// closure.
    pub(crate) fn close_steps(self, context: &mut Context) -> JsResult<()> {
        let result = create_read_result(JsValue::undefined(), true, context)?;
        self.resolvers
            .resolve
            .call(&JsValue::undefined(), &[result], context)?;
        Ok(())
    }

    /// Note: Rejects the read promise with the stream error.
    pub(crate) fn error_steps(self, error: JsValue, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .reject
            .call(&JsValue::undefined(), &[error], context)?;
        Ok(())
    }
}

/// Note: Tags the concrete controller carrier stored in `ReadableStream.[[controller]]`.
#[derive(Clone, Trace, Finalize)]
pub(crate) enum ReadableStreamController {
    Default(ReadableStreamDefaultController),
}

impl ReadableStreamController {
    /// Note: Dispatches `[[CancelSteps]]` to the concrete controller carrier.
    pub(crate) fn cancel_steps(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        match self {
            Self::Default(controller) => controller.cancel_steps(reason, context),
        }
    }

    /// Note: Dispatches `[[PullSteps]]` to the concrete controller carrier.
    pub(crate) fn pull_steps(&self, read_request: ReadRequest, context: &mut Context) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.pull_steps(read_request, context),
        }
    }

    /// Note: Dispatches `[[ReleaseSteps]]` to the concrete controller carrier.
    pub(crate) fn release_steps(&self) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.release_steps(),
        }
    }

    /// Note: Downcasts the controller slot to the default-controller carrier used by the current
    /// readable-stream implementation.
    pub(crate) fn as_default_controller(&self) -> ReadableStreamDefaultController {
        match self {
            Self::Default(controller) => controller.clone(),
        }
    }
}

/// Note: Tags the concrete reader carrier stored in `ReadableStream.[[reader]]`.
#[derive(Clone, Trace, Finalize)]
pub(crate) enum ReadableStreamReader {
    Default(ReadableStreamDefaultReader),
}

impl ReadableStreamReader {
    /// Note: Reports whether the current reader slot stores a default reader.
    pub(crate) fn is_default_reader(&self) -> bool {
        matches!(self, Self::Default(_))
    }

    /// Note: Downcasts the reader slot to the default-reader carrier used by the current
    /// readable-stream implementation.
    pub(crate) fn as_default_reader(&self) -> Option<ReadableStreamDefaultReader> {
        match self {
            Self::Default(reader) => Some(reader.clone()),
        }
    }
}

/// Note: Creates the `{ value, done }` object shape that Streams read requests resolve with.
fn create_read_result(value: JsValue, done: bool, context: &mut Context) -> JsResult<JsValue> {
    let mut initializer = ObjectInitializer::new(context);
    initializer.property(js_string!("value"), value, Attribute::all());
    initializer.property(js_string!("done"), done, Attribute::all());
    Ok(JsValue::from(initializer.build()))
}

/// <https://webidl.spec.whatwg.org/#a-promise-resolved-with>
pub(crate) fn resolved_promise(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    Ok(JsPromise::resolve(value, context)?.into())
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
pub(crate) fn rejected_promise(reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
    Ok(JsPromise::reject(JsError::from_opaque(reason), context)?.into())
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
pub(crate) fn promise_from_value(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    Ok(JsPromise::resolve(value, context)?.into())
}

/// Note: Creates the rejected promise shape that readable-stream methods use for TypeError paths.
pub(crate) fn rejected_type_error_promise(
    message: &'static str,
    context: &mut Context,
) -> JsResult<JsObject> {
    rejected_promise(type_error_value(message, context)?, context)
}

/// <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
pub(crate) fn transform_promise_to_undefined(
    promise_object: &JsObject,
    context: &mut Context,
) -> JsResult<JsObject> {
    let on_fulfilled = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    Ok(JsPromise::from_object(promise_object.clone())?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

/// Note: Marks a promise handled by attaching an inert rejection reaction through Boa's native
/// promise hooks.
pub(crate) fn mark_promise_as_handled(
    promise_object: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let on_rejected = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    let _ = JsPromise::from_object(promise_object.clone())?.catch(on_rejected, context)?;
    Ok(())
}

/// Note: Returns JavaScript `undefined` for inert fulfillment and rejection handlers.
fn return_undefined(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

/// Note: Materializes a JavaScript `TypeError` value for later promise rejection.
pub(crate) fn type_error_value(message: &'static str, context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(
        JsNativeError::typ().with_message(message).into_opaque(context),
    ))
}

/// Note: Materializes a JavaScript `RangeError` value for later promise rejection.
pub(crate) fn range_error_value(message: &'static str, context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(
        JsNativeError::range().with_message(message).into_opaque(context),
    ))
}