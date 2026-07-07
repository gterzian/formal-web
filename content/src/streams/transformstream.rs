use log::debug;
use std::{cell::Cell, rc::Rc};

use js_engine::{Completion, ExecutionContext, JsTypes, PromiseResolvers};

use crate::js::Types;

use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{promise_from_value, rejected_promise, resolved_promise};

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
use super::{ReadableStream, WritableStream, type_error_value};
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

fn stream_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_STREAMS").is_some()
}

fn log_stream_debug(message: impl AsRef<str>) {
    if stream_debug_enabled() {
        debug!("[stream-debug][transform] {}", message.as_ref());
    }
}

/// <https://streams.spec.whatwg.org/#ts-class>
#[gc_struct]
pub struct TransformStream {
    /// <https://streams.spec.whatwg.org/#transformstream-backpressure>
    #[ignore_trace]
    backpressure: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#transformstream-backpressurechangepromise>
    backpressure_change_promise: GcCell<Option<JsObject>>,
    backpressure_change_resolvers: GcCell<Option<PromiseResolvers<Types>>>,

    /// <https://streams.spec.whatwg.org/#transformstream-controller>
    controller: GcCell<Option<TransformStreamDefaultController>>,
    controller_object: GcCell<Option<JsObject>>,

    /// <https://streams.spec.whatwg.org/#transformstream-readable>
    readable: GcCell<Option<ReadableStream>>,
    readable_object: GcCell<Option<JsObject>>,

    /// <https://streams.spec.whatwg.org/#transformstream-writable>
    writable: GcCell<Option<WritableStream>>,
    writable_object: GcCell<Option<JsObject>>,
}

impl TransformStream {
    pub(crate) fn new() -> Self {
        Self {
            backpressure: Rc::new(Cell::new(false)),
            backpressure_change_promise: gc_cell_new(None),
            backpressure_change_resolvers: gc_cell_new(None),
            controller: gc_cell_new(None),
            controller_object: gc_cell_new(None),
            readable: gc_cell_new(None),
            readable_object: gc_cell_new(None),
            writable: gc_cell_new(None),
            writable_object: gc_cell_new(None),
        }
    }

