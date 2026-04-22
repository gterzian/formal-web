use std::{cell::Cell, rc::Rc};

use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    class::Class,
    job::PromiseJob,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::{promise_from_value, rejected_promise, resolved_promise};

use super::{
    ReadableStream, WritableStream, type_error_value,
};
use super::readablestream::create_readable_stream;
use super::readablestreamdefaultcontroller::{
    CancelAlgorithm, PullAlgorithm, StartAlgorithm as ReadableStartAlgorithm,
};
use super::readablestreamsupport::SourceMethod;
use super::writablestream::create_writable_stream;
use super::writablestreamdefaultcontroller::{
    AbortAlgorithm, CloseAlgorithm, StartAlgorithm as WritableStartAlgorithm, WriteAlgorithm,
    writable_stream_default_controller_error_if_needed,
};

fn stream_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_STREAMS").is_some()
}

fn log_stream_debug(message: impl AsRef<str>) {
    if stream_debug_enabled() {
        eprintln!("[stream-debug][transform] {}", message.as_ref());
    }
}

fn queued_resolved_promise(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    let (promise, resolvers) = JsPromise::new_pending(context);
    let realm = context.realm().clone();
    context.enqueue_job(
        PromiseJob::with_realm(
            move |context| {
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[value], context)?;
                Ok(JsValue::undefined())
            },
            realm,
        )
        .into(),
    );
    Ok(promise.into())
}

/// <https://streams.spec.whatwg.org/#ts-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct TransformStream {
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#transformstream-backpressure>
    #[unsafe_ignore_trace]
    backpressure: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#transformstream-backpressurechangepromise>
    backpressure_change_promise: Gc<GcRefCell<Option<JsObject>>>,
    backpressure_change_resolvers: Gc<GcRefCell<Option<ResolvingFunctions>>>,

    /// <https://streams.spec.whatwg.org/#transformstream-controller>
    controller: Gc<GcRefCell<Option<TransformStreamDefaultController>>>,

    /// <https://streams.spec.whatwg.org/#transformstream-readable>
    readable: Gc<GcRefCell<Option<ReadableStream>>>,

    /// <https://streams.spec.whatwg.org/#transformstream-writable>
    writable: Gc<GcRefCell<Option<WritableStream>>>,
}

