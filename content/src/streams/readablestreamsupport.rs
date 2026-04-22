use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    job::PromiseJob,
    js_string,
    object::{
        JsObject, ObjectInitializer,
        builtins::{JsFunction, JsPromise},
    },
    property::Attribute,
};
use boa_gc::{Finalize, Trace};

use crate::webidl::{EcmascriptHost, ExceptionBehavior, invoke_callback_function};

use super::readablestreamdefaultcontroller::ReadableStreamDefaultController;
use super::readablestreamdefaultreader::ReadableStreamDefaultReader;

/// <https://streams.spec.whatwg.org/#readablestream-state>
#[derive(Clone, Debug, Eq, PartialEq, Trace, Finalize)]
pub(crate) enum ReadableStreamState {
    Readable,
    Closed,
    Errored,
}
/// `SetUpReadableStreamDefaultControllerFromUnderlyingSource` passes into Web IDL callback
/// invocation.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct SourceMethod {
    this_value: JsObject,

    /// <https://webidl.spec.whatwg.org/#dfn-callback-function>
    callback: JsObject,
}

impl SourceMethod {
    /// Web IDL invocation.
    pub(crate) fn new(this_value: JsObject, callback: JsObject) -> Self {
        Self {
            this_value,
            callback,
        }
    }

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
/// readable-stream algorithms.
struct ContextCallbackHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextCallbackHost<'a> {
    fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl EcmascriptHost for ContextCallbackHost<'_> {
    fn context(&mut self) -> &mut Context {
        self.context
    }

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
/// produces a chunk, closes, or errors.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadRequest {
    sink: ReadRequestSink,
}

#[derive(Clone, Trace, Finalize)]
enum ReadRequestSink {
    Promise {
        resolvers: ResolvingFunctions,
    },
    Reaction {
        on_fulfilled: JsFunction,
        on_rejected: JsFunction,
    },
}

impl ReadRequest {
    pub(crate) fn new(context: &mut Context) -> (Self, JsObject) {
        let (promise, resolvers) = JsPromise::new_pending(context);
        (
            Self {
                sink: ReadRequestSink::Promise { resolvers },
            },
            promise.into(),
        )
    }

    pub(crate) fn new_reaction(on_fulfilled: JsFunction, on_rejected: JsFunction) -> Self {
        Self {
            sink: ReadRequestSink::Reaction {
                on_fulfilled,
                on_rejected,
            },
        }
    }

    pub(crate) fn chunk_steps(self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        let result = create_read_result(chunk, false, context)?;
        match &self.sink {
            ReadRequestSink::Promise { resolvers } => {
                let resolvers = resolvers.clone();
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[result], context)?;
                Ok(())
            }
            ReadRequestSink::Reaction { on_fulfilled, .. } => {
                queue_read_request_reaction(on_fulfilled.clone(), result, context)
            }
        }
    }

    /// closure.
    pub(crate) fn close_steps(self, context: &mut Context) -> JsResult<()> {
        let result = create_read_result(JsValue::undefined(), true, context)?;
        match &self.sink {
            ReadRequestSink::Promise { resolvers } => {
                let resolvers = resolvers.clone();
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[result], context)?;
                Ok(())
            }
            ReadRequestSink::Reaction { on_fulfilled, .. } => {
                queue_read_request_reaction(on_fulfilled.clone(), result, context)
            }
        }
    }

    pub(crate) fn error_steps(self, error: JsValue, context: &mut Context) -> JsResult<()> {
        match &self.sink {
            ReadRequestSink::Promise { resolvers } => {
                let resolvers = resolvers.clone();
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[error], context)?;
                Ok(())
            }
            ReadRequestSink::Reaction { on_rejected, .. } => {
                queue_read_request_reaction(on_rejected.clone(), error, context)
            }
        }
    }
}

fn queue_read_request_reaction(
    callback: JsFunction,
    value: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    let realm = context.realm().clone();
    context.enqueue_job(
        PromiseJob::with_realm(
            move |context| {
                callback.call(&JsValue::undefined(), &[value], context)?;
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
}

impl ReadableStreamController {
    pub(crate) fn cancel_steps(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        match self {
            Self::Default(controller) => controller.cancel_steps(reason, context),
        }
    }
    pub(crate) fn pull_steps(&self, read_request: ReadRequest, context: &mut Context) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.pull_steps(read_request, context),
        }
    }
    pub(crate) fn release_steps(&self) -> JsResult<()> {
        match self {
            Self::Default(controller) => controller.release_steps(),
        }
    }
    /// readable-stream implementation.
    pub(crate) fn as_default_controller(&self) -> ReadableStreamDefaultController {
        match self {
            Self::Default(controller) => controller.clone(),
        }
    }
}
#[derive(Clone, Trace, Finalize)]
pub(crate) enum ReadableStreamReader {
    Default(ReadableStreamDefaultReader),
}

impl ReadableStreamReader {
    pub(crate) fn is_default_reader(&self) -> bool {
        matches!(self, Self::Default(_))
    }
    /// readable-stream implementation.
    pub(crate) fn as_default_reader(&self) -> Option<ReadableStreamDefaultReader> {
        match self {
            Self::Default(reader) => Some(reader.clone()),
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
        JsNativeError::typ().with_message(message).into_opaque(context),
    ))
}
pub(crate) fn range_error_value(message: &'static str, context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(
        JsNativeError::range().with_message(message).into_opaque(context),
    ))
}