    pub(crate) fn readable(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<ReadableStream, Types> {
        self.readable
            .borrow()
            .clone()
            .ok_or_else(|| ec.new_type_error("TransformStream is missing its readable side"))
    }

    pub(crate) fn readable_object(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        self.readable_object.borrow().clone().ok_or_else(|| {
            ec.new_type_error("TransformStream is missing its readable JavaScript object")
        })
    }

    pub(crate) fn writable(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<WritableStream, Types> {
        self.writable
            .borrow()
            .clone()
            .ok_or_else(|| ec.new_type_error("TransformStream is missing its writable side"))
    }

    pub(crate) fn writable_object(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        self.writable_object.borrow().clone().ok_or_else(|| {
            ec.new_type_error("TransformStream is missing its writable JavaScript object")
        })
    }

    pub(crate) fn controller_slot(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<TransformStreamDefaultController, Types> {
        self.controller
            .borrow()
            .clone()
            .ok_or_else(|| ec.new_type_error("TransformStream is missing its controller"))
    }

    pub(crate) fn controller_object(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        self.controller_object.borrow().clone().ok_or_else(|| {
            ec.new_type_error("TransformStream is missing its controller JavaScript object")
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
#[gc_struct]
pub struct TransformStreamDefaultController {
    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-stream>
    stream: GcCell<Option<TransformStream>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-transformalgorithm>
    transform_algorithm: GcCell<Option<TransformAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-flushalgorithm>
    flush_algorithm: GcCell<Option<FlushAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-cancelalgorithm>
    cancel_algorithm: GcCell<Option<TransformCancelAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-finishpromise>
    finish_promise: GcCell<Option<JsObject>>,
    finish_resolvers: GcCell<Option<PromiseResolvers<Types>>>,
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-transformalgorithm>
#[gc_struct]
pub(crate) enum TransformAlgorithm {
    Identity,
    JavaScript(SourceMethod),
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-flushalgorithm>
#[gc_struct]
pub(crate) enum FlushAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

/// <https://streams.spec.whatwg.org/#transformstreamdefaultcontroller-cancelalgorithm>
#[gc_struct]
pub(crate) enum TransformCancelAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl TransformStreamDefaultController {
    pub(crate) fn new() -> Self {
        Self {
            stream: gc_cell_new(None),
            transform_algorithm: gc_cell_new(None),
            flush_algorithm: gc_cell_new(None),
            cancel_algorithm: gc_cell_new(None),
            finish_promise: gc_cell_new(None),
            finish_resolvers: gc_cell_new(None),
        }
    }

    fn stream_slot(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<TransformStream, Types> {
        self.stream.borrow().clone().ok_or_else(|| {
            ec.new_type_error("TransformStreamDefaultController is not attached to a stream")
        })
    }

    fn controller_object(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        self.stream_slot(ec)?.controller_object(ec)
    }

    fn readable_controller(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<super::ReadableStreamDefaultController, Types> {
        let stream = self.stream_slot(ec)?;
        let readable = stream.readable(ec)?;
        let controller = readable
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
        Ok(controller.as_default_controller())
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-desired-size>
    pub(crate) fn desired_size(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<f64>, Types> {
        // Step 1: "Let readableController be this.[[stream]].[[readable]].[[controller]]."
        let readable_controller = self.readable_controller(ec)?;

        // Step 2: "Return ! ReadableStreamDefaultControllerGetDesiredSize(readableController)."
        readable_controller.get_desired_size(ec)
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-enqueue>
    pub(crate) fn enqueue(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        transform_stream_default_controller_enqueue(self.clone(), chunk, ec)
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-error>
    pub(crate) fn error(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        transform_stream_default_controller_error(self.clone(), reason, ec)
    }

    /// <https://streams.spec.whatwg.org/#ts-default-controller-terminate>
    pub(crate) fn terminate(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        transform_stream_default_controller_terminate(self.clone(), ec)
    }
}

// ---- Abstract operations ----

fn sink_write_algorithm_fn(
    args: &[JsValue],
    _this: JsValue,
    stream: &TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 2.1: "Return ! TransformStreamDefaultSinkWriteAlgorithm(stream, chunk)."
    let chunk = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    let promise = transform_stream_default_sink_write_algorithm(stream.clone(), chunk, ec)?;
    Ok(JsValue::from(promise))
}

fn sink_abort_algorithm_fn(
    args: &[JsValue],
    _this: JsValue,
    stream: &TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 3.1: "Return ! TransformStreamDefaultSinkAbortAlgorithm(stream, reason)."
    let reason = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    let promise = transform_stream_default_sink_abort_algorithm(stream.clone(), reason, ec)?;
    Ok(JsValue::from(promise))
}

fn sink_close_algorithm_fn(
    _args: &[JsValue],
    _this: JsValue,
    stream: &TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 4.1: "Return ! TransformStreamDefaultSinkCloseAlgorithm(stream)."
    let promise = transform_stream_default_sink_close_algorithm(stream.clone(), ec)?;
    Ok(JsValue::from(promise))
}

fn perform_transform_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    stream: &TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let error = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    // Step 2.1: "Perform ! TransformStreamError(controller.[[stream]], r)."
    transform_stream_error(stream, error.clone(), ec)?;
    // Step 2.2: "Throw r."
    Err(error)
}

fn controller_enqueue_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(TransformStream, TransformStreamDefaultController, JsValue),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (stream, controller, chunk) = captures;
    // Step 3.3.1: "Let writable be stream.[[writable]]."
    let writable = stream.writable(ec)?;
    // Step 3.3.2: "Let state be writable.[[state]]."
    // Step 3.3.3: "If state is \"erroring\", throw writable.[[storedError]]."
    if writable.state() == super::WritableStreamState::Erroring {
        return Err(writable.stored_error());
    }
    // Step 3.3.4: "Assert: state is \"writable\"."
    debug_assert_eq!(writable.state(), super::WritableStreamState::Writable);
    // Step 3.3.5: "Return ! TransformStreamDefaultControllerPerformTransform(controller, chunk)."
    let promise = transform_stream_default_controller_perform_transform(
        controller.clone(),
        chunk.clone(),
        ec,
    )?;
    Ok(JsValue::from(promise))
}

fn sink_abort_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(
        TransformStreamDefaultController,
        ReadableStream,
        JsValue,
        bool,
    ),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, readable, reason, reject_finish_on_fulfilled_cancel) = captures;
    if *reject_finish_on_fulfilled_cancel {
        // Step 7.1.1: Reject finishPromise with readable.[[storedError]].
        if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
            resolvers.reject(readable.stored_error(), ec)?;
        }
    } else {
        // Step 7.1.2.1: "Perform ! ReadableStreamDefaultControllerError(readable.[[controller]], reason)."
        let readable_controller = readable
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
        readable_controller
            .as_default_controller()
            .error_steps(reason.clone(), ec)?;
        // Step 7.1.2.2: Resolve finishPromise.
        if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
            resolvers.resolve(ec.value_undefined(), ec)?;
        }
    }
    Ok(ec.value_undefined())
}

fn sink_abort_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    captures: &(TransformStreamDefaultController, ReadableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, readable) = captures;
    let error = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    // Step 7.2.1: "Perform ! ReadableStreamDefaultControllerError(readable.[[controller]], r)."
    let readable_controller = readable
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
    readable_controller
        .as_default_controller()
        .error_steps(error.clone(), ec)?;
    // Step 7.2.2: Reject finishPromise with r.
    if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
        resolvers.reject(error, ec)?;
    }
    Ok(ec.value_undefined())
}

fn sink_close_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(TransformStreamDefaultController, ReadableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, readable) = captures;
    let readable_state = readable.state();
    if readable_state == super::ReadableStreamState::Errored {
        // Step 7.1.1: Reject finishPromise with readable.[[storedError]].
        if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
            resolvers.reject(readable.stored_error(), ec)?;
        }
    } else {
        // Step 7.1.2.1: "Perform ! ReadableStreamDefaultControllerClose(readable.[[controller]])."
        let readable_controller = readable
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
        readable_controller
            .as_default_controller()
            .close_steps(ec)?;
        // Step 7.1.2.2: Resolve finishPromise.
        if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
            resolvers.resolve(ec.value_undefined(), ec)?;
        }
    }
    Ok(ec.value_undefined())
}

fn sink_close_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    captures: &(TransformStreamDefaultController, ReadableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, readable) = captures;
    let error = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    // Step 7.2.1: "Perform ! ReadableStreamDefaultControllerError(readable.[[controller]], r)."
    let readable_controller = readable
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
    readable_controller
        .as_default_controller()
        .error_steps(error.clone(), ec)?;
    // Step 7.2.2: Reject finishPromise with r.
    if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
        resolvers.reject(error, ec)?;
    }
    Ok(ec.value_undefined())
}

fn source_cancel_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(
        TransformStreamDefaultController,
        TransformStream,
        WritableStream,
        JsValue,
        bool,
    ),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, stream, writable, reason, reject_finish_on_fulfilled_cancel) = captures;
    let writable_state = writable.state();
    log_stream_debug(format!(
        "source cancel fulfilled writable_state={:?} reject_finish={} stored_error={}",
        writable_state,
        reject_finish_on_fulfilled_cancel,
        writable.stored_error().display()
    ));
    // Step 7.1.1: "If writable.[[state]] is \"errored\", reject controller.[[finishPromise]] with writable.[[storedError]]."
    if *reject_finish_on_fulfilled_cancel {
        if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
            resolvers.reject(writable.stored_error(), ec)?;
        }
    } else {
        // Step 7.1.2.1: "Perform ! WritableStreamDefaultControllerErrorIfNeeded(writable.[[controller]], reason)."
        let writable_controller = writable
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
        writable_stream_default_controller_error_if_needed(
            writable_controller.as_default_controller(),
            reason.clone(),
            ec,
        )?;
        // Step 7.1.2.2: "Perform ! TransformStreamUnblockWrite(stream)."
        transform_stream_unblock_write(stream, ec)?;
        // Step 7.1.2.3: "Resolve controller.[[finishPromise]] with undefined."
        if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
            resolvers.resolve(ec.value_undefined(), ec)?;
        }
    }
    Ok(ec.value_undefined())
}