impl TransformStream {
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            backpressure: Rc::new(Cell::new(false)),
            backpressure_change_promise: Gc::new(GcRefCell::new(None)),
            backpressure_change_resolvers: Gc::new(GcRefCell::new(None)),
            controller: Gc::new(GcRefCell::new(None)),
            readable: Gc::new(GcRefCell::new(None)),
            writable: Gc::new(GcRefCell::new(None)),
        }
    }

    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }

    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("TransformStream is missing its JavaScript object")
                .into()
        })
    }

    pub(crate) fn readable(&self) -> JsResult<ReadableStream> {
        self.readable.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("TransformStream is missing its readable side")
                .into()
        })
    }

    pub(crate) fn writable(&self) -> JsResult<WritableStream> {
        self.writable.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("TransformStream is missing its writable side")
                .into()
        })
    }

    pub(crate) fn controller_slot(&self) -> JsResult<TransformStreamDefaultController> {
        self.controller.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("TransformStream is missing its controller")
                .into()
        })
    }

    pub(crate) fn backpressure(&self) -> bool {
        self.backpressure.get()
    }

    pub(crate) fn backpressure_change_promise(&self) -> Option<JsObject> {
        self.backpressure_change_promise.borrow().clone()
    }
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct TransformStreamDefaultController {
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-stream>
    stream: Gc<GcRefCell<Option<TransformStream>>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-transformalgorithm>
    transform_algorithm: Gc<GcRefCell<Option<TransformAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-flushalgorithm>
    flush_algorithm: Gc<GcRefCell<Option<FlushAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-cancelalgorithm>
    cancel_algorithm: Gc<GcRefCell<Option<TransformCancelAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-finishpromise>
    finish_promise: Gc<GcRefCell<Option<JsObject>>>,
    finish_resolvers: Gc<GcRefCell<Option<ResolvingFunctions>>>,
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-transformalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum TransformAlgorithm {
    Identity,
    JavaScript(SourceMethod),
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-flushalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum FlushAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-cancelalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum TransformCancelAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl TransformStreamDefaultController {
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            stream: Gc::new(GcRefCell::new(None)),
            transform_algorithm: Gc::new(GcRefCell::new(None)),
            flush_algorithm: Gc::new(GcRefCell::new(None)),
            cancel_algorithm: Gc::new(GcRefCell::new(None)),
            finish_promise: Gc::new(GcRefCell::new(None)),
            finish_resolvers: Gc::new(GcRefCell::new(None)),
        }
    }

    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }

    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("TransformStreamDefaultController is missing its JavaScript object")
                .into()
        })
    }

    fn stream_slot(&self) -> JsResult<TransformStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("TransformStreamDefaultController is not attached to a stream")
                .into()
        })
    }

    fn readable_controller(&self) -> JsResult<super::ReadableStreamDefaultController> {
        let stream = self.stream_slot()?;
        let readable = stream.readable()?;
        let controller = readable.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream is missing its controller")
        })?;
        Ok(controller.as_default_controller())
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-desired-size>
    pub(crate) fn desired_size(&self) -> JsResult<Option<f64>> {
        // Step 1: "Let readableController be this.[[stream]].[[readable]].[[controller]]."
        let readable_controller = self.readable_controller()?;

        // Step 2: "Return ! ReadableStreamDefaultControllerGetDesiredSize(readableController)."
        readable_controller.get_desired_size()
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-enqueue>
    pub(crate) fn enqueue(&self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        transform_stream_default_controller_enqueue(self.clone(), chunk, context)
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-error>
    pub(crate) fn error(&self, reason: JsValue, context: &mut Context) -> JsResult<()> {
        transform_stream_default_controller_error(self.clone(), reason, context)
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-terminate>
    pub(crate) fn terminate(&self, context: &mut Context) -> JsResult<()> {
        transform_stream_default_controller_terminate(self.clone(), context)
    }
}

// ---- Abstract operations ----

/// <https://streams.spec.whatwg.org/#initialize-transform-stream>
fn initialize_transform_stream(
    stream: &TransformStream,
    start_promise: JsObject,
    writable_high_water_mark: f64,
    writable_size_algorithm: SizeAlgorithm,
    readable_high_water_mark: f64,
    readable_size_algorithm: SizeAlgorithm,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let startAlgorithm be an algorithm that returns startPromise."
    // Note: The readable and writable setup helpers expose distinct Rust enum types for the same spec algorithm.
    let writable_start_algorithm = WritableStartAlgorithm::ReturnValue(JsValue::from(start_promise.clone()));
    let readable_start_algorithm = ReadableStartAlgorithm::ReturnValue(JsValue::from(start_promise));

    // Step 2: "Let writeAlgorithm be the following steps, taking a chunk argument:"
    let stream_for_write = stream.clone();
    let write_algorithm = WriteAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, args, stream: &TransformStream, context| {
                // Step 2.1: "Return ! TransformStreamDefaultSinkWriteAlgorithm(stream, chunk)."
                let chunk = args.get_or_undefined(0).clone();
                let promise = transform_stream_default_sink_write_algorithm(stream.clone(), chunk, context)?;
                Ok(JsValue::from(promise))
            },
            stream_for_write,
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 3: "Let abortAlgorithm be the following steps, taking a reason argument:"
    let stream_for_abort = stream.clone();
    let abort_algorithm = AbortAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, args, stream: &TransformStream, context| {
                // Step 3.1: "Return ! TransformStreamDefaultSinkAbortAlgorithm(stream, reason)."
                let reason = args.get_or_undefined(0).clone();
                let promise = transform_stream_default_sink_abort_algorithm(stream.clone(), reason, context)?;
                Ok(JsValue::from(promise))
            },
            stream_for_abort,
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 4: "Let closeAlgorithm be the following steps:"
    let stream_for_close = stream.clone();
    let close_algorithm = CloseAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, _, stream: &TransformStream, context| {
                // Step 4.1: "Return ! TransformStreamDefaultSinkCloseAlgorithm(stream)."
                let promise = transform_stream_default_sink_close_algorithm(stream.clone(), context)?;
                Ok(JsValue::from(promise))
            },
            stream_for_close,
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 5: "Set stream.[[writable]] to ! CreateWritableStream(startAlgorithm, writeAlgorithm, closeAlgorithm, abortAlgorithm, writableHighWaterMark, writableSizeAlgorithm)."
    let writable = create_writable_stream(
        writable_start_algorithm,
        write_algorithm,
        close_algorithm,
        abort_algorithm,
        Some(writable_high_water_mark),
        Some(writable_size_algorithm),
        context,
    )?;
    *stream.writable.borrow_mut() = Some(writable);

    // Step 6: "Let pullAlgorithm be the following steps:"
    let stream_for_pull = stream.clone();
    let pull_algorithm = PullAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, _, stream: &TransformStream, context| {
                // Step 6.1: "Return ! TransformStreamDefaultSourcePullAlgorithm(stream)."
                let promise = transform_stream_default_source_pull_algorithm(stream.clone(), context)?;
                Ok(JsValue::from(promise))
            },
            stream_for_pull,
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 7: "Let cancelAlgorithm be the following steps, taking a reason argument:"
    let stream_for_cancel = stream.clone();
    let cancel_algorithm = CancelAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, args, stream: &TransformStream, context| {
                // Step 7.1: "Return ! TransformStreamDefaultSourceCancelAlgorithm(stream, reason)."
                let reason = args.get_or_undefined(0).clone();
                let promise = transform_stream_default_source_cancel_algorithm(stream.clone(), reason, context)?;
                Ok(JsValue::from(promise))
            },
            stream_for_cancel,
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 8: "Set stream.[[readable]] to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancelAlgorithm, readableHighWaterMark, readableSizeAlgorithm)."
    let readable = create_readable_stream(
        readable_start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        Some(readable_high_water_mark),
        Some(readable_size_algorithm),
        context,
    )?;
    *stream.readable.borrow_mut() = Some(readable);

    // Step 9: "Set stream.[[backpressure]] and stream.[[backpressureChangePromise]] to undefined."
    // Note: The implementation initializes [[backpressure]] with a boolean field and then immediately assigns the spec-visible initial state via TransformStreamSetBackpressure.

    // Step 10: "Perform ! TransformStreamSetBackpressure(stream, true)."
    transform_stream_set_backpressure(stream, true, context)?;

    // Step 11: "Set stream.[[controller]] to undefined."
    *stream.controller.borrow_mut() = None;

    Ok(())
}

/// <https://streams.spec.whatwg.org/#transform-stream-error>
fn transform_stream_error(
    stream: &TransformStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Perform ! ReadableStreamDefaultControllerError(stream.[[readable]].[[controller]], e)."
    let readable = stream.readable()?;
    let readable_controller = readable.controller_slot().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream is missing its controller")
    })?;
    readable_controller
        .as_default_controller()
        .error_steps(error.clone(), context)?;

    // Step 2: "Perform ! TransformStreamErrorWritableAndUnblockWrite(stream, e)."
    transform_stream_error_writable_and_unblock_write(stream, error, context)
}

/// <https://streams.spec.whatwg.org/#transform-stream-error-writable-and-unblock-write>
fn transform_stream_error_writable_and_unblock_write(
    stream: &TransformStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Perform ! TransformStreamDefaultControllerClearAlgorithms(stream.[[controller]])."
    let controller = stream.controller_slot()?;
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 2: "Perform ! WritableStreamDefaultControllerErrorIfNeeded(stream.[[writable]].[[controller]], e)."
    let writable = stream.writable()?;
    let writable_controller = writable.controller_slot().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream is missing its controller")
    })?;
    writable_stream_default_controller_error_if_needed(
        writable_controller.as_default_controller(),
        error,
        context,
    )?;

    // Step 3: "Perform ! TransformStreamUnblockWrite(stream)."
    transform_stream_unblock_write(stream, context)
}

/// <https://streams.spec.whatwg.org/#transform-stream-set-backpressure>
fn transform_stream_set_backpressure(
    stream: &TransformStream,
    backpressure: bool,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Assert: stream.[[backpressure]] is not backpressure."
    // Note: On first call during initialization, backpressure is undefined (treated as not-equal).

    // Step 2: "If stream.[[backpressureChangePromise]] is not undefined, resolve stream.[[backpressureChangePromise]] with undefined."
    if let Some(resolvers) = stream.backpressure_change_resolvers.borrow_mut().take() {
        resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
    }

    // Step 3: "Set stream.[[backpressureChangePromise]] to a new promise."
    let (promise, resolvers) = JsPromise::new_pending(context);
    *stream.backpressure_change_promise.borrow_mut() = Some(promise.into());
    *stream.backpressure_change_resolvers.borrow_mut() = Some(resolvers);

    // Step 4: "Set stream.[[backpressure]] to backpressure."
    stream.backpressure.set(backpressure);

    Ok(())
}

/// <https://streams.spec.whatwg.org/#transform-stream-unblock-write>
fn transform_stream_unblock_write(
    stream: &TransformStream,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "If stream.[[backpressure]] is true, perform ! TransformStreamSetBackpressure(stream, false)."
    if stream.backpressure() {
        transform_stream_set_backpressure(stream, false, context)?;
    }

    Ok(())
}

// ---- Default controller operations ----

/// <https://streams.spec.whatwg.org/#set-up-transform-stream-default-controller>
fn set_up_transform_stream_default_controller(
    stream: &TransformStream,
    controller: TransformStreamDefaultController,
    transform_algorithm: TransformAlgorithm,
    flush_algorithm: FlushAlgorithm,
    cancel_algorithm: TransformCancelAlgorithm,
) {
    // Step 1: "Assert: stream implements TransformStream."

    // Step 2: "Assert: stream.[[controller]] is undefined."
    debug_assert!(stream.controller.borrow().is_none());

    // Step 3: "Set controller.[[stream]] to stream."
    *controller.stream.borrow_mut() = Some(stream.clone());

    // Step 4: "Set stream.[[controller]] to controller."
    *stream.controller.borrow_mut() = Some(controller.clone());

    // Step 5: "Set controller.[[transformAlgorithm]] to transformAlgorithm."
    *controller.transform_algorithm.borrow_mut() = Some(transform_algorithm);

    // Step 6: "Set controller.[[flushAlgorithm]] to flushAlgorithm."
    *controller.flush_algorithm.borrow_mut() = Some(flush_algorithm);

    // Step 7: "Set controller.[[cancelAlgorithm]] to cancelAlgorithm."
    *controller.cancel_algorithm.borrow_mut() = Some(cancel_algorithm);
}