fn source_cancel_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    captures: &(
        TransformStreamDefaultController,
        TransformStream,
        WritableStream,
    ),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, stream, writable) = captures;
    let error = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    // Step 7.2.1: "Perform ! WritableStreamDefaultControllerErrorIfNeeded(writable.[[controller]], r)."
    let writable_controller = writable
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
    writable_stream_default_controller_error_if_needed(
        writable_controller.as_default_controller(),
        error.clone(),
        ec,
    )?;
    // Step 7.2.2: "Perform ! TransformStreamUnblockWrite(stream)."
    transform_stream_unblock_write(stream, ec)?;
    // Step 7.2.3: Reject finishPromise with r.
    if let Some(resolvers) = controller.finish_resolvers.borrow_mut().take() {
        resolvers.reject(error, ec)?;
    }
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#initialize-transform-stream>
fn initialize_transform_stream(
    stream: &TransformStream,
    start_promise: JsObject,
    writable_high_water_mark: f64,
    writable_size_algorithm: SizeAlgorithm,
    readable_high_water_mark: f64,
    readable_size_algorithm: SizeAlgorithm,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Let startAlgorithm be an algorithm that returns startPromise."
    // Note: The readable and writable setup helpers expose distinct Rust enum types for the same spec algorithm.
    let global = ec.realm_global_object();
    let writable_start_algorithm =
        WritableStartAlgorithm::ReturnValue(JsValue::from(start_promise.clone()));
    let readable_start_algorithm =
        ReadableStartAlgorithm::ReturnValue(JsValue::from(start_promise));

    // Step 2: "Let writeAlgorithm be the following steps, taking a chunk argument:"
    let write_callback =
        crate::webidl::Callback::from_object(Types::object_from_function(ec.create_builtin_fn(
            Box::new({
                let c = stream.clone();
                move |args, this, ec| sink_write_algorithm_fn(args, this, &c, ec)
            }),
            1,
            ec.property_key_from_str(""),
        )));
    let write_algorithm =
        WriteAlgorithm::JavaScript(SourceMethod::new(global.clone(), write_callback));

    // Step 3: "Let abortAlgorithm be the following steps, taking a reason argument:"
    let abort_callback =
        crate::webidl::Callback::from_object(Types::object_from_function(ec.create_builtin_fn(
            Box::new({
                let c = stream.clone();
                move |args, this, ec| sink_abort_algorithm_fn(args, this, &c, ec)
            }),
            1,
            ec.property_key_from_str(""),
        )));
    let abort_algorithm =
        AbortAlgorithm::JavaScript(SourceMethod::new(global.clone(), abort_callback));

    // Step 4: "Let closeAlgorithm be the following steps:"
    let close_callback =
        crate::webidl::Callback::from_object(Types::object_from_function(ec.create_builtin_fn(
            Box::new({
                let c = stream.clone();
                move |args, this, ec| sink_close_algorithm_fn(args, this, &c, ec)
            }),
            0,
            ec.property_key_from_str(""),
        )));
    let close_algorithm = CloseAlgorithm::JavaScript(SourceMethod::new(global, close_callback));

    // Step 5: "Set stream.[[writable]] to ! CreateWritableStream(startAlgorithm, writeAlgorithm, closeAlgorithm, abortAlgorithm, writableHighWaterMark, writableSizeAlgorithm)."
    let (writable, writable_object) = create_writable_stream(
        writable_start_algorithm,
        write_algorithm,
        close_algorithm,
        abort_algorithm,
        Some(writable_high_water_mark),
        Some(writable_size_algorithm),
        ec,
    )?;
    *stream.writable.borrow_mut() = Some(writable);
    *stream.writable_object.borrow_mut() = Some(writable_object);

    // Step 6: "Let pullAlgorithm be the following steps:"
    let pull_algorithm = PullAlgorithm::TransformStreamDefaultSourcePull(stream.clone());

    // Step 7: "Let cancelAlgorithm be the following steps, taking a reason argument:"
    let cancel_algorithm = CancelAlgorithm::TransformStreamDefaultSourceCancel(stream.clone());

    // Step 8: "Set stream.[[readable]] to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancelAlgorithm, readableHighWaterMark, readableSizeAlgorithm)."
    let (readable, readable_object) = create_readable_stream(
        readable_start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        Some(readable_high_water_mark),
        Some(readable_size_algorithm),
        ec,
    )?;
    *stream.readable.borrow_mut() = Some(readable);
    *stream.readable_object.borrow_mut() = Some(readable_object);

    // Step 9: "Set stream.[[backpressure]] and stream.[[backpressureChangePromise]] to undefined."
    // Note: The implementation initializes [[backpressure]] with a boolean field and then immediately assigns the spec-visible initial state via TransformStreamSetBackpressure.

    // Step 10: "Perform ! TransformStreamSetBackpressure(stream, true)."
    transform_stream_set_backpressure(stream, true, ec)?;

    // Step 11: "Set stream.[[controller]] to undefined."
    *stream.controller.borrow_mut() = None;
    *stream.controller_object.borrow_mut() = None;

    Ok(())
}

/// <https://streams.spec.whatwg.org/#transform-stream-error>
fn transform_stream_error(
    stream: &TransformStream,
    error: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Perform ! ReadableStreamDefaultControllerError(stream.[[readable]].[[controller]], e)."
    let readable = stream.readable(ec)?;
    let readable_controller = readable
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
    readable_controller
        .as_default_controller()
        .error_steps(error.clone(), ec)?;

    // Step 2: "Perform ! TransformStreamErrorWritableAndUnblockWrite(stream, e)."
    transform_stream_error_writable_and_unblock_write(stream, error, ec)
}

/// <https://streams.spec.whatwg.org/#transform-stream-error-writable-and-unblock-write>
fn transform_stream_error_writable_and_unblock_write(
    stream: &TransformStream,
    error: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Perform ! TransformStreamDefaultControllerClearAlgorithms(stream.[[controller]])."
    let controller = stream.controller_slot(ec)?;
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 2: "Perform ! WritableStreamDefaultControllerErrorIfNeeded(stream.[[writable]].[[controller]], e)."
    let writable = stream.writable(ec)?;
    let writable_controller = writable
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
    writable_stream_default_controller_error_if_needed(
        writable_controller.as_default_controller(),
        error,
        ec,
    )?;

    // Step 3: "Perform ! TransformStreamUnblockWrite(stream)."
    transform_stream_unblock_write(stream, ec)
}

/// <https://streams.spec.whatwg.org/#transform-stream-set-backpressure>
fn transform_stream_set_backpressure(
    stream: &TransformStream,
    backpressure: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Assert: stream.[[backpressure]] is not backpressure."
    // Note: On first call during initialization, backpressure is undefined (treated as not-equal).

    // Step 2: "If stream.[[backpressureChangePromise]] is not undefined, resolve stream.[[backpressureChangePromise]] with undefined."
    if let Some(resolvers) = stream.backpressure_change_resolvers.borrow_mut().take() {
        resolvers.resolve(ec.value_undefined(), ec)?;
    }

    // Step 3: "Set stream.[[backpressureChangePromise]] to a new promise."
    let (promise, resolvers) = ec.new_promise_pending()?;
    let promise_obj = Types::value_as_object(&promise)
        .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
    *stream.backpressure_change_promise.borrow_mut() = Some(promise_obj);
    *stream.backpressure_change_resolvers.borrow_mut() = Some(resolvers);

    // Step 4: "Set stream.[[backpressure]] to backpressure."
    stream.backpressure.set(backpressure);

    Ok(())
}

/// <https://streams.spec.whatwg.org/#transform-stream-unblock-write>
fn transform_stream_unblock_write(
    stream: &TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "If stream.[[backpressure]] is true, perform ! TransformStreamSetBackpressure(stream, false)."
    if stream.backpressure() {
        transform_stream_set_backpressure(stream, false, ec)?;
    }

    Ok(())
}

// ---- Default controller operations ----