/// <https://streams.spec.whatwg.org/#set-up-transform-stream-default-controller-from-transformer>
fn set_up_transform_stream_default_controller_from_transformer(
    stream: &TransformStream,
    transformer: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<TransformStreamDefaultController> {
    // Step 1: "Let controller be a new TransformStreamDefaultController."
    let controller = create_transform_stream_default_controller(context)?;

    // Step 2: Default transformAlgorithm is identity (enqueue the chunk).
    let mut transform_algorithm = TransformAlgorithm::Identity;

    // Step 3: Default flushAlgorithm returns resolved promise.
    let mut flush_algorithm = FlushAlgorithm::ReturnUndefined;

    // Step 4: Default cancelAlgorithm returns resolved promise.
    let mut cancel_algorithm = TransformCancelAlgorithm::ReturnUndefined;

    if let Some(transformer_obj) = transformer {
        // Step 5: "If transformerDict['transform'] exists..."
        if let Some(transform) = get_callable_method(transformer_obj, "transform", context)? {
            transform_algorithm = TransformAlgorithm::JavaScript(SourceMethod::new(
                transformer_obj.clone(),
                transform,
            ));
        }

        // Step 6: "If transformerDict['flush'] exists..."
        if let Some(flush) = get_callable_method(transformer_obj, "flush", context)? {
            flush_algorithm = FlushAlgorithm::JavaScript(SourceMethod::new(
                transformer_obj.clone(),
                flush,
            ));
        }

        // Step 7: "If transformerDict['cancel'] exists..."
        if let Some(cancel) = get_callable_method(transformer_obj, "cancel", context)? {
            cancel_algorithm = TransformCancelAlgorithm::JavaScript(SourceMethod::new(
                transformer_obj.clone(),
                cancel,
            ));
        }
    }

    // Step 8: "Perform ! SetUpTransformStreamDefaultController(stream, controller, transformAlgorithm, flushAlgorithm, cancelAlgorithm)."
    set_up_transform_stream_default_controller(
        stream,
        controller.clone(),
        transform_algorithm,
        flush_algorithm,
        cancel_algorithm,
    );

    Ok(controller)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-clear-algorithms>
fn transform_stream_default_controller_clear_algorithms(
    controller: &TransformStreamDefaultController,
) {
    // Step 1: "Set controller.[[transformAlgorithm]] to undefined."
    *controller.transform_algorithm.borrow_mut() = None;

    // Step 2: "Set controller.[[flushAlgorithm]] to undefined."
    *controller.flush_algorithm.borrow_mut() = None;

    // Step 3: "Set controller.[[cancelAlgorithm]] to undefined."
    *controller.cancel_algorithm.borrow_mut() = None;
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-enqueue>
fn transform_stream_default_controller_enqueue(
    controller: TransformStreamDefaultController,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot()?;

    // Step 2: "Let readableController be stream.[[readable]].[[controller]]."
    let readable_controller = controller.readable_controller()?;

    // Step 3: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(readableController) is false, throw a TypeError exception."
    if !readable_controller.can_close_or_enqueue()? {
        return Err(JsNativeError::typ()
            .with_message("ReadableStream is not in a state that permits enqueue")
            .into());
    }

    // Step 4: "Let enqueueResult be ReadableStreamDefaultControllerEnqueue(readableController, chunk)."
    // Step 5: "If enqueueResult is an abrupt completion..."
    if let Err(error) = readable_controller.enqueue_steps(chunk, context) {
        // Step 5.1: "Perform ! TransformStreamErrorWritableAndUnblockWrite(stream, enqueueResult.[[Value]])."
        let error_value = error.into_opaque(context)?;
        transform_stream_error_writable_and_unblock_write(&stream, error_value, context)?;

        // Step 5.2: "Throw stream.[[readable]].[[storedError]]."
        return Err(boa_engine::JsError::from_opaque(stream.readable()?.stored_error()));
    }

    // Step 6: "Let backpressure be ! ReadableStreamDefaultControllerHasBackpressure(readableController)."
    let backpressure = readable_controller.has_backpressure()?;

    // Step 7: "If backpressure is not stream.[[backpressure]],"
    if backpressure != stream.backpressure() {
        // Step 7.1: "Assert: backpressure is true."
        debug_assert!(backpressure);

        // Step 7.2: "Perform ! TransformStreamSetBackpressure(stream, true)."
        transform_stream_set_backpressure(&stream, true, context)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-error>
fn transform_stream_default_controller_error(
    controller: TransformStreamDefaultController,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Perform ! TransformStreamError(controller.[[stream]], e)."
    let stream = controller.stream_slot()?;
    transform_stream_error(&stream, reason, context)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-perform-transform>
fn transform_stream_default_controller_perform_transform(
    controller: TransformStreamDefaultController,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let transformPromise be the result of performing controller.[[transformAlgorithm]], passing chunk."
    let transform_algorithm = controller.transform_algorithm.borrow().clone();
    let transform_promise = match transform_algorithm {
        Some(TransformAlgorithm::Identity) => {
            // Step 1: "Let transformPromise be the result of performing controller.[[transformAlgorithm]], passing chunk."
            // Note: The default identity transform algorithm enqueues chunk directly.
            if let Err(error) = transform_stream_default_controller_enqueue(controller.clone(), chunk, context) {
                rejected_promise(error.into_opaque(context)?, context)?
            } else {
                resolved_promise(JsValue::undefined(), context)?
            }
        }
        Some(TransformAlgorithm::JavaScript(ref callback)) => {
            let controller_value = JsValue::from(controller.object()?);
            match callback.call(&[chunk, controller_value], context) {
                Ok(value) => promise_from_value(value, context)?,
                Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
            }
        }
        None => {
            return Err(JsNativeError::typ()
                .with_message("TransformStreamDefaultController is missing its transform algorithm")
                .into());
        }
    };

    // Step 2: "Return the result of reacting to transformPromise with the following rejection steps given the argument r:"
    let stream = controller.stream_slot()?;
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, stream: &TransformStream, context| {
            let error = args.get_or_undefined(0).clone();
            // Step 2.1: "Perform ! TransformStreamError(controller.[[stream]], r)."
            transform_stream_error(stream, error.clone(), context)?;
            // Step 2.2: "Throw r."
            Err(boa_engine::JsError::from_opaque(error))
        },
        stream,
    )
    .to_js_function(context.realm());
    let result = JsPromise::from_object(transform_promise)?.then(None, Some(on_rejected), context)?;
    Ok(result.into())
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-terminate>
fn transform_stream_default_controller_terminate(
    controller: TransformStreamDefaultController,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot()?;

    // Step 2: "Let readableController be stream.[[readable]].[[controller]]."
    let readable_controller = controller.readable_controller()?;

    // Step 3: "Perform ! ReadableStreamDefaultControllerClose(readableController)."
    readable_controller.close_steps(context)?;

    // Step 4: "Let error be a TypeError exception indicating that the stream has been terminated."
    let error = type_error_value("TransformStream has been terminated", context)?;

    let writable = stream.writable()?;
    log_stream_debug(format!(
        "terminate before error writable_state={:?}",
        writable.state()
    ));

    // Step 5: "Perform ! TransformStreamErrorWritableAndUnblockWrite(stream, error)."
    let result = transform_stream_error_writable_and_unblock_write(&stream, error, context);
    log_stream_debug(format!(
        "terminate after error writable_state={:?} stored_error={}",
        writable.state(),
        writable.stored_error().display()
    ));
    result
}

// ---- Default sink algorithms ----

/// <https://streams.spec.whatwg.org/#transform-stream-default-sink-write-algorithm>
fn transform_stream_default_sink_write_algorithm(
    stream: TransformStream,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Assert: stream.[[writable]].[[state]] is \"writable\"."

    // Step 2: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot()?;

    // Step 3: "If stream.[[backpressure]] is true,"
    if stream.backpressure() {
        // Step 3.1: "Let backpressureChangePromise be stream.[[backpressureChangePromise]]."
        let backpressure_change_promise = stream.backpressure_change_promise().ok_or_else(|| {
            JsNativeError::typ().with_message("TransformStream is missing its backpressure change promise")
        })?;

        // Step 3.2: "Assert: backpressureChangePromise is not undefined."

        // Step 3.3: "Return the result of reacting to backpressureChangePromise with the following fulfillment steps:"
        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, captures: &(TransformStream, TransformStreamDefaultController, JsValue), context| {
                let (stream, controller, chunk) = captures;

                // Step 3.3.1: "Let writable be stream.[[writable]]."
                let writable = stream.writable()?;

                // Step 3.3.2: "Let state be writable.[[state]]."
                // Step 3.3.3: "If state is \"erroring\", throw writable.[[storedError]]."
                if writable.state() == super::WritableStreamState::Erroring {
                    return Err(boa_engine::JsError::from_opaque(writable.stored_error()));
                }

                // Step 3.3.4: "Assert: state is \"writable\"."
                debug_assert_eq!(writable.state(), super::WritableStreamState::Writable);

                // Step 3.3.5: "Return ! TransformStreamDefaultControllerPerformTransform(controller, chunk)."
                let promise = transform_stream_default_controller_perform_transform(
                    controller.clone(),
                    chunk.clone(),
                    context,
                )?;
                Ok(JsValue::from(promise))
            },
            (stream, controller, chunk),
        )
        .to_js_function(context.realm());

        let result = JsPromise::from_object(backpressure_change_promise)?
            .then(Some(on_fulfilled), None, context)?;
        return Ok(result.into());
    }

    // Step 4: "Return ! TransformStreamDefaultControllerPerformTransform(controller, chunk)."
    transform_stream_default_controller_perform_transform(controller, chunk, context)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-sink-abort-algorithm>
fn transform_stream_default_sink_abort_algorithm(
    stream: TransformStream,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot()?;

    // Step 2: "If controller.[[finishPromise]] is not undefined, return controller.[[finishPromise]]."
    if let Some(finish_promise) = controller.finish_promise.borrow().clone() {
        return Ok(finish_promise);
    }

    // Step 3: "Let readable be stream.[[readable]]."
    let readable = stream.readable()?;

    // Step 4: "Let controller.[[finishPromise]] be a new promise."
    let (finish_promise, finish_resolvers) = JsPromise::new_pending(context);
    let finish_promise_obj: JsObject = finish_promise.into();
    *controller.finish_promise.borrow_mut() = Some(finish_promise_obj.clone());
    *controller.finish_resolvers.borrow_mut() = Some(finish_resolvers);

    // Step 5: "Let cancelPromise be the result of performing controller.[[cancelAlgorithm]], passing reason."
    let cancel_algorithm = controller.cancel_algorithm.borrow().clone();
    let cancel_promise = match cancel_algorithm {
        Some(TransformCancelAlgorithm::ReturnUndefined) => {
            queued_resolved_promise(JsValue::undefined(), context)?
        }
        Some(TransformCancelAlgorithm::JavaScript(ref callback)) => {
            match callback.call(&[reason.clone()], context) {
                Ok(value) => promise_from_value(value, context)?,
                Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
            }
        }
        None => queued_resolved_promise(JsValue::undefined(), context)?,
    };

    // Step 6: "Perform ! TransformStreamDefaultControllerClearAlgorithms(controller)."
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 7: React to cancelPromise.
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, captures: &(TransformStreamDefaultController, ReadableStream, JsValue), context| {
            let (controller, readable, reason) = captures;
            let readable_state = readable.state();

            if readable_state == super::ReadableStreamState::Errored {
                // Step 7.1.1: Reject finishPromise with readable.[[storedError]].
                if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                    resolvers.reject.call(&JsValue::undefined(), &[readable.stored_error()], context)?;
                }
            } else {
                // Step 7.1.2.1: "Perform ! ReadableStreamDefaultControllerError(readable.[[controller]], reason)."
                let readable_controller = readable.controller_slot().ok_or_else(|| {
                    JsNativeError::typ().with_message("ReadableStream is missing its controller")
                })?;
                readable_controller
                    .as_default_controller()
                    .error_steps(reason.clone(), context)?;

                // Step 7.1.2.2: Resolve finishPromise.
                if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                    resolvers.resolve.call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
                }
            }

            Ok(JsValue::undefined())
        },
        (controller.clone(), readable.clone(), reason),
    )
    .to_js_function(context.realm());

    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, captures: &(TransformStreamDefaultController, ReadableStream), context| {
            let (controller, readable) = captures;
            let error = args.get_or_undefined(0).clone();

            // Step 7.2.1: "Perform ! ReadableStreamDefaultControllerError(readable.[[controller]], r)."
            let readable_controller = readable.controller_slot().ok_or_else(|| {
                JsNativeError::typ().with_message("ReadableStream is missing its controller")
            })?;
            readable_controller
                .as_default_controller()
                .error_steps(error.clone(), context)?;

            // Step 7.2.2: Reject finishPromise with r.
            if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                resolvers.reject.call(&JsValue::undefined(), &[error], context)?;
            }

            Ok(JsValue::undefined())
        },
        (controller, readable),
    )
    .to_js_function(context.realm());

    let _ = JsPromise::from_object(cancel_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;

    // Step 8: "Return controller.[[finishPromise]]."
    Ok(finish_promise_obj)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-sink-close-algorithm>
fn transform_stream_default_sink_close_algorithm(
    stream: TransformStream,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot()?;

    // Step 2: "If controller.[[finishPromise]] is not undefined, return controller.[[finishPromise]]."
    if let Some(finish_promise) = controller.finish_promise.borrow().clone() {
        return Ok(finish_promise);
    }

    // Step 3: "Let readable be stream.[[readable]]."
    let readable = stream.readable()?;

    // Step 4: "Let controller.[[finishPromise]] be a new promise."
    let (finish_promise, finish_resolvers) = JsPromise::new_pending(context);
    let finish_promise_obj: JsObject = finish_promise.into();
    *controller.finish_promise.borrow_mut() = Some(finish_promise_obj.clone());
    *controller.finish_resolvers.borrow_mut() = Some(finish_resolvers);

    // Step 5: "Let flushPromise be the result of performing controller.[[flushAlgorithm]]."
    let flush_algorithm = controller.flush_algorithm.borrow().clone();
    let flush_promise = match flush_algorithm {
        Some(FlushAlgorithm::ReturnUndefined) => {
            queued_resolved_promise(JsValue::undefined(), context)?
        }
        Some(FlushAlgorithm::JavaScript(ref callback)) => {
            let controller_value = JsValue::from(controller.object()?);
            match callback.call(&[controller_value], context) {
                Ok(value) => promise_from_value(value, context)?,
                Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
            }
        }
        None => queued_resolved_promise(JsValue::undefined(), context)?,
    };

    // Step 6: "Perform ! TransformStreamDefaultControllerClearAlgorithms(controller)."
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 7: React to flushPromise.
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, captures: &(TransformStreamDefaultController, ReadableStream), context| {
            let (controller, readable) = captures;
            let readable_state = readable.state();

            if readable_state == super::ReadableStreamState::Errored {
                // Step 7.1.1: Reject finishPromise with readable.[[storedError]].
                if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                    resolvers.reject.call(&JsValue::undefined(), &[readable.stored_error()], context)?;
                }
            } else {
                // Step 7.1.2.1: "Perform ! ReadableStreamDefaultControllerClose(readable.[[controller]])."
                let readable_controller = readable.controller_slot().ok_or_else(|| {
                    JsNativeError::typ().with_message("ReadableStream is missing its controller")
                })?;
                readable_controller.as_default_controller().close_steps(context)?;

                // Step 7.1.2.2: Resolve finishPromise.
                if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                    resolvers.resolve.call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
                }
            }

            Ok(JsValue::undefined())
        },
        (controller.clone(), readable.clone()),
    )
    .to_js_function(context.realm());

    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, captures: &(TransformStreamDefaultController, ReadableStream), context| {
            let (controller, readable) = captures;
            let error = args.get_or_undefined(0).clone();

            // Step 7.2.1: "Perform ! ReadableStreamDefaultControllerError(readable.[[controller]], r)."
            let readable_controller = readable.controller_slot().ok_or_else(|| {
                JsNativeError::typ().with_message("ReadableStream is missing its controller")
            })?;
            readable_controller
                .as_default_controller()
                .error_steps(error.clone(), context)?;

            // Step 7.2.2: Reject finishPromise with r.
            if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                resolvers.reject.call(&JsValue::undefined(), &[error], context)?;
            }

            Ok(JsValue::undefined())
        },
        (controller, readable),
    )
    .to_js_function(context.realm());

    let _ = JsPromise::from_object(flush_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;

    // Step 8: "Return controller.[[finishPromise]]."
    Ok(finish_promise_obj)
}

// ---- Default source algorithms ----

/// <https://streams.spec.whatwg.org/#transform-stream-default-source-pull-algorithm>
fn transform_stream_default_source_pull_algorithm(
    stream: TransformStream,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Assert: stream.[[backpressure]] is true."
    debug_assert!(stream.backpressure());

    // Step 2: "Assert: stream.[[backpressureChangePromise]] is not undefined."
    debug_assert!(stream.backpressure_change_promise().is_some());

    // Step 3: "Perform ! TransformStreamSetBackpressure(stream, false)."
    transform_stream_set_backpressure(&stream, false, context)?;

    // Step 4: "Return stream.[[backpressureChangePromise]]."
    stream.backpressure_change_promise().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("TransformStream is missing its backpressure change promise")
            .into()
    })
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-source-cancel-algorithm>
fn transform_stream_default_source_cancel_algorithm(
    stream: TransformStream,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot()?;

    // Step 2: "If controller.[[finishPromise]] is not undefined, return controller.[[finishPromise]]."
    if let Some(finish_promise) = controller.finish_promise.borrow().clone() {
        return Ok(finish_promise);
    }

    // Step 3: "Let writable be stream.[[writable]]."
    let writable = stream.writable()?;

    // Step 4: "Let controller.[[finishPromise]] be a new promise."
    let (finish_promise, finish_resolvers) = JsPromise::new_pending(context);
    let finish_promise_obj: JsObject = finish_promise.into();
    *controller.finish_promise.borrow_mut() = Some(finish_promise_obj.clone());
    *controller.finish_resolvers.borrow_mut() = Some(finish_resolvers);

    // Step 5: "Let cancelPromise be the result of performing controller.[[cancelAlgorithm]], passing reason."
    let cancel_algorithm = controller.cancel_algorithm.borrow().clone();
    let cancel_promise = match cancel_algorithm {
        Some(TransformCancelAlgorithm::ReturnUndefined) => {
            queued_resolved_promise(JsValue::undefined(), context)?
        }
        Some(TransformCancelAlgorithm::JavaScript(ref callback)) => {
            match callback.call(&[reason.clone()], context) {
                Ok(value) => promise_from_value(value, context)?,
                Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
            }
        }
        None => queued_resolved_promise(JsValue::undefined(), context)?,
    };

    // Step 6: "Perform ! TransformStreamDefaultControllerClearAlgorithms(controller)."
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 7: React to cancelPromise.
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, captures: &(TransformStreamDefaultController, TransformStream, WritableStream, JsValue), context| {
            let (controller, stream, writable, reason) = captures;
            let writable_state = writable.state();
            log_stream_debug(format!(
                "source cancel fulfilled writable_state={:?} stored_error={}",
                writable_state,
                writable.stored_error().display()
            ));

            // Preserve any existing writable-side failure, but keep readable cancel fulfilled.
            let writable_controller = writable.controller_slot().ok_or_else(|| {
                JsNativeError::typ().with_message("WritableStream is missing its controller")
            })?;
            writable_stream_default_controller_error_if_needed(
                writable_controller.as_default_controller(),
                reason.clone(),
                context,
            )?;

            // Step 7.1.2.2: "Perform ! TransformStreamUnblockWrite(stream)."
            transform_stream_unblock_write(stream, context)?;

            // Step 7.1.2.3: Resolve finishPromise.
            if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                resolvers.resolve.call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
            }

            Ok(JsValue::undefined())
        },
        (controller.clone(), stream.clone(), writable.clone(), reason),
    )
    .to_js_function(context.realm());

    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, captures: &(TransformStreamDefaultController, TransformStream, WritableStream), context| {
            let (controller, stream, writable) = captures;
            let error = args.get_or_undefined(0).clone();

            // Step 7.2.1: "Perform ! WritableStreamDefaultControllerErrorIfNeeded(writable.[[controller]], r)."
            let writable_controller = writable.controller_slot().ok_or_else(|| {
                JsNativeError::typ().with_message("WritableStream is missing its controller")
            })?;
            writable_stream_default_controller_error_if_needed(
                writable_controller.as_default_controller(),
                error.clone(),
                context,
            )?;

            // Step 7.2.2: "Perform ! TransformStreamUnblockWrite(stream)."
            transform_stream_unblock_write(stream, context)?;

            // Step 7.2.3: Reject finishPromise with r.
            if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
                resolvers.reject.call(&JsValue::undefined(), &[error], context)?;
            }

            Ok(JsValue::undefined())
        },
        (controller, stream, writable),
    )
    .to_js_function(context.realm());

    let _ = JsPromise::from_object(cancel_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;

    // Step 8: "Return controller.[[finishPromise]]."
    Ok(finish_promise_obj)
}