/// <https://streams.spec.whatwg.org/#set-up-transform-stream-default-controller>
fn set_up_transform_stream_default_controller(
    stream: &TransformStream,
    controller: TransformStreamDefaultController,
    controller_object: &JsObject,
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
    *stream.controller_object.borrow_mut() = Some(controller_object.clone());

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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<TransformStreamDefaultController, Types> {
    // Step 1: "Let controller be a new TransformStreamDefaultController."
    let (controller, controller_object) = create_transform_stream_default_controller(ec)?;

    // Step 2: Default transformAlgorithm is identity (enqueue the chunk).
    let mut transform_algorithm = TransformAlgorithm::Identity;

    // Step 3: Default flushAlgorithm returns resolved promise.
    let mut flush_algorithm = FlushAlgorithm::ReturnUndefined;

    // Step 4: Default cancelAlgorithm returns resolved promise.
    let mut cancel_algorithm = TransformCancelAlgorithm::ReturnUndefined;

    if let Some(transformer_obj) = transformer {
        // Step 5: "If transformerDict['transform'] exists..."
        if let Some(transform) = get_callable_method(transformer_obj, "transform", ec)? {
            transform_algorithm = TransformAlgorithm::JavaScript(SourceMethod::new(
                transformer_obj.clone(),
                crate::webidl::Callback::from_object(transform),
            ));
        }

        // Step 6: "If transformerDict['flush'] exists..."
        if let Some(flush) = get_callable_method(transformer_obj, "flush", ec)? {
            flush_algorithm = FlushAlgorithm::JavaScript(SourceMethod::new(
                transformer_obj.clone(),
                crate::webidl::Callback::from_object(flush),
            ));
        }

        // Step 7: "If transformerDict['cancel'] exists..."
        if let Some(cancel) = get_callable_method(transformer_obj, "cancel", ec)? {
            cancel_algorithm = TransformCancelAlgorithm::JavaScript(SourceMethod::new(
                transformer_obj.clone(),
                crate::webidl::Callback::from_object(cancel),
            ));
        }
    }

    // Step 8: "Perform ! SetUpTransformStreamDefaultController(stream, controller, transformAlgorithm, flushAlgorithm, cancelAlgorithm)."
    set_up_transform_stream_default_controller(
        stream,
        controller.clone(),
        &controller_object,
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot(ec)?;

    // Step 2: "Let readableController be stream.[[readable]].[[controller]]."
    let readable_controller = controller.readable_controller(ec)?;

    // Step 3: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(readableController) is false, throw a TypeError exception."
    if !readable_controller.can_close_or_enqueue(ec)? {
        return Err(ec.new_type_error("ReadableStream is not in a state that permits enqueue"));
    }

    // Step 4: "Let enqueueResult be ReadableStreamDefaultControllerEnqueue(readableController, chunk)."
    // Step 5: "If enqueueResult is an abrupt completion..."
    if let Err(error_value) = readable_controller.enqueue_steps(chunk, ec) {
        // Step 5.1: "Perform ! TransformStreamErrorWritableAndUnblockWrite(stream, enqueueResult.[[Value]])."
        transform_stream_error_writable_and_unblock_write(&stream, error_value, ec)?;

        // Step 5.2: "Throw stream.[[readable]].[[storedError]]."
        return Err(stream.readable(ec)?.stored_error());
    }

    // Step 6: "Let backpressure be ! ReadableStreamDefaultControllerHasBackpressure(readableController)."
    let backpressure = readable_controller.has_backpressure(ec)?;

    // Step 7: "If backpressure is not stream.[[backpressure]],"
    if backpressure != stream.backpressure() {
        // Step 7.1: "Assert: backpressure is true."
        debug_assert!(backpressure);

        // Step 7.2: "Perform ! TransformStreamSetBackpressure(stream, true)."
        transform_stream_set_backpressure(&stream, true, ec)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-error>
fn transform_stream_default_controller_error(
    controller: TransformStreamDefaultController,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Perform ! TransformStreamError(controller.[[stream]], e)."
    let stream = controller.stream_slot(ec)?;
    transform_stream_error(&stream, reason, ec)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-perform-transform>
fn transform_stream_default_controller_perform_transform(
    controller: TransformStreamDefaultController,
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let transformPromise be the result of performing controller.[[transformAlgorithm]], passing chunk."
    let transform_algorithm = controller.transform_algorithm.borrow().clone();
    let transform_promise = match transform_algorithm {
        Some(TransformAlgorithm::Identity) => {
            // Note: The default identity transform algorithm enqueues chunk directly.
            let enqueue_result =
                transform_stream_default_controller_enqueue(controller.clone(), chunk, ec);
            match enqueue_result {
                Err(error) => rejected_promise(error, ec)?,
                Ok(_) => resolved_promise(ec.value_undefined(), ec)?,
            }
        }
        Some(TransformAlgorithm::JavaScript(ref callback)) => {
            let controller_value = JsValue::from(controller.controller_object(ec)?);
            match callback.call(&[chunk, controller_value], ec) {
                Ok(value) => promise_from_value(value, ec)?,
                Err(error) => rejected_promise(error, ec)?,
            }
        }
        None => {
            return Err(ec.new_type_error(
                "TransformStreamDefaultController is missing its transform algorithm",
            ));
        }
    };

    // Step 2: "Return the result of reacting to transformPromise with the following rejection steps given the argument r:"
    let stream = controller.stream_slot(ec)?;
    let on_rejected = ec.create_builtin_fn(
        Box::new({
            let c = stream;
            move |args, this, ec| perform_transform_on_rejected_fn(args, this, &c, ec)
        }),
        1,
        ec.property_key_from_str(""),
    );
    let transform_js_promise = Types::object_as_promise(&transform_promise)
        .ok_or_else(|| ec.new_type_error("transformPromise is not a Promise"))?;
    let result_promise =
        ec.perform_promise_then(transform_js_promise, None, Some(on_rejected), None)?;
    Ok(Types::value_as_object(&result_promise).unwrap_or_else(|| ec.realm_global_object()))
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-controller-terminate>
fn transform_stream_default_controller_terminate(
    controller: TransformStreamDefaultController,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot(ec)?;

    // Step 2: "Let readableController be stream.[[readable]].[[controller]]."
    let readable_controller = controller.readable_controller(ec)?;

    // Step 3: "Perform ! ReadableStreamDefaultControllerClose(readableController)."
    readable_controller.close_steps(ec)?;

    // Step 4: "Let error be a TypeError exception indicating that the stream has been terminated."
    let error = type_error_value("TransformStream has been terminated", ec)?;

    let writable = stream.writable(ec)?;
    log_stream_debug(format!(
        "terminate before error writable_state={:?}",
        writable.state()
    ));

    // Step 5: "Perform ! TransformStreamErrorWritableAndUnblockWrite(stream, error)."
    let result = transform_stream_error_writable_and_unblock_write(&stream, error, ec);
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Assert: stream.[[writable]].[[state]] is \"writable\"."

    // Step 2: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot(ec)?;

    // Step 3: "If stream.[[backpressure]] is true,"
    if stream.backpressure() {
        // Step 3.1: "Let backpressureChangePromise be stream.[[backpressureChangePromise]]."
        let backpressure_change_promise =
            stream.backpressure_change_promise().ok_or_else(|| {
                ec.new_type_error("TransformStream is missing its backpressure change promise")
            })?;

        // Step 3.2: "Assert: backpressureChangePromise is not undefined."

        // Step 3.3: "Return the result of reacting to backpressureChangePromise with the following fulfillment steps:"
        let on_fulfilled = ec.create_builtin_fn(
            Box::new({
                let c = (stream, controller, chunk);
                move |args, this, ec| controller_enqueue_on_fulfilled_fn(args, this, &c, ec)
            }),
            0,
            ec.property_key_from_str(""),
        );

        let backpressure_js_promise = Types::object_as_promise(&backpressure_change_promise)
            .ok_or_else(|| ec.new_type_error("backpressureChangePromise is not a Promise"))?;
        let result =
            ec.perform_promise_then(backpressure_js_promise, Some(on_fulfilled), None, None)?;
        return Ok(Types::value_as_object(&result).unwrap_or_else(|| ec.realm_global_object()));
    }

    // Step 4: "Return ! TransformStreamDefaultControllerPerformTransform(controller, chunk)."
    transform_stream_default_controller_perform_transform(controller, chunk, ec)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-sink-abort-algorithm>
fn transform_stream_default_sink_abort_algorithm(
    stream: TransformStream,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot(ec)?;

    // Step 2: "If controller.[[finishPromise]] is not undefined, return controller.[[finishPromise]]."
    if let Some(finish_promise) = controller.finish_promise.borrow().clone() {
        return Ok(finish_promise);
    }

    // Step 3: "Let readable be stream.[[readable]]."
    let readable = stream.readable(ec)?;

    // Step 4: "Let controller.[[finishPromise]] be a new promise."
    let (finish_promise, finish_resolvers) = ec.new_promise_pending()?;
    let finish_promise_obj = Types::value_as_object(&finish_promise)
        .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
    *controller.finish_promise.borrow_mut() = Some(finish_promise_obj.clone());
    *controller.finish_resolvers.borrow_mut() = Some(finish_resolvers);

    // Step 5: "Let cancelPromise be the result of performing controller.[[cancelAlgorithm]], passing reason."
    let readable_state_before_cancel = readable.state();
    let cancel_algorithm = controller.cancel_algorithm.borrow().clone();
    let cancel_promise = match cancel_algorithm {
        Some(TransformCancelAlgorithm::ReturnUndefined) => {
            let (cancel_value, resolvers) = ec.new_promise_pending()?;
            resolvers.resolve(ec.value_undefined(), ec)?;
            Types::value_as_object(&cancel_value)
                .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?
        }
        Some(TransformCancelAlgorithm::JavaScript(ref callback)) => {
            match callback.call(&[reason.clone()], ec) {
                Ok(value) => promise_from_value(value, ec)?,
                Err(error) => rejected_promise(error, ec)?,
            }
        }
        None => {
            let (cancel_value, resolvers) = ec.new_promise_pending()?;
            resolvers.resolve(ec.value_undefined(), ec)?;
            Types::value_as_object(&cancel_value)
                .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?
        }
    };
    let reject_finish_on_fulfilled_cancel = readable_state_before_cancel
        == super::ReadableStreamState::Readable
        && readable.state() == super::ReadableStreamState::Errored;

    // Step 6: "Perform ! TransformStreamDefaultControllerClearAlgorithms(controller)."
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 7: React to cancelPromise.
    let on_fulfilled = ec.create_builtin_fn(
        Box::new({
            let c = (
                controller.clone(),
                readable.clone(),
                reason,
                reject_finish_on_fulfilled_cancel,
            );
            move |args, this, ec| sink_abort_on_fulfilled_fn(args, this, &c, ec)
        }),
        0,
        ec.property_key_from_str(""),
    );

    let on_rejected = ec.create_builtin_fn(
        Box::new({
            let c = (controller, readable);
            move |args, this, ec| sink_abort_on_rejected_fn(args, this, &c, ec)
        }),
        1,
        ec.property_key_from_str(""),
    );

    let cancel_js_promise = Types::object_as_promise(&cancel_promise)
        .ok_or_else(|| ec.new_type_error("cancelPromise is not a Promise"))?;
    ec.perform_promise_then(
        cancel_js_promise,
        Some(on_fulfilled),
        Some(on_rejected),
        None,
    )?;

    // Step 8: "Return controller.[[finishPromise]]."
    Ok(finish_promise_obj)
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-sink-close-algorithm>
fn transform_stream_default_sink_close_algorithm(
    stream: TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot(ec)?;

    // Step 2: "If controller.[[finishPromise]] is not undefined, return controller.[[finishPromise]]."
    if let Some(finish_promise) = controller.finish_promise.borrow().clone() {
        return Ok(finish_promise);
    }

    // Step 3: "Let readable be stream.[[readable]]."
    let readable = stream.readable(ec)?;

    // Step 4: "Let controller.[[finishPromise]] be a new promise."
    let (finish_promise, finish_resolvers) = ec.new_promise_pending()?;
    let finish_promise_obj = Types::value_as_object(&finish_promise)
        .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
    *controller.finish_promise.borrow_mut() = Some(finish_promise_obj.clone());
    *controller.finish_resolvers.borrow_mut() = Some(finish_resolvers);

    // Step 5: "Let flushPromise be the result of performing controller.[[flushAlgorithm]]."
    let flush_algorithm = controller.flush_algorithm.borrow().clone();
    let flush_promise = match flush_algorithm {
        Some(FlushAlgorithm::ReturnUndefined) => {
            // Immediately resolved promise (no enqueue needed in EC path)
            let (promise_value, resolvers) = ec.new_promise_pending()?;
            resolvers.resolve(ec.value_undefined(), ec)?;
            Types::value_as_object(&promise_value)
                .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?
        }
        Some(FlushAlgorithm::JavaScript(ref callback)) => {
            let controller_value = JsValue::from(controller.controller_object(ec)?);
            match callback.call(&[controller_value], ec) {
                Ok(value) => promise_from_value(value, ec)?,
                Err(error) => rejected_promise(error, ec)?,
            }
        }
        None => {
            // Immediately resolved promise
            let (promise_value, resolvers) = ec.new_promise_pending()?;
            resolvers.resolve(ec.value_undefined(), ec)?;
            Types::value_as_object(&promise_value)
                .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?
        }
    };

    // Step 6: "Perform ! TransformStreamDefaultControllerClearAlgorithms(controller)."
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 7: React to flushPromise.
    let on_fulfilled = ec.create_builtin_fn(
        Box::new({
            let c = (controller.clone(), readable.clone());
            move |args, this, ec| sink_close_on_fulfilled_fn(args, this, &c, ec)
        }),
        0,
        ec.property_key_from_str(""),
    );

    let on_rejected = ec.create_builtin_fn(
        Box::new({
            let c = (controller, readable);
            move |args, this, ec| sink_close_on_rejected_fn(args, this, &c, ec)
        }),
        1,
        ec.property_key_from_str(""),
    );

    let flush_js_promise = Types::object_as_promise(&flush_promise)
        .ok_or_else(|| ec.new_type_error("flushPromise is not a Promise"))?;
    ec.perform_promise_then(
        flush_js_promise,
        Some(on_fulfilled),
        Some(on_rejected),
        None,
    )?;

    // Step 8: "Return controller.[[finishPromise]]."
    Ok(finish_promise_obj)
}

// ---- Default source algorithms ----

/// <https://streams.spec.whatwg.org/#transform-stream-default-source-pull-algorithm>
pub(crate) fn transform_stream_default_source_pull_algorithm(
    stream: TransformStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Assert: stream.[[backpressure]] is true."
    debug_assert!(stream.backpressure());

    // Step 2: "Assert: stream.[[backpressureChangePromise]] is not undefined."
    debug_assert!(stream.backpressure_change_promise().is_some());

    // Step 3: "Perform ! TransformStreamSetBackpressure(stream, false)."
    transform_stream_set_backpressure(&stream, false, ec)?;

    // Step 4: "Return stream.[[backpressureChangePromise]]."
    stream.backpressure_change_promise().ok_or_else(|| {
        ec.new_type_error("TransformStream is missing its backpressure change promise")
    })
}

/// <https://streams.spec.whatwg.org/#transform-stream-default-source-cancel-algorithm>
pub(crate) fn transform_stream_default_source_cancel_algorithm(
    stream: TransformStream,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let controller be stream.[[controller]]."
    let controller = stream.controller_slot(ec)?;

    // Step 2: "If controller.[[finishPromise]] is not undefined, return controller.[[finishPromise]]."
    if let Some(finish_promise) = controller.finish_promise.borrow().clone() {
        return Ok(finish_promise);
    }

    // Step 3: "Let writable be stream.[[writable]]."
    let writable = stream.writable(ec)?;

    // Step 4: "Let controller.[[finishPromise]] be a new promise."
    let (finish_promise, finish_resolvers) = ec.new_promise_pending()?;
    let finish_promise_obj = Types::value_as_object(&finish_promise)
        .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
    *controller.finish_promise.borrow_mut() = Some(finish_promise_obj.clone());
    *controller.finish_resolvers.borrow_mut() = Some(finish_resolvers);

    // Step 5: "Let cancelPromise be the result of performing controller.[[cancelAlgorithm]], passing reason."
    let writable_state_before_cancel = writable.state();
    let cancel_algorithm = controller.cancel_algorithm.borrow().clone();
    let cancel_promise = match cancel_algorithm {
        Some(TransformCancelAlgorithm::ReturnUndefined) => {
            // Immediately resolved promise
            let (promise_value, resolvers) = ec.new_promise_pending()?;
            resolvers.resolve(ec.value_undefined(), ec)?;
            Types::value_as_object(&promise_value)
                .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?
        }
        Some(TransformCancelAlgorithm::JavaScript(ref callback)) => {
            match callback.call(&[reason.clone()], ec) {
                Ok(value) => promise_from_value(value, ec)?,
                Err(error) => rejected_promise(error, ec)?,
            }
        }
        None => {
            // Immediately resolved promise
            let (promise_value, resolvers) = ec.new_promise_pending()?;
            resolvers.resolve(ec.value_undefined(), ec)?;
            Types::value_as_object(&promise_value)
                .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?
        }
    };
    let reject_finish_on_fulfilled_cancel = writable_state_before_cancel
        == super::WritableStreamState::Writable
        && writable.state() != super::WritableStreamState::Writable;

    // Step 6: "Perform ! TransformStreamDefaultControllerClearAlgorithms(controller)."
    transform_stream_default_controller_clear_algorithms(&controller);

    // Step 7: React to cancelPromise.
    let on_fulfilled = ec.create_builtin_fn(
        Box::new({
            let c = (
                controller.clone(),
                stream.clone(),
                writable.clone(),
                reason,
                reject_finish_on_fulfilled_cancel,
            );
            move |args, this, ec| source_cancel_on_fulfilled_fn(args, this, &c, ec)
        }),
        0,
        ec.property_key_from_str(""),
    );

    let on_rejected = ec.create_builtin_fn(
        Box::new({
            let c = (controller, stream, writable);
            move |args, this, ec| source_cancel_on_rejected_fn(args, this, &c, ec)
        }),
        1,
        ec.property_key_from_str(""),
    );

    let cancel_js_promise = Types::object_as_promise(&cancel_promise)
        .ok_or_else(|| ec.new_type_error("cancelPromise is not a Promise"))?;
    ec.perform_promise_then(
        cancel_js_promise,
        Some(on_fulfilled),
        Some(on_rejected),
        None,
    )?;

    // Step 8: "Return controller.[[finishPromise]]."
    Ok(finish_promise_obj)
}

// ---- Constructor helpers ----

fn create_transform_stream_default_controller(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(TransformStreamDefaultController, JsObject), Types> {
    let controller = TransformStreamDefaultController::new();
    let controller_object: JsObject = create_interface_instance::<
        Types,
        TransformStreamDefaultController,
    >(controller.clone(), ec)?
    .into();
    Ok((controller, controller_object))
}

/// <https://streams.spec.whatwg.org/#ts-constructor>
pub(crate) fn construct_transform_stream(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<TransformStream, Types> {
    let stream = TransformStream::new();
    let undefined = ec.value_undefined();

    // Step 1: "If transformer is missing, set it to null."
    let null_val = ec.value_null();
    let transformer = if args.is_empty() {
        null_val.clone()
    } else {
        args[0].clone()
    };

    let transformer_object =
        if ec.same_value(&transformer, &null_val) || ec.same_value(&transformer, &undefined) {
            None
        } else {
            Some(Types::value_as_object(&transformer).ok_or_else(|| {
                ec.new_type_error("TransformStream transformer must be an object")
            })?)
        };

    // Step 2: "Let transformerDict be transformer, converted to an IDL value of type Transformer."
    // Note: The implementation retains the original transformer object so it can invoke the transformer callbacks with the original callback this value.

    // Step 3: "If transformerDict[\"readableType\"] exists, throw a RangeError exception."
    if let Some(ref obj) = transformer_object {
        let readable_type_key = ec.property_key_from_str("readableType");
        if ec.has_property(obj.clone(), readable_type_key)? {
            return Err(
                ec.new_range_error("TransformStream transformer.readableType is not supported")
            );
        }

        // Step 4: "If transformerDict[\"writableType\"] exists, throw a RangeError exception."
        let writable_type_key = ec.property_key_from_str("writableType");
        if ec.has_property(obj.clone(), writable_type_key)? {
            return Err(
                ec.new_range_error("TransformStream transformer.writableType is not supported")
            );
        }
    }

    // Step 5: "Let readableHighWaterMark be ? ExtractHighWaterMark(readableStrategy, 0)."
    let readable_strategy = args.get(2).cloned().unwrap_or(undefined.clone());
    let readable_high_water_mark = extract_high_water_mark(&readable_strategy, 0.0, ec)?;

    // Step 6: "Let readableSizeAlgorithm be ! ExtractSizeAlgorithm(readableStrategy)."
    let readable_size_algorithm = extract_size_algorithm(&readable_strategy, ec)?;

    // Step 7: "Let writableHighWaterMark be ? ExtractHighWaterMark(writableStrategy, 1)."
    let writable_strategy = args.get(1).cloned().unwrap_or(undefined.clone());
    let writable_high_water_mark = extract_high_water_mark(&writable_strategy, 1.0, ec)?;

    // Step 8: "Let writableSizeAlgorithm be ! ExtractSizeAlgorithm(writableStrategy)."
    let writable_size_algorithm = extract_size_algorithm(&writable_strategy, ec)?;

    // Step 9: "Let startPromise be a new promise."
    let (start_promise, start_resolvers) = ec.new_promise_pending()?;
    let start_promise_obj = Types::value_as_object(&start_promise)
        .ok_or_else(|| ec.new_type_error("startPromise is not an object"))?;

    // Step 10: "Perform ! InitializeTransformStream(this, startPromise, ...)."
    initialize_transform_stream(
        &stream,
        start_promise_obj,
        writable_high_water_mark,
        writable_size_algorithm,
        readable_high_water_mark,
        readable_size_algorithm,
        ec,
    )?;

    // Step 11: "Perform ? SetUpTransformStreamDefaultControllerFromTransformer(this, transformer, transformerDict)."
    let controller = set_up_transform_stream_default_controller_from_transformer(
        &stream,
        transformer_object.as_ref(),
        ec,
    )?;

    // Step 12: "If transformerDict[\"start\"] exists, then resolve startPromise with the result of invoking transformerDict[\"start\"] with argument list « this.[[controller]] » and callback this value transformer."
    if let Some(ref transformer_obj) = transformer_object {
        if let Some(start) = get_callable_method(transformer_obj, "start", ec)? {
            let controller_value = JsValue::from(controller.controller_object(ec)?);
            let source_method = SourceMethod::new(
                transformer_obj.clone(),
                crate::webidl::Callback::from_object(start),
            );
            let result = source_method.call(&[controller_value], ec)?;
            ec.call(&start_resolvers.resolve, &undefined, &[result])?;
        } else {
            // Step 13: "Otherwise, resolve startPromise with undefined."
            ec.call(&start_resolvers.resolve, &undefined, &[undefined.clone()])?;
        }
    } else {
        // Step 13: "Otherwise, resolve startPromise with undefined."
        ec.call(&start_resolvers.resolve, &undefined, &[undefined.clone()])?;
    }

    Ok(stream)
}

pub(crate) fn with_transform_stream_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&TransformStream) -> R,
) -> Completion<R, Types> {
    let stream_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<TransformStream>());
    let stream = match stream_ref {
        Some(s) => s,
        None => return Err(ec.new_type_error("object is not a TransformStream")),
    };
    Ok(f(stream))
}

pub(crate) fn with_transform_stream_default_controller_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&TransformStreamDefaultController) -> R,
) -> Completion<R, Types> {
    let ctrl_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<TransformStreamDefaultController>());
    let controller = match ctrl_ref {
        Some(c) => c,
        None => return Err(ec.new_type_error("object is not a TransformStreamDefaultController")),
    };
    Ok(f(controller))
}

fn get_callable_method(
    object: &JsObject,
    property: &'static str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<JsObject>, Types> {
    let value = js_engine::EcmascriptHost::get(ec, object, property)?;
    let undefined = ec.value_undefined();
    if ec.same_value(&value, &undefined) {
        return Ok(None);
    }

    let method = Types::value_as_object(&value).ok_or_else(|| {
        ec.new_type_error("TransformStream transformer property must be callable when provided")
    })?;
    if !ec.is_callable(&value) {
        return Err(ec.new_type_error(
            "TransformStream transformer property must be callable when provided",
        ));
    }

    Ok(Some(method.clone()))
}