// ---- Constructor helpers ----

fn create_transform_stream_default_controller(
    context: &mut Context,
) -> JsResult<TransformStreamDefaultController> {
    let controller = TransformStreamDefaultController::new(None);
    let controller_object =
        TransformStreamDefaultController::from_data(controller.clone(), context)?;
    controller.set_reflector(controller_object);
    Ok(controller)
}

/// <https://streams.spec.whatwg.org/#ts-constructor>
pub(crate) fn construct_transform_stream(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<TransformStream> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("TransformStream receiver is not an object")
    })?;
    let stream = TransformStream::new(Some(stream_object.clone()));

    // Step 1: "If transformer is missing, set it to null."
    let transformer = if args.is_empty() {
        JsValue::null()
    } else {
        args[0].clone()
    };

    let transformer_object = if transformer.is_null() || transformer.is_undefined() {
        None
    } else {
        Some(transformer.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("TransformStream transformer must be an object")
        })?)
    };

    // Step 2: "Let transformerDict be transformer, converted to an IDL value of type Transformer."
    // Note: The runtime retains the original transformer object so it can invoke the transformer callbacks with the original callback this value.

    // Step 3: "If transformerDict[\"readableType\"] exists, throw a RangeError exception."
    if let Some(ref obj) = transformer_object {
        if obj.has_property(js_string!("readableType"), context)? {
            return Err(JsNativeError::range()
                .with_message("TransformStream transformer.readableType is not supported")
                .into());
        }

        // Step 4: "If transformerDict[\"writableType\"] exists, throw a RangeError exception."
        if obj.has_property(js_string!("writableType"), context)? {
            return Err(JsNativeError::range()
                .with_message("TransformStream transformer.writableType is not supported")
                .into());
        }
    }

    // Step 5: "Let readableHighWaterMark be ? ExtractHighWaterMark(readableStrategy, 0)."
    let readable_strategy = args.get(2).cloned().unwrap_or(JsValue::undefined());
    let readable_high_water_mark = extract_high_water_mark(&readable_strategy, 0.0, context)?;

    // Step 6: "Let readableSizeAlgorithm be ! ExtractSizeAlgorithm(readableStrategy)."
    let readable_size_algorithm = extract_size_algorithm(&readable_strategy, context)?;

    // Step 7: "Let writableHighWaterMark be ? ExtractHighWaterMark(writableStrategy, 1)."
    let writable_strategy = args.get(1).cloned().unwrap_or(JsValue::undefined());
    let writable_high_water_mark = extract_high_water_mark(&writable_strategy, 1.0, context)?;

    // Step 8: "Let writableSizeAlgorithm be ! ExtractSizeAlgorithm(writableStrategy)."
    let writable_size_algorithm = extract_size_algorithm(&writable_strategy, context)?;

    // Step 9: "Let startPromise be a new promise."
    let (start_promise, start_resolvers) = JsPromise::new_pending(context);

    // Step 10: "Perform ! InitializeTransformStream(this, startPromise, ...)."
    initialize_transform_stream(
        &stream,
        start_promise.into(),
        writable_high_water_mark,
        writable_size_algorithm,
        readable_high_water_mark,
        readable_size_algorithm,
        context,
    )?;

    // Step 11: "Perform ? SetUpTransformStreamDefaultControllerFromTransformer(this, transformer, transformerDict)."
    let controller = set_up_transform_stream_default_controller_from_transformer(
        &stream,
        transformer_object.as_ref(),
        context,
    )?;

    // Step 12: "If transformerDict[\"start\"] exists, then resolve startPromise with the result of invoking transformerDict[\"start\"] with argument list « this.[[controller]] » and callback this value transformer."
    if let Some(ref transformer_obj) = transformer_object {
        if let Some(start) = get_callable_method(transformer_obj, "start", context)? {
            let controller_value = JsValue::from(controller.object()?);
            let source_method = SourceMethod::new(transformer_obj.clone(), start);
            let result = source_method.call(&[controller_value], context)?;
            start_resolvers
                .resolve
                .call(&JsValue::undefined(), &[result], context)?;
        } else {
            // Step 13: "Otherwise, resolve startPromise with undefined."
            start_resolvers
                .resolve
                .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
        }
    } else {
        // Step 13: "Otherwise, resolve startPromise with undefined."
        start_resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
    }

    Ok(stream)
}

pub(crate) fn with_transform_stream_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&TransformStream) -> R,
) -> JsResult<R> {
    let stream = object
        .downcast_ref::<TransformStream>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not a TransformStream"))?;
    Ok(f(&stream))
}

pub(crate) fn with_transform_stream_default_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&TransformStreamDefaultController) -> R,
) -> JsResult<R> {
    let controller = object
        .downcast_ref::<TransformStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("object is not a TransformStreamDefaultController")
        })?;
    Ok(f(&controller))
}

fn get_callable_method(
    object: &JsObject,
    property: &'static str,
    context: &mut Context,
) -> JsResult<Option<JsObject>> {
    let value = object.get(js_string!(property), context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    let method = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message(format!(
            "TransformStream transformer.{property} must be callable when provided"
        ))
    })?;
    if !method.is_callable() {
        return Err(JsNativeError::typ()
            .with_message(format!(
                "TransformStream transformer.{property} must be callable when provided"
            ))
            .into());
    }

    Ok(Some(method.clone()))
}
