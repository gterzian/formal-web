use log::error;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue,
    builtins::{
        iterable::create_iter_result_object,
        promise::{PromiseState, ResolvingFunctions},
    },
    js_string,
    native_function::NativeFunction,
    object::{
        JsObject,
        builtins::{JsArray, JsFunction, JsPromise},
    },
    symbol::JsSymbol,
};
use boa_gc::{Finalize, Gc, GcRef, GcRefMut, Trace};

use crate::dom::{AbortAlgorithm as SignalAbortAlgorithm, AbortSignal};
use crate::js::with_abort_signal_ref;
use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{
    error_to_rejection_reason, mark_promise_as_handled, promise_from_completion,
    promise_from_value, rejected_promise, rejected_promise_from_error, resolved_promise,
    transform_promise_to_undefined,
};
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

use super::{
    ArrayBufferViewDescriptor, CancelAlgorithm, PullAlgorithm, ReadIntoRequest, ReadRequest,
    ReadableByteStreamController, ReadableStreamController, ReadableStreamDefaultReader,
    ReadableStreamGenericReader, ReadableStreamReader, ReadableStreamState, StartAlgorithm,
    acquire_readable_stream_byob_reader, acquire_readable_stream_default_reader,
    queue_internal_stream_microtask, readable_stream_byob_reader_release,
    readable_stream_default_reader_error_read_requests, readable_stream_default_reader_release,
    rejected_type_error_promise, set_up_readable_byte_stream_controller_from_underlying_source,
    set_up_readable_stream_default_controller,
    set_up_readable_stream_default_controller_from_underlying_source, type_error_value,
    with_readable_stream_byob_reader_ref, with_readable_stream_default_reader_ref,
};
use js_engine::{Completion, ExecutionContext};

/// <https://streams.spec.whatwg.org/#rs-class>
#[gc_struct]
pub struct ReadableStream {
    /// <https://streams.spec.whatwg.org/#readablestream-controller>
    controller: GcCell<Option<ReadableStreamController>>,

    controller_object: GcCell<Option<JsObject>>,

    /// <https://streams.spec.whatwg.org/#readablestream-reader>
    reader: GcCell<Option<ReadableStreamReader>>,

    /// <https://streams.spec.whatwg.org/#readablestream-disturbed>
    #[unsafe_ignore_trace]
    disturbed: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestream-state>
    #[unsafe_ignore_trace]
    state: Rc<RefCell<ReadableStreamState>>,

    /// <https://streams.spec.whatwg.org/#readablestream-storederror>
    stored_error: GcCell<JsValue>,
}

impl ReadableStream {
    pub(crate) fn new() -> Self {
        Self {
            controller: gc_cell_new(None),
            controller_object: gc_cell_new(None),
            reader: gc_cell_new(None),
            disturbed: Rc::new(Cell::new(false)),
            state: Rc::new(RefCell::new(ReadableStreamState::Readable)),
            stored_error: gc_cell_new(JsValue::undefined()),
        }
    }

    pub(crate) fn controller_slot(&self) -> Option<ReadableStreamController> {
        self.controller.borrow().clone()
    }

    pub(crate) fn set_controller_slot(&self, controller: Option<ReadableStreamController>) {
        *self.controller.borrow_mut() = controller;
    }

    pub(crate) fn controller_object_slot(&self) -> Option<JsObject> {
        self.controller_object.borrow().clone()
    }

    pub(crate) fn set_controller_object_slot(&self, controller_object: Option<JsObject>) {
        *self.controller_object.borrow_mut() = controller_object;
    }

    pub(crate) fn reader_slot(&self) -> Option<ReadableStreamReader> {
        self.reader.borrow().clone()
    }

    pub(crate) fn set_reader_slot(&self, reader: Option<ReadableStreamReader>) {
        *self.reader.borrow_mut() = reader;
    }

    pub(crate) fn state(&self) -> ReadableStreamState {
        self.state.borrow().clone()
    }

    pub(crate) fn set_state(&self, state: ReadableStreamState) {
        *self.state.borrow_mut() = state;
    }

    pub(crate) fn stored_error(&self) -> JsValue {
        self.stored_error.borrow().clone()
    }

    pub(crate) fn set_stored_error(&self, error: JsValue) {
        *self.stored_error.borrow_mut() = error;
    }

    pub(crate) fn set_disturbed(&self, disturbed: bool) {
        self.disturbed.set(disturbed);
    }

    /// <https://streams.spec.whatwg.org/#initialize-readable-stream>
    fn initialize_readable_stream(&mut self) {
        // Step 1: "Set stream.[[state]] to \"readable\"."
        *self.state.borrow_mut() = ReadableStreamState::Readable;

        // Step 2: "Set stream.[[reader]] and stream.[[storedError]] to undefined."
        *self.reader.borrow_mut() = None;
        *self.stored_error.borrow_mut() = JsValue::undefined();

        // Step 3: "Set stream.[[disturbed]] to false."
        self.disturbed.set(false);
    }

    /// <https://streams.spec.whatwg.org/#is-readable-stream-locked>
    pub(crate) fn is_readable_stream_locked(&self) -> bool {
        // Step 1: "If stream.[[reader]] is undefined, return false."
        if self.reader_slot().is_none() {
            return false;
        }

        // Step 2: "Return true."
        true
    }

    /// <https://streams.spec.whatwg.org/#rs-locked>
    pub(crate) fn locked(&self) -> bool {
        // Step 1: "Return ! IsReadableStreamLocked(this)."
        self.is_readable_stream_locked()
    }

    /// <https://streams.spec.whatwg.org/#rs-cancel>
    pub(crate) fn cancel(&mut self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.is_readable_stream_locked() {
            let ec_ref = js_engine::boa::context_as_ec(context);
            return crate::js::completion_to_js_result(rejected_type_error_promise(
                "Cannot cancel a stream that already has a reader",
                ec_ref,
            ));
        }

        // Step 2: "Return ! ReadableStreamCancel(this, reason)."
        readable_stream_cancel(self.clone(), reason, context)
    }

    /// Generic entry point for <https://streams.spec.whatwg.org/#rs-cancel>.
    /// Returns `Completion` — the binding layer uses this directly without bridging.
    pub(crate) fn cancel_ec(
        &mut self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.is_readable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot cancel a stream that already has a reader",
                ec,
            );
        }

        // Step 2: "Return ! ReadableStreamCancel(this, reason)."
        readable_stream_cancel_ec(self.clone(), reason, ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-get-reader>
    pub(crate) fn get_reader(
        &mut self,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        self.get_reader_ec(options, js_engine::boa::context_as_ec(context))
            .map_err(|e| JsError::from_opaque(e))
    }

    /// Generic entry point for <https://streams.spec.whatwg.org/#rs-get-reader>.
    /// Returns `Completion` — the binding layer uses this directly without bridging.
    pub(crate) fn get_reader_ec(
        &mut self,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        let options_object: Option<JsObject> = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(options.as_object().ok_or_else(|| {
                ec.new_type_error("ReadableStream.getReader() options must be an object")
            })?)
        };

        // Step 1: "If options[\"mode\"] does not exist, return ? AcquireReadableStreamDefaultReader(this)."
        let Some(options_object) = options_object else {
            return acquire_readable_stream_default_reader(self.clone(), ec);
        };

        let mode_key = ec.property_key_from_str("mode");
        if !ec.has_property(options_object.clone(), mode_key.clone())? {
            return acquire_readable_stream_default_reader(self.clone(), ec);
        }

        let mode = ExecutionContext::get(ec, options_object.clone(), mode_key)?;
        if mode.is_undefined() {
            return acquire_readable_stream_default_reader(self.clone(), ec);
        }

        // Step 2: "Assert: options[\"mode\"] is \"byob\"."
        let mode = ec.to_rust_string(mode)?;
        if mode != "byob" {
            return Err(ec.new_type_error(
                "ReadableStream.getReader() only supports the default reader mode",
            ));
        }

        // Step 3: "Return ? AcquireReadableStreamBYOBReader(this)."
        let Some(controller) = self.controller_slot() else {
            return Err(ec.new_type_error("ReadableStream is missing its controller"));
        };
        if controller.as_byte_controller().is_none() {
            return Err(ec.new_type_error("Cannot acquire a BYOB reader for a non-byte stream"));
        }

        acquire_readable_stream_byob_reader(self.clone(), ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-through>
    pub(crate) fn pipe_through(
        &mut self,
        transform: &JsValue,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, throw a TypeError exception."
        if self.locked() {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.pipeThrough() called on a locked stream")
                .into());
        }

        // Step 2: "If ! IsWritableStreamLocked(transform[\"writable\"]) is true, throw a TypeError exception."
        // Note: This implementation performs the lock check below, after reading options members.
        let transform_obj = transform.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream.pipeThrough() requires a ReadableWritablePair")
        })?;
        let readable_value = transform_obj.get(js_string!("readable"), context)?;
        let readable_obj = readable_value.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableWritablePair is missing its readable property")
        })?;
        let _ = with_readable_stream_ref(&readable_obj, |stream| stream.clone())?;

        let writable_value = transform_obj.get(js_string!("writable"), context)?;
        let writable_obj = writable_value.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableWritablePair is missing its writable property")
        })?;

        // Step 3: "Let signal be options[\"signal\"] if it exists, or undefined otherwise."
        //
        // Note: The implementation order diverges from the specification.
        // The specification performs Step 2 (IsWritableStreamLocked) before reading options members.
        // This implementation normalizes options first so option getters run before the lock check.
        // This ordering is currently required to match WPT behavior for
        // pipeThrough() should throw if an option getter grabs a writer.
        let options = normalize_pipe_options(options, context)?;

        // Step 2: "If ! IsWritableStreamLocked(transform[\"writable\"]) is true, throw a TypeError exception."
        let writable_locked = super::with_writable_stream_ref(&writable_obj, |ws| ws.locked())?;
        if writable_locked {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.pipeThrough(): destination writable stream is locked")
                .into());
        }

        // Step 4: "Let promise be ! ReadableStreamPipeTo(this, transform[\"writable\"], options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        let destination = super::with_writable_stream_ref(&writable_obj, |ws| ws.clone())?;
        let promise = readable_stream_pipe_to(
            self.clone(),
            destination,
            options.prevent_close,
            options.prevent_abort,
            options.prevent_cancel,
            options.signal,
            context,
        );

        // Step 5: "Set promise.[[PromiseIsHandled]] to true."
        crate::webidl::mark_promise_as_handled(&promise, js_engine::boa::context_as_ec(context))
            .map_err(boa_engine::JsError::from_opaque)?;

        // Step 6: "Return transform[\"readable\"]."
        Ok(readable_value)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-to>
    pub(crate) fn pipe_to(
        &mut self,
        destination: &JsValue,
        options: &JsValue,
        context: &mut Context,
    ) -> JsObject {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.locked() {
            return promise_rejected_with_type_error(
                "ReadableStream.pipeTo() called on a locked stream",
                context,
            );
        }

        // Step 2: "If ! IsWritableStreamLocked(destination) is true, return a promise rejected with a TypeError exception."
        let dest_obj = match destination.as_object() {
            Some(obj) => obj.clone(),
            None => {
                return promise_rejected_with_type_error(
                    "ReadableStream.pipeTo() requires a WritableStream destination",
                    context,
                );
            }
        };
        let dest_locked = match super::with_writable_stream_ref(&dest_obj, |ws| ws.locked()) {
            Ok(locked) => locked,
            Err(error) => return promise_rejected_with_error(error, context),
        };
        if dest_locked {
            return promise_rejected_with_type_error(
                "ReadableStream.pipeTo(): destination is locked",
                context,
            );
        }

        let options = match normalize_pipe_options(options, context) {
            Ok(options) => options,
            Err(error) => return promise_rejected_with_error(error, context),
        };

        let dest = match super::with_writable_stream_ref(&dest_obj, |ws| ws.clone()) {
            Ok(dest) => dest,
            Err(error) => return promise_rejected_with_error(error, context),
        };

        // Step 4: "Return ! ReadableStreamPipeTo(this, destination, options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        readable_stream_pipe_to(
            self.clone(),
            dest,
            options.prevent_close,
            options.prevent_abort,
            options.prevent_cancel,
            options.signal,
            context,
        )
    }

    /// <https://streams.spec.whatwg.org/#rs-tee>
    pub(crate) fn tee(&mut self, context: &mut Context) -> JsResult<JsValue> {
        // Step 1: "Return ? ReadableStreamTee(this, false)."
        Ok(readable_stream_tee(self.clone(), false, context)?.into_js_value(context))
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-through>
    pub(crate) fn pipe_through_ec(
        &mut self,
        transform: &JsValue,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        self.pipe_through(transform, options, ctx)
            .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-to>
    pub(crate) fn pipe_to_ec(
        &mut self,
        destination: &JsValue,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        Ok(self.pipe_to(destination, options, ctx))
    }

    /// <https://streams.spec.whatwg.org/#rs-tee>
    pub(crate) fn tee_ec(
        &mut self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        self.tee(ctx)
            .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))
    }
}

struct ReadableStreamTeeBranches {
    _branch1: ReadableStream,
    branch1_object: JsObject,
    _branch2: ReadableStream,
    branch2_object: JsObject,
}

impl ReadableStreamTeeBranches {
    fn into_js_value(self, context: &mut Context) -> JsValue {
        JsArray::from_iter(
            [self.branch1_object, self.branch2_object]
                .into_iter()
                .map(JsValue::from),
            context,
        )
        .into()
    }
}

/// pull and cancel algorithms.
#[derive(Trace, Finalize)]
pub(crate) struct TeeState {
    source_stream: ReadableStream,
    reader: ReadableStreamDefaultReader,
    branch1: Option<ReadableStream>,
    branch2: Option<ReadableStream>,
    cancel_promise: JsObject,
    cancel_resolvers: boa_engine::builtins::promise::ResolvingFunctions,
    #[unsafe_ignore_trace]
    reading: bool,
    #[unsafe_ignore_trace]
    read_again: bool,
    #[unsafe_ignore_trace]
    canceled1: bool,
    #[unsafe_ignore_trace]
    canceled2: bool,
    reason1: JsValue,
    reason2: JsValue,
}

/// <https://streams.spec.whatwg.org/#readable-stream-tee>
fn readable_stream_tee(
    stream: ReadableStream,
    clone_for_branch2: bool,
    context: &mut Context,
) -> JsResult<ReadableStreamTeeBranches> {
    // Step 1: "Assert: stream implements ReadableStream."
    // Step 2: "Assert: cloneForBranch2 is a boolean."

    // Step 3: "If stream.[[controller]] implements ReadableByteStreamController, return ? ReadableByteStreamTee(stream)."
    if stream
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
        .is_some()
    {
        return readable_byte_stream_tee(stream, context);
    }

    // Step 4: "Return ? ReadableStreamDefaultTee(stream, cloneForBranch2)."
    readable_stream_default_tee(stream, clone_for_branch2, context)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
fn readable_stream_default_tee(
    stream: ReadableStream,
    clone_for_branch2: bool,
    context: &mut Context,
) -> JsResult<ReadableStreamTeeBranches> {
    // Step 1: "Assert: stream implements ReadableStream."
    // Step 2: "Assert: cloneForBranch2 is a boolean."

    // Step 3: "Let reader be ? AcquireReadableStreamDefaultReader(stream)."
    let reader_object =
        crate::js::completion_to_js_result(acquire_readable_stream_default_reader(
            stream.clone(),
            js_engine::boa::context_as_ec(context),
        ))?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, |reader| reader.clone())?;

    // Step 12: "Let cancelPromise be a new promise."
    let reader_closed_promise = reader.closed()?;

    // Step 19: "Upon rejection of reader.[[closedPromise]] with reason r,"
    // Note: mark the source reader's closed promise as handled before attaching the forwarding
    // reaction so engine-level unhandled-rejection reporting does not race this internal hook.
    mark_promise_as_handled(
        &reader_closed_promise,
        js_engine::boa::context_as_ec(context),
    )
    .map_err(boa_engine::JsError::from_opaque)?;

    let (cancel_promise, cancel_resolvers) = JsPromise::new_pending(context);

    // Step 4: "Let reading be false."
    // Step 5: "Let readAgain be false."
    // Step 6: "Let canceled1 be false."
    // Step 7: "Let canceled2 be false."
    // Step 8: "Let reason1 be undefined."
    // Step 9: "Let reason2 be undefined."
    // Step 10: "Let branch1 be undefined."
    // Step 11: "Let branch2 be undefined."
    let tee_state = gc_cell_new(TeeState {
        source_stream: stream,
        reader,
        branch1: None,
        branch2: None,
        cancel_promise: cancel_promise.into(),
        cancel_resolvers,
        reading: false,
        read_again: false,
        canceled1: false,
        canceled2: false,
        reason1: JsValue::undefined(),
        reason2: JsValue::undefined(),
    });

    // Step 16: "Let startAlgorithm be an algorithm that returns undefined."
    // Step 17: "Set branch1 to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancel1Algorithm)."
    let (branch1, branch1_object) = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableStreamDefaultTee {
            tee_state: tee_state.clone(),
            clone_for_branch2,
        },
        CancelAlgorithm::ReadableStreamDefaultTeeBranch1(tee_state.clone()),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        context,
    )?;

    // Step 18: "Set branch2 to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancel2Algorithm)."
    let (branch2, branch2_object) = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableStreamDefaultTee {
            tee_state: tee_state.clone(),
            clone_for_branch2,
        },
        CancelAlgorithm::ReadableStreamDefaultTeeBranch2(tee_state.clone()),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        context,
    )?;

    {
        let mut tee_state = tee_state.borrow_mut();
        tee_state.branch1 = Some(branch1.clone());
        tee_state.branch2 = Some(branch2.clone());
    }

    // Step 19: "Upon rejection of reader.[[closedPromise]] with reason r,"
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args: &[JsValue], tee_state: &GcCell<TeeState>, context| {
            let error = args.get_or_undefined(0).clone();
            let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
                let tee_state = tee_state.borrow();
                (
                    tee_state.branch1.clone(),
                    tee_state.branch2.clone(),
                    tee_state.canceled1,
                    tee_state.canceled2,
                    tee_state.cancel_resolvers.clone(),
                )
            };

            // Step 19.1: "Perform ! ReadableStreamDefaultControllerError(branch1.[[controller]], r)."
            if let Some(branch1) = branch1.as_ref() {
                if let Err(error) = default_tee_error_branch(branch1, error.clone(), context) {
                    error!("[readable-stream] default tee error branch1 failed: {error}");
                }
            }

            // Step 19.2: "Perform ! ReadableStreamDefaultControllerError(branch2.[[controller]], r)."
            if let Some(branch2) = branch2.as_ref() {
                if let Err(error) = default_tee_error_branch(branch2, error, context) {
                    error!("[readable-stream] default tee error branch2 failed: {error}");
                }
            }

            // Step 19.3: "If canceled1 is false or canceled2 is false, resolve cancelPromise with undefined."
            if !canceled1 || !canceled2 {
                if let Err(error) = cancel_resolvers.resolve.call(
                    &JsValue::undefined(),
                    &[JsValue::undefined()],
                    context,
                ) {
                    error!("[readable-stream] failed to resolve cancel promise: {error}");
                }
            }

            Ok(JsValue::undefined())
        },
        tee_state,
    )
    .to_js_function(context.realm());
    let forward_error: JsObject = JsPromise::from_object(reader_closed_promise)?
        .catch(on_rejected, context)?
        .into();
    mark_promise_as_handled(&forward_error, js_engine::boa::context_as_ec(context))
        .map_err(boa_engine::JsError::from_opaque)?;

    // Step 20: "Return « branch1, branch2 »."
    Ok(ReadableStreamTeeBranches {
        _branch1: branch1,
        branch1_object,
        _branch2: branch2,
        branch2_object,
    })
}

fn structured_clone_value(value: JsValue, context: &mut Context) -> JsResult<JsValue> {
    let structured_clone = context
        .global_object()
        .get(js_string!("structuredClone"), context)?
        .as_object()
        .and_then(JsFunction::from_object)
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("structuredClone is not available on the global object")
        })?;
    structured_clone.call(&JsValue::undefined(), &[value], context)
}

fn default_tee_enqueue_to_branch(
    branch: &ReadableStream,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    let Some(controller) = branch
        .controller_slot()
        .map(|controller| controller.as_default_controller())
    else {
        return Ok(());
    };
    crate::js::completion_to_js_result(
        controller.enqueue(chunk, js_engine::boa::context_as_ec(context)),
    )
}

fn default_tee_close_branch(branch: &ReadableStream, context: &mut Context) -> JsResult<()> {
    let Some(controller) = branch
        .controller_slot()
        .map(|controller| controller.as_default_controller())
    else {
        return Ok(());
    };
    crate::js::completion_to_js_result(controller.close(js_engine::boa::context_as_ec(context)))
}

fn default_tee_error_branch(
    branch: &ReadableStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    let Some(controller) = branch
        .controller_slot()
        .map(|controller| controller.as_default_controller())
    else {
        return Ok(());
    };
    crate::js::completion_to_js_result(
        controller.error(error, js_engine::boa::context_as_ec(context)),
    )
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_pull_algorithm(
    tee_state: GcCell<TeeState>,
    clone_for_branch2: bool,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 13.1: "If reading is true,"
    {
        let mut tee_state = tee_state.borrow_mut();
        if tee_state.reading {
            // Step 13.1.1: "Set readAgain to true."
            tee_state.read_again = true;

            // Step 13.1.2: "Return a promise resolved with undefined."
            return Ok(JsValue::undefined());
        }

        // Step 13.2: "Set reading to true."
        tee_state.reading = true;
    }

    // Step 13.3: "Let readRequest be a read request with the following items:"
    let read_request = ReadRequest::ReadableStreamDefaultTee {
        tee_state: tee_state.clone(),
        clone_for_branch2,
    };
    let reader = tee_state.borrow().reader.clone();

    // Step 13.4: "Perform ! ReadableStreamDefaultReaderRead(reader, readRequest)."
    if let Err(error) = crate::js::completion_to_js_result(
        reader.read_with_request(read_request, js_engine::boa::context_as_ec(context)),
    ) {
        tee_state.borrow_mut().reading = false;
        return Err(error);
    }

    // Step 13.5: "Return a promise resolved with undefined."
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_read_request_chunk_steps(
    tee_state: GcCell<TeeState>,
    clone_for_branch2: bool,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    queue_internal_stream_microtask(
        move |context| {
            // Step 13.3 chunk steps 1.1: "Set readAgain to false."
            tee_state.borrow_mut().read_again = false;

            // Step 13.3 chunk steps 1.2: "Let chunk1 and chunk2 be chunk."
            let chunk1 = chunk.clone();
            let mut chunk2 = chunk;
            let (source_stream, branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
                let tee_state = tee_state.borrow();
                (
                    tee_state.source_stream.clone(),
                    tee_state.branch1.clone(),
                    tee_state.branch2.clone(),
                    tee_state.canceled1,
                    tee_state.canceled2,
                    tee_state.cancel_resolvers.clone(),
                )
            };

            // Step 13.3 chunk steps 1.3: "If canceled2 is false and cloneForBranch2 is true,"
            if !canceled2 && clone_for_branch2 {
                // Step 13.3 chunk steps 1.3.1: "Let cloneResult be StructuredClone(chunk2)."
                match structured_clone_value(chunk2.clone(), context) {
                    Ok(cloned_chunk) => {
                        // Step 13.3 chunk steps 1.3.3: "Otherwise, set chunk2 to cloneResult.[[Value]]."
                        chunk2 = cloned_chunk;
                    }
                    Err(error) => {
                        let error = error.into_opaque(context)?;

                        // Step 13.3 chunk steps 1.3.2.1: "Perform ! ReadableStreamDefaultControllerError(branch1.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch1) = branch1.as_ref() {
                            if let Err(error) =
                                default_tee_error_branch(branch1, error.clone(), context)
                            {
                                error!(
                                    "[readable-stream] default tee error branch1 (chunk) failed: {error}"
                                );
                            }
                        }

                        // Step 13.3 chunk steps 1.3.2.2: "Perform ! ReadableStreamDefaultControllerError(branch2.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch2) = branch2.as_ref() {
                            if let Err(error) =
                                default_tee_error_branch(branch2, error.clone(), context)
                            {
                                error!(
                                    "[readable-stream] default tee error branch2 (chunk) failed: {error}"
                                );
                            }
                        }

                        // Step 13.3 chunk steps 1.3.2.3: "Resolve cancelPromise with ! ReadableStreamCancel(stream, cloneResult.[[Value]])."
                        let cancel_result = readable_stream_cancel(source_stream, error, context)?;
                        cancel_resolvers.resolve.call(
                            &JsValue::undefined(),
                            &[JsValue::from(cancel_result)],
                            context,
                        )?;

                        // Step 13.3 chunk steps 1.3.2.4: "Return."
                        return Ok(());
                    }
                }
            }

            // Step 13.3 chunk steps 1.4: "If canceled1 is false, perform ! ReadableStreamDefaultControllerEnqueue(branch1.[[controller]], chunk1)."
            if !canceled1 {
                if let Some(branch1) = branch1.as_ref() {
                    default_tee_enqueue_to_branch(branch1, chunk1, context)?;
                }
            }

            // Step 13.3 chunk steps 1.5: "If canceled2 is false, perform ! ReadableStreamDefaultControllerEnqueue(branch2.[[controller]], chunk2)."
            if !canceled2 {
                if let Some(branch2) = branch2.as_ref() {
                    default_tee_enqueue_to_branch(branch2, chunk2, context)?;
                }
            }

            // Step 13.3 chunk steps 1.6: "Set reading to false."
            // Step 13.3 chunk steps 1.7: "If readAgain is true, perform pullAlgorithm."
            let should_read_again = {
                let mut tee_state = tee_state.borrow_mut();
                tee_state.reading = false;
                let should_read_again = tee_state.read_again;
                tee_state.read_again = false;
                should_read_again
            };

            if should_read_again {
                let pull_promise = promise_from_completion(
                    readable_stream_default_tee_pull_algorithm(
                        tee_state.clone(),
                        clone_for_branch2,
                        context,
                    ),
                    js_engine::boa::context_as_ec(context),
                );
                mark_promise_as_handled(
                    &JsObject::from(pull_promise),
                    js_engine::boa::context_as_ec(context),
                )
                .map_err(boa_engine::JsError::from_opaque)?;
            }

            Ok(())
        },
        context,
    )
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_read_request_close_steps(
    tee_state: GcCell<TeeState>,
    context: &mut Context,
) -> JsResult<()> {
    let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
        let mut tee_state = tee_state.borrow_mut();

        // Step 13.3 close steps 1: "Set reading to false."
        tee_state.reading = false;
        (
            tee_state.branch1.clone(),
            tee_state.branch2.clone(),
            tee_state.canceled1,
            tee_state.canceled2,
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 13.3 close steps 2: "If canceled1 is false, perform ! ReadableStreamDefaultControllerClose(branch1.[[controller]])."
    if !canceled1 {
        if let Some(branch1) = branch1.as_ref() {
            default_tee_close_branch(branch1, context)?;
        }
    }

    // Step 13.3 close steps 3: "If canceled2 is false, perform ! ReadableStreamDefaultControllerClose(branch2.[[controller]])."
    if !canceled2 {
        if let Some(branch2) = branch2.as_ref() {
            default_tee_close_branch(branch2, context)?;
        }
    }

    // Step 13.3 close steps 4: "If canceled1 is false or canceled2 is false, resolve cancelPromise with undefined."
    if !canceled1 || !canceled2 {
        cancel_resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_read_request_error_steps(
    tee_state: GcCell<TeeState>,
    _context: &mut Context,
) -> JsResult<()> {
    // Step 13.3 error steps 1: "Set reading to false."
    tee_state.borrow_mut().reading = false;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_cancel1_algorithm(
    tee_state: GcCell<TeeState>,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (source_stream, cancel_promise, canceled2, reason1, reason2, cancel_resolvers) = {
        let mut tee_state = tee_state.borrow_mut();

        // Step 14.1: "Set canceled1 to true."
        tee_state.canceled1 = true;

        // Step 14.2: "Set reason1 to reason."
        tee_state.reason1 = reason;
        (
            tee_state.source_stream.clone(),
            tee_state.cancel_promise.clone(),
            tee_state.canceled2,
            tee_state.reason1.clone(),
            tee_state.reason2.clone(),
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 14.3: "If canceled2 is true,"
    if canceled2 {
        // Step 14.3.1: "Let compositeReason be ! CreateArrayFromList(« reason1, reason2 »)."
        let composite_reason =
            JsArray::from_iter([reason1, reason2].into_iter().map(JsValue::from), context);

        // Step 14.3.2: "Let cancelResult be ! ReadableStreamCancel(stream, compositeReason)."
        let cancel_result =
            readable_stream_cancel(source_stream, JsValue::from(composite_reason), context)?;

        // Step 14.3.3: "Resolve cancelPromise with cancelResult."
        cancel_resolvers.resolve.call(
            &JsValue::undefined(),
            &[JsValue::from(cancel_result)],
            context,
        )?;
    }

    // Step 14.4: "Return cancelPromise."
    Ok(JsValue::from(cancel_promise))
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_cancel2_algorithm(
    tee_state: GcCell<TeeState>,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (source_stream, cancel_promise, canceled1, reason1, reason2, cancel_resolvers) = {
        let mut tee_state = tee_state.borrow_mut();

        // Step 15.1: "Set canceled2 to true."
        tee_state.canceled2 = true;

        // Step 15.2: "Set reason2 to reason."
        tee_state.reason2 = reason;
        (
            tee_state.source_stream.clone(),
            tee_state.cancel_promise.clone(),
            tee_state.canceled1,
            tee_state.reason1.clone(),
            tee_state.reason2.clone(),
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 15.3: "If canceled1 is true,"
    if canceled1 {
        // Step 15.3.1: "Let compositeReason be ! CreateArrayFromList(« reason1, reason2 »)."
        let composite_reason =
            JsArray::from_iter([reason1, reason2].into_iter().map(JsValue::from), context);

        // Step 15.3.2: "Let cancelResult be ! ReadableStreamCancel(stream, compositeReason)."
        let cancel_result =
            readable_stream_cancel(source_stream, JsValue::from(composite_reason), context)?;

        // Step 15.3.3: "Resolve cancelPromise with cancelResult."
        cancel_resolvers.resolve.call(
            &JsValue::undefined(),
            &[JsValue::from(cancel_result)],
            context,
        )?;
    }

    // Step 15.4: "Return cancelPromise."
    Ok(JsValue::from(cancel_promise))
}

#[derive(Clone, Trace, Finalize)]
enum ReadableStreamFromIteratorKind {
    Async,
    Sync,
}

#[derive(Clone, Trace, Finalize)]
struct ReadableStreamFromIteratorRecord {
    iterator: JsObject,
    next_method: JsObject,
    kind: ReadableStreamFromIteratorKind,
}

impl ReadableStreamFromIteratorRecord {
    fn next_result_promise(&self, context: &mut Context) -> JsResult<JsObject> {
        let next_result =
            self.next_method
                .call(&JsValue::from(self.iterator.clone()), &[], context)?;

        match self.kind {
            ReadableStreamFromIteratorKind::Async => crate::js::completion_to_js_result(
                promise_from_value(next_result, js_engine::boa::context_as_ec(context)),
            ),
            ReadableStreamFromIteratorKind::Sync => {
                promise_from_sync_iterator_result(next_result, context)
            }
        }
    }

    fn return_result_promise(
        &self,
        reason: JsValue,
        context: &mut Context,
    ) -> JsResult<Option<JsObject>> {
        let return_method = get_optional_callable_method_value(
            self.iterator.get(js_string!("return"), context)?,
            "ReadableStream.from() iterator.return",
        )?;
        let Some(return_method) = return_method else {
            return Ok(None);
        };

        let return_result =
            return_method.call(&JsValue::from(self.iterator.clone()), &[reason], context)?;
        let return_promise = match self.kind {
            ReadableStreamFromIteratorKind::Async => {
                promise_from_value(return_result, js_engine::boa::context_as_ec(context))
                    .map_err(boa_engine::JsError::from_opaque)?
            }
            ReadableStreamFromIteratorKind::Sync => {
                promise_from_sync_iterator_result(return_result, context)?
            }
        };
        Ok(Some(return_promise))
    }
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadableStreamFromIterableState {
    iterator_record: ReadableStreamFromIteratorRecord,
    stream: GcCell<Option<ReadableStream>>,
}

impl ReadableStreamFromIterableState {
    fn new(iterator_record: ReadableStreamFromIteratorRecord) -> Self {
        Self {
            iterator_record,
            stream: gc_cell_new(None),
        }
    }

    fn set_stream(&self, stream: ReadableStream) {
        *self.stream.borrow_mut() = Some(stream);
    }

    fn stream(&self) -> JsResult<ReadableStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream.from() is missing its stream")
                .into()
        })
    }
}
/// <https://streams.spec.whatwg.org/#rs-constructor>
pub(crate) fn construct_readable_stream(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ReadableStream> {
    let mut stream = ReadableStream::new();

    // Step 1: "If underlyingSource is missing, set it to undefined."
    let underlying_source = if args.is_empty() {
        JsValue::undefined()
    } else {
        args[0].clone()
    };

    // Step 2: "Let underlyingSourceDict be underlyingSource, converted to an IDL value of type UnderlyingSource."
    // Note: The implementation keeps the original JavaScript object so it can invoke the underlying source callbacks directly.
    let underlying_source_object = if underlying_source.is_undefined() {
        None
    } else {
        Some(underlying_source.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream underlyingSource must be an object")
        })?)
    };

    // Step 3: "Perform ! InitializeReadableStream(this)."
    // Note: The backing struct is returned from the data constructor, after which Boa wraps it
    // in the newly created JsObject.
    stream.initialize_readable_stream();

    let strategy = args.get_or_undefined(1).clone();
    match underlying_source_type(underlying_source_object.as_ref(), context)?.as_deref() {
        Some("bytes") => {
            // Step 4.1: "If strategy[\"size\"] exists, throw a RangeError exception."
            if strategy_has_size(&strategy, context)? {
                return Err(JsNativeError::range()
                    .with_message("a byte stream strategy cannot include a size function")
                    .into());
            }

            // Step 4.2: "Let highWaterMark be ? ExtractHighWaterMark(strategy, 0)."
            let high_water_mark =
                extract_high_water_mark(&strategy, 0.0, js_engine::boa::context_as_ec(context))
                    .map_err(JsError::from_opaque)?;

            // Step 4.3: "Perform ? SetUpReadableByteStreamControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark)."
            crate::js::completion_to_js_result(
                set_up_readable_byte_stream_controller_from_underlying_source(
                    stream.clone(),
                    underlying_source_object,
                    high_water_mark,
                    js_engine::boa::context_as_ec(context),
                ),
            )?;
            return Ok(stream);
        }
        Some(_) => {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream underlyingSource.type must be \"bytes\" when present")
                .into());
        }
        None => {}
    }

    // Step 5.1: "Assert: underlyingSourceDict[\"type\"] does not exist."
    debug_assert!(underlying_source_type(underlying_source_object.as_ref(), context)?.is_none());

    // Step 5.2: "Let sizeAlgorithm be ! ExtractSizeAlgorithm(strategy)."
    let size_algorithm = extract_size_algorithm(&strategy, js_engine::boa::context_as_ec(context))
        .map_err(JsError::from_opaque)?;

    // Step 5.3: "Let highWaterMark be ? ExtractHighWaterMark(strategy, 1)."
    let high_water_mark =
        extract_high_water_mark(&strategy, 1.0, js_engine::boa::context_as_ec(context))
            .map_err(JsError::from_opaque)?;

    // Step 5.4: "Perform ? SetUpReadableStreamDefaultControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark, sizeAlgorithm)."
    crate::js::completion_to_js_result(
        set_up_readable_stream_default_controller_from_underlying_source(
            stream.clone(),
            underlying_source_object,
            high_water_mark,
            size_algorithm,
            js_engine::boa::context_as_ec(context),
        ),
    )?;

    Ok(stream)
}

/// <https://streams.spec.whatwg.org/#create-readable-stream>
pub(crate) fn create_readable_stream(
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: Option<f64>,
    size_algorithm: Option<SizeAlgorithm>,
    context: &mut Context,
) -> JsResult<(ReadableStream, JsObject)> {
    // Step 1: "If highWaterMark was not passed, set it to 1."
    let high_water_mark = high_water_mark.unwrap_or(1.0);

    // Step 2: "If sizeAlgorithm was not passed, set it to an algorithm that returns 1."
    let size_algorithm = size_algorithm.unwrap_or(SizeAlgorithm::ReturnOne);

    // Step 3: "Assert: ! IsNonNegativeNumber(highWaterMark) is true."
    debug_assert!(high_water_mark >= 0.0 && !high_water_mark.is_nan());

    // Step 4: "Let stream be a new ReadableStream."
    let (mut stream, stream_object) = create_readable_stream_object(context)?;

    // Step 5: "Perform ! InitializeReadableStream(stream)."
    stream.initialize_readable_stream();

    // Step 6: "Let controller be a new ReadableStreamDefaultController."
    let controller = super::ReadableStreamDefaultController::new();
    let ec_ref = js_engine::boa::context_as_ec(context);
    let controller_object = create_interface_instance::<
        crate::js::Types,
        super::ReadableStreamDefaultController,
    >(controller.clone(), ec_ref)
    .map_err(JsError::from_opaque)?;

    // Step 7: "Perform ? SetUpReadableStreamDefaultController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, highWaterMark, sizeAlgorithm)."
    crate::js::completion_to_js_result(set_up_readable_stream_default_controller(
        stream.clone(),
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        high_water_mark,
        size_algorithm,
        js_engine::boa::context_as_ec(context),
    ))?;

    // Step 8: "Return stream."
    Ok((stream, stream_object))
}

pub(crate) fn construct_readable_stream_ec(
    new_target: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ReadableStream, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    construct_readable_stream(new_target, args, ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))
}

fn create_readable_stream_object(context: &mut Context) -> JsResult<(ReadableStream, JsObject)> {
    let ec_ref = js_engine::boa::context_as_ec(context);
    let stream = ReadableStream::new();
    let stream_object: JsObject =
        create_interface_instance::<crate::js::Types, ReadableStream>(stream.clone(), ec_ref)
            .map_err(JsError::from_opaque)?
            .into();
    Ok((stream, stream_object))
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-createreadablebytestream>
fn create_readable_byte_stream(
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    context: &mut Context,
) -> JsResult<(ReadableStream, JsObject)> {
    // Step 1: "Let stream be a new ReadableStream."
    let (mut stream, stream_object) = create_readable_stream_object(context)?;

    // Step 2: "Perform ! InitializeReadableStream(stream)."
    stream.initialize_readable_stream();

    // Step 3: "Let controller be a new ReadableByteStreamController."
    let controller = ReadableByteStreamController::new();
    let controller_object = create_interface_instance::<
        crate::js::Types,
        ReadableByteStreamController,
    >(controller.clone(), js_engine::boa::context_as_ec(context))
    .map_err(JsError::from_opaque)?;

    // Step 4: "Perform ? SetUpReadableByteStreamController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, 0, undefined)."
    crate::js::completion_to_js_result(super::set_up_readable_byte_stream_controller(
        stream.clone(),
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        0.0,
        None,
        js_engine::boa::context_as_ec(context),
    ))?;

    // Step 5: "Return stream."
    Ok((stream, stream_object))
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable(
    async_iterable: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let stream be undefined."
    let state = ReadableStreamFromIterableState::new(get_readable_stream_from_iterator_record(
        async_iterable,
        context,
    )?);

    // Step 2: "Let iteratorRecord be ? GetIterator(asyncIterable, async)."
    // Note: `get_readable_stream_from_iterator_record()` normalizes async iterators and the
    // async-from-sync fallback into a record whose `next_result_promise()` matches the spec.

    // Step 3: "Let startAlgorithm be an algorithm that returns undefined."
    let start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 4: "Let pullAlgorithm be the following steps:"
    let pull_algorithm = PullAlgorithm::ReadableStreamFromIterable(state.clone());

    // Step 5: "Let cancelAlgorithm be the following steps, given reason:"
    let cancel_algorithm = CancelAlgorithm::ReadableStreamFromIterable(state.clone());

    // Step 6: "Set stream to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancelAlgorithm, 0)."
    let (stream, stream_object) = create_readable_stream(
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        Some(0.0),
        None,
        context,
    )?;
    state.set_stream(stream);

    // Step 7: "Return stream."
    Ok(stream_object)
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable_ec(
    async_iterable: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    readable_stream_from_iterable(async_iterable, ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable_pull_algorithm(
    state: ReadableStreamFromIterableState,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 4.1: "Let nextResult be IteratorNext(iteratorRecord)."
    let next_result = state.iterator_record.next_result_promise(context);

    // Step 4.2: "If nextResult is an abrupt completion, return a promise rejected with nextResult.[[Value]]."
    let next_promise = match next_result {
        Ok(next_promise) => next_promise,
        Err(error) => {
            return crate::js::completion_to_js_result(rejected_promise(
                error.into_opaque(context)?,
                js_engine::boa::context_as_ec(context),
            ));
        }
    };

    // Step 4.3: "Let nextPromise be a promise resolved with nextResult.[[Value]]."
    // Note: `next_result_promise()` already returns the promise produced by `PromiseResolve`
    // and applies async-from-sync iterator adaptation for sync iterables.

    // Step 4.4: "Return the result of reacting to nextPromise with the following fulfillment steps, given iterResult:"
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &ReadableStreamFromIterableState, context| {
            let iter_result = args.get_or_undefined(0).clone();

            // Step 4.4.1: "If iterResult is not an Object, throw a TypeError."
            let iter_result_object = iter_result.as_object().ok_or_else(|| {
                JsNativeError::typ().with_message(
                    "ReadableStream.from() iterator next() must fulfill with an object",
                )
            })?;

            // Step 4.4.2: "Let done be ? IteratorComplete(iterResult)."
            let done = iter_result_object
                .get(js_string!("done"), context)?
                .to_boolean();

            let stream = state.stream()?;
            let controller = stream.controller_slot().ok_or_else(|| {
                JsNativeError::typ().with_message("ReadableStream.from() is missing its controller")
            })?;
            let controller = controller.as_default_controller();

            // Step 4.4.3: "If done is true:"
            if done {
                // Step 4.4.3.1: "Perform ! ReadableStreamDefaultControllerClose(stream.[[controller]])."
                crate::js::completion_to_js_result(
                    controller.close_steps(js_engine::boa::context_as_ec(context)),
                )?;
                return Ok(JsValue::undefined());
            }

            // Step 4.4.4.1: "Let value be ? IteratorValue(iterResult)."
            let value = iter_result_object.get(js_string!("value"), context)?;

            // Step 4.4.4.2: "Perform ! ReadableStreamDefaultControllerEnqueue(stream.[[controller]], value)."
            crate::js::completion_to_js_result(
                controller.enqueue_steps(value, js_engine::boa::context_as_ec(context)),
            )?;
            Ok(JsValue::undefined())
        },
        state,
    )
    .to_js_function(context.realm());

    Ok(JsPromise::from_object(next_promise)?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable_cancel_algorithm(
    state: ReadableStreamFromIterableState,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 5.1: "Let iterator be iteratorRecord.[[Iterator]]."
    // Step 5.2: "Let returnMethod be GetMethod(iterator, \"return\")."
    // Step 5.3: "If returnMethod is an abrupt completion, return a promise rejected with returnMethod.[[Value]]."
    // Step 5.4: "If returnMethod.[[Value]] is undefined, return a promise resolved with undefined."
    // Step 5.5: "Let returnResult be Call(returnMethod.[[Value]], iterator, « reason »)."
    // Step 5.6: "If returnResult is an abrupt completion, return a promise rejected with returnResult.[[Value]]."
    // Step 5.7: "Let returnPromise be a promise resolved with returnResult.[[Value]]."
    // Note: `return_result_promise()` folds those steps together and applies the async-from-sync
    // iterator adaptation required for sync iterables.
    let return_result = state.iterator_record.return_result_promise(reason, context);
    let return_promise = match return_result {
        Ok(Some(return_promise)) => return_promise,
        Ok(None) => {
            return crate::js::completion_to_js_result(resolved_promise(
                JsValue::undefined(),
                js_engine::boa::context_as_ec(context),
            ));
        }
        Err(error) => {
            return crate::js::completion_to_js_result(rejected_promise(
                error.into_opaque(context)?,
                js_engine::boa::context_as_ec(context),
            ));
        }
    };

    // Step 5.8: "Return the result of reacting to returnPromise with the following fulfillment steps, given iterResult:"
    let on_fulfilled = NativeFunction::from_fn_ptr(|_, args, _| {
        // Step 5.8.1: "If iterResult is not an Object, throw a TypeError."
        if args.get_or_undefined(0).as_object().is_none() {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.from() iterator return() must fulfill with an object")
                .into());
        }

        // Step 5.8.2: "Return undefined."
        Ok(JsValue::undefined())
    })
    .to_js_function(context.realm());

    Ok(JsPromise::from_object(return_promise)?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

fn get_readable_stream_from_iterator_record(
    async_iterable: JsValue,
    context: &mut Context,
) -> JsResult<ReadableStreamFromIteratorRecord> {
    let iterable_object = async_iterable.to_object(context)?;

    if let Some(async_iterator_method) = get_optional_callable_method_value(
        iterable_object.get(JsSymbol::async_iterator(), context)?,
        "ReadableStream.from() iterable[@@asyncIterator]",
    )? {
        let iterator = async_iterator_method
            .call(&async_iterable, &[], context)?
            .as_object()
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("ReadableStream.from() @@asyncIterator must return an object")
            })?
            .clone();
        let next_method = get_required_callable_method(
            &iterator,
            "next",
            "ReadableStream.from() iterator.next must be callable",
            context,
        )?;
        return Ok(ReadableStreamFromIteratorRecord {
            iterator,
            next_method,
            kind: ReadableStreamFromIteratorKind::Async,
        });
    }

    let iterator_method = get_optional_callable_method_value(
        iterable_object.get(JsSymbol::iterator(), context)?,
        "ReadableStream.from() iterable[@@iterator]",
    )?
    .ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStream.from() requires an async iterable or iterable")
    })?;
    let iterator = iterator_method
        .call(&async_iterable, &[], context)?
        .as_object()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream.from() @@iterator must return an object")
        })?
        .clone();
    let next_method = get_required_callable_method(
        &iterator,
        "next",
        "ReadableStream.from() iterator.next must be callable",
        context,
    )?;
    Ok(ReadableStreamFromIteratorRecord {
        iterator,
        next_method,
        kind: ReadableStreamFromIteratorKind::Sync,
    })
}

fn promise_from_sync_iterator_result(
    iter_result: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    let iter_result_object = match iter_result.as_object() {
        Some(iter_result_object) => iter_result_object.clone(),
        None => {
            let js_error: JsError = JsNativeError::typ()
                .with_message("ReadableStream.from() iterator result must be an object")
                .into();
            return crate::js::completion_to_js_result(rejected_promise(
                js_error.into_opaque(context)?,
                js_engine::boa::context_as_ec(context),
            ));
        }
    };

    let done = match iter_result_object.get(js_string!("done"), context) {
        Ok(done) => done.to_boolean(),
        Err(error) => {
            return crate::js::completion_to_js_result(rejected_promise(
                error.into_opaque(context)?,
                js_engine::boa::context_as_ec(context),
            ));
        }
    };
    let value = match iter_result_object.get(js_string!("value"), context) {
        Ok(value) => value,
        Err(error) => {
            return crate::js::completion_to_js_result(rejected_promise(
                error.into_opaque(context)?,
                js_engine::boa::context_as_ec(context),
            ));
        }
    };
    let value_promise = match promise_from_value(value, js_engine::boa::context_as_ec(context)) {
        Ok(value_promise) => value_promise,
        Err(error) => {
            return crate::js::completion_to_js_result(rejected_promise(
                error,
                js_engine::boa::context_as_ec(context),
            ));
        }
    };
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, args, done: &bool, context| {
            Ok(create_iter_result_object(
                args.get_or_undefined(0).clone(),
                *done,
                context,
            ))
        },
        done,
    )
    .to_js_function(context.realm());

    Ok(JsPromise::from_object(value_promise)?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

fn get_optional_callable_method_value(
    value: JsValue,
    description: &'static str,
) -> JsResult<Option<JsObject>> {
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }

    let method = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message(format!("{description} must be callable when provided"))
    })?;
    if !method.is_callable() {
        return Err(JsNativeError::typ()
            .with_message(format!("{description} must be callable when provided"))
            .into());
    }

    Ok(Some(method.clone()))
}

fn get_required_callable_method(
    object: &JsObject,
    property: &'static str,
    message: &'static str,
    context: &mut Context,
) -> JsResult<JsObject> {
    get_optional_callable_method_value(object.get(js_string!(property), context)?, message)?
        .ok_or_else(|| JsNativeError::typ().with_message(message).into())
}
pub(crate) fn with_readable_stream_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStream) -> R,
) -> JsResult<R> {
    let stream = object
        .downcast_ref::<ReadableStream>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not a ReadableStream"))?;
    Ok(f(&stream))
}

pub(crate) fn with_readable_stream_ref_ec<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableStream) -> R,
) -> Completion<R, crate::js::Types> {
    let stream_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStream>());
    let stream = match stream_ref {
        Some(s) => s,
        None => return Err(ec.new_type_error("object is not a ReadableStream")),
    };
    Ok(f(stream))
}

/// <https://streams.spec.whatwg.org/#readable-stream-cancel>
pub(crate) fn readable_stream_cancel(
    stream: ReadableStream,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    readable_stream_cancel_ec(stream, reason, js_engine::boa::context_as_ec(context))
        .map_err(|e| JsError::from_opaque(e))
}

/// Generic entry point for <https://streams.spec.whatwg.org/#readable-stream-cancel>.
/// Returns `Completion` — the binding layer uses this directly without bridging.
pub(crate) fn readable_stream_cancel_ec(
    stream: ReadableStream,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Set stream.[[disturbed]] to true."
    stream.set_disturbed(true);

    // Step 2: "If stream.[[state]] is \"closed\", return a promise resolved with undefined."
    if stream.state() == ReadableStreamState::Closed {
        return resolved_promise(ec.value_undefined(), ec);
    }

    // Step 3: "If stream.[[state]] is \"errored\", return a promise rejected with stream.[[storedError]]."
    if stream.state() == ReadableStreamState::Errored {
        return rejected_promise(stream.stored_error(), ec);
    }

    // Step 4: "Perform ! ReadableStreamClose(stream)."
    readable_stream_close_ec(stream.clone(), ec)?;

    // Step 5: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 6: "If reader is not undefined and reader implements ReadableStreamBYOBReader,"
    // Note: The byte-stream controller's cancel steps own any pending BYOB read-into cleanup.
    let _ = reader;

    // Step 7: "Let sourceCancelPromise be ! stream.[[controller]].[[CancelSteps]](reason)."
    let controller = stream
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
    let source_cancel_promise = controller.cancel_steps(reason, ec)?;

    // Step 8: "Return the result of reacting to sourceCancelPromise with a fulfillment step that returns undefined."
    // This implements <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
    // by calling Web IDL's transform_promise_to_undefined which wraps PerformPromiseThen.
    transform_promise_to_undefined(&source_cancel_promise, ec)
}

/// <https://streams.spec.whatwg.org/#readable-stream-close>
pub(crate) fn readable_stream_close(stream: ReadableStream, context: &mut Context) -> JsResult<()> {
    readable_stream_close_ec(stream, js_engine::boa::context_as_ec(context))
        .map_err(|e| JsError::from_opaque(e))
}

/// Generic entry point for <https://streams.spec.whatwg.org/#readable-stream-close>.
/// Returns `Completion` — the binding layer uses this directly without bridging.
pub(crate) fn readable_stream_close_ec(
    stream: ReadableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 2: "Set stream.[[state]] to \"closed\"."
    stream.set_state(ReadableStreamState::Closed);

    // Step 3: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 4: "If reader is undefined, return."
    let Some(reader) = reader else {
        return Ok(());
    };

    match &reader {
        ReadableStreamReader::Default(reader) => {
            // Step 5: "Resolve reader.[[closedPromise]] with undefined."
            // Note: ec_to_ctx — ResolvingFunctions::resolve.call requires Boa Context.
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[JsValue::undefined()], ctx)
                    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
                reader.set_closed_resolvers_slot_value(None);
            }

            // Step 6.1: "Let readRequests be reader.[[readRequests]]."
            let read_requests = reader.take_read_requests();

            // Step 6.2: "Set reader.[[readRequests]] to an empty list."
            // Note: `take_read_requests()` empties the list before the requests are processed.

            // Step 6.3: "For each readRequest of readRequests,"
            for read_request in read_requests {
                // Step 6.3.1: "Perform readRequest's close steps."
                read_request.close_steps(ec)?;
            }
        }
        ReadableStreamReader::BYOB(reader) => {
            // Note: ec_to_ctx — ResolvingFunctions::resolve.call requires Boa Context.
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[JsValue::undefined()], ctx)
                    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
                reader.set_closed_resolvers_slot_value(None);
            }
        }
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-error>
pub(crate) fn readable_stream_error(
    stream: ReadableStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 2: "Set stream.[[state]] to \"errored\"."
    stream.set_state(ReadableStreamState::Errored);

    // Step 3: "Set stream.[[storedError]] to e."
    stream.set_stored_error(error.clone());

    // Step 4: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 5: "If reader is undefined, return."
    let Some(reader) = reader else {
        return Ok(());
    };

    match &reader {
        ReadableStreamReader::Default(reader) => {
            // Step 7: "Set reader.[[closedPromise]].[[PromiseIsHandled]] to true."
            if let Some(closed_promise) = reader.closed_promise_slot_value() {
                mark_promise_as_handled(&closed_promise, js_engine::boa::context_as_ec(context))
                    .map_err(boa_engine::JsError::from_opaque)?;
            }

            // Step 6: "Reject reader.[[closedPromise]] with e."
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[error.clone()], context)?;
                reader.set_closed_resolvers_slot_value(None);
            }

            // Step 8.1: "Perform ! ReadableStreamDefaultReaderErrorReadRequests(reader, e)."
            crate::js::completion_to_js_result(readable_stream_default_reader_error_read_requests(
                reader.clone(),
                error,
                js_engine::boa::context_as_ec(context),
            ))
        }
        ReadableStreamReader::BYOB(reader) => {
            if let Some(closed_promise) = reader.closed_promise_slot_value() {
                mark_promise_as_handled(&closed_promise, js_engine::boa::context_as_ec(context))
                    .map_err(boa_engine::JsError::from_opaque)?;
            }

            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[error.clone()], context)?;
                reader.set_closed_resolvers_slot_value(None);
            }

            Ok(())
        }
    }
}

/// <https://streams.spec.whatwg.org/#readable-stream-add-read-request>
pub(crate) fn readable_stream_add_read_request(
    stream: ReadableStream,
    read_request: ReadRequest,
) -> JsResult<()> {
    // Step 1: "Assert: stream.[[reader]] implements ReadableStreamDefaultReader."
    let reader = stream
        .reader_slot()
        .and_then(|reader| reader.as_default_reader())
        .ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream is not locked to a default reader")
        })?;

    // Step 2: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 3: "Append readRequest to stream.[[reader]].[[readRequests]]."
    reader.push_read_request(read_request);
    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-fulfill-read-request>
pub(crate) fn readable_stream_fulfill_read_request(
    stream: ReadableStream,
    chunk: JsValue,
    done: bool,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Assert: ! ReadableStreamHasDefaultReader(stream) is true."
    let reader = stream
        .reader_slot()
        .and_then(|reader| reader.as_default_reader())
        .ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream is not locked to a default reader")
        })?;

    // Step 2: "Let reader be stream.[[reader]]."

    // Step 3: "Assert: reader.[[readRequests]] is not empty."
    debug_assert!(reader.read_requests_len() > 0);

    // Step 4: "Let readRequest be reader.[[readRequests]][0]."
    // Step 5: "Remove readRequest from reader.[[readRequests]]."
    let read_request = reader.shift_read_request().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream has no pending read request")
    })?;

    // Step 6: "If done is true, perform readRequest's close steps."
    if done {
        let ec: &mut dyn ExecutionContext<crate::js::Types> =
            js_engine::boa::context_as_ec(context);
        return crate::js::completion_to_js_result(read_request.close_steps(ec));
    }

    // Step 7: "Otherwise, perform readRequest's chunk steps, given chunk."
    let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
    crate::js::completion_to_js_result(read_request.chunk_steps(chunk, ec))
}

/// <https://streams.spec.whatwg.org/#readable-stream-get-num-read-requests>
pub(crate) fn readable_stream_get_num_read_requests(stream: ReadableStream) -> usize {
    // Step 1: "Assert: ! ReadableStreamHasDefaultReader(stream) is true."
    debug_assert!(readable_stream_has_default_reader(&stream));

    // Step 2: "Return stream.[[reader]].[[readRequests]]'s size."
    stream
        .reader_slot()
        .and_then(|reader| reader.as_default_reader())
        .map(|reader| reader.read_requests_len())
        .unwrap_or(0)
}

/// <https://streams.spec.whatwg.org/#readable-stream-has-default-reader>
pub(crate) fn readable_stream_has_default_reader(stream: &ReadableStream) -> bool {
    // Step 1: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 2: "If reader is undefined, return false."
    let Some(reader) = reader else {
        return false;
    };

    // Step 3: "If reader implements ReadableStreamDefaultReader, return true."
    if reader.is_default_reader() {
        return true;
    }

    // Step 4: "Return false."
    false
}

// ---- ByteTeeState ---------------------------------------------------------

/// Internal state for ReadableByteStreamTee algorithm, maintained across pull and cancel operations.
/// See `readable_byte_stream_tee` for the algorithm using this state.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct ByteTeeState {
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    source_stream: ReadableStream,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    reader: ReadableStreamReader,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    branch1: Option<ReadableStream>,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    branch2: Option<ReadableStream>,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    cancel_promise: JsObject,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    cancel_resolvers: boa_engine::builtins::promise::ResolvingFunctions,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    reading: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    read_again_for_branch1: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    read_again_for_branch2: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    canceled1: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    canceled2: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    reason1: JsValue,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    reason2: JsValue,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    reader_generation: u64,
}

// ---- helpers used inside the byte tee algorithms --------------------------

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_enqueue_to_branch(
    branch: &ReadableStream,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step helper: "Perform ! ReadableByteStreamControllerEnqueue(branchX.[[controller]], chunkX)."
    let Some(controller) = branch
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
    else {
        return Ok(());
    };
    let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
    crate::js::completion_to_js_result(controller.enqueue(chunk, ec))
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_error_branch(
    branch: &ReadableStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step helper: "Perform ! ReadableByteStreamControllerError(branchX.[[controller]], r)."
    let Some(controller) = branch
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
    else {
        return Ok(());
    };
    let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
    crate::js::completion_to_js_result(controller.error(error, ec))
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_close_branch(branch: &ReadableStream, context: &mut Context) -> JsResult<()> {
    // Step helper: "Perform ! ReadableByteStreamControllerClose(branchX.[[controller]])."
    let Some(controller) = branch
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
    else {
        return Ok(());
    };
    let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
    crate::js::completion_to_js_result(controller.close(ec))
}

/// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-has-pending-pull-intos>
fn byte_tee_pending_pull_into_controller(
    branch: &ReadableStream,
) -> Option<ReadableByteStreamController> {
    let controller = branch.controller_slot()?.as_byte_controller()?;
    if controller.pending_pull_intos_len() > 0 {
        Some(controller)
    } else {
        None
    }
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_forward_reader_error(
    reader_object: &JsObject,
    tee_state: &GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<()> {
    let closed_promise = if let Ok(closed) = reader_object.get(js_string!("closed"), context) {
        closed
            .as_object()
            .and_then(|o| JsPromise::from_object(o.clone()).ok())
    } else {
        None
    };
    let Some(closed_promise) = closed_promise else {
        return Ok(());
    };

    // Step helper: "Let thisReader be reader" for the forwardReaderError closure.
    let generation_at_attach = tee_state.borrow().reader_generation;
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args: &[JsValue], captures: &(u64, GcCell<ByteTeeState>), context| {
            let (captured_generation, tee_state) = captures;
            // Step helper: "If thisReader is not reader, return."
            if tee_state.borrow().reader_generation != *captured_generation {
                return Ok(JsValue::undefined());
            }
            let error = args.get_or_undefined(0).clone();
            let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
                let tee = tee_state.borrow();
                (
                    tee.branch1.clone(),
                    tee.branch2.clone(),
                    tee.canceled1,
                    tee.canceled2,
                    tee.cancel_resolvers.clone(),
                )
            };
            if let Some(ref branch1) = branch1 {
                if let Err(error) = byte_tee_error_branch(branch1, error.clone(), context) {
                    error!("[readable-stream] byte tee error branch1 failed: {error}");
                }
            }
            if let Some(ref branch2) = branch2 {
                if let Err(error) = byte_tee_error_branch(branch2, error, context) {
                    error!("[readable-stream] byte tee error branch2 failed: {error}");
                }
            }
            if !canceled1 || !canceled2 {
                cancel_resolvers.resolve.call(
                    &JsValue::undefined(),
                    &[JsValue::undefined()],
                    context,
                )?;
            }
            Ok(JsValue::undefined())
        },
        (generation_at_attach, tee_state.clone()),
    )
    .to_js_function(context.realm());
    let _ = closed_promise.catch(on_rejected, context)?;
    Ok(())
}

fn byte_tee_ignore_pull_completion(
    completion: JsResult<JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    let promise = promise_from_completion(completion, js_engine::boa::context_as_ec(context));
    mark_promise_as_handled(
        &JsObject::from(promise),
        js_engine::boa::context_as_ec(context),
    )
    .map_err(boa_engine::JsError::from_opaque)
}

fn byte_tee_switch_to_default_reader(
    tee_state: &GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<()> {
    if !matches!(tee_state.borrow().reader, ReadableStreamReader::BYOB(_)) {
        return Ok(());
    }

    let (old_reader, source_stream) = {
        let tee = tee_state.borrow();
        (
            tee.reader.as_byob_reader().unwrap(),
            tee.source_stream.clone(),
        )
    };
    tee_state.borrow_mut().reader_generation += 1;
    crate::js::completion_to_js_result(readable_stream_byob_reader_release(
        old_reader,
        js_engine::boa::context_as_ec(context),
    ))?;
    let new_reader_object =
        crate::js::completion_to_js_result(acquire_readable_stream_default_reader(
            source_stream,
            js_engine::boa::context_as_ec(context),
        ))?;
    let new_reader = with_readable_stream_default_reader_ref(&new_reader_object, |r| r.clone())?;
    tee_state.borrow_mut().reader = ReadableStreamReader::Default(new_reader);
    byte_tee_forward_reader_error(&new_reader_object, tee_state, context)
}

fn byte_tee_switch_to_byob_reader(
    tee_state: &GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<()> {
    if !matches!(tee_state.borrow().reader, ReadableStreamReader::Default(_)) {
        return Ok(());
    }

    let (old_reader, source_stream) = {
        let tee = tee_state.borrow();
        (
            tee.reader.as_default_reader().unwrap(),
            tee.source_stream.clone(),
        )
    };
    tee_state.borrow_mut().reader_generation += 1;
    crate::js::completion_to_js_result(readable_stream_default_reader_release(
        old_reader,
        js_engine::boa::context_as_ec(context),
    ))?;
    let new_reader_object = crate::js::completion_to_js_result(
        acquire_readable_stream_byob_reader(source_stream, js_engine::boa::context_as_ec(context)),
    )?;
    let new_reader = with_readable_stream_byob_reader_ref(&new_reader_object, |r| r.clone())?;
    tee_state.borrow_mut().reader = ReadableStreamReader::BYOB(new_reader);
    byte_tee_forward_reader_error(&new_reader_object, tee_state, context)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_default_reader_chunk_steps(
    tee_state: GcCell<ByteTeeState>,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    queue_internal_stream_microtask(
        move |context| {
            // Step 18.2 chunk steps 1.1: "Set readAgainForBranch1 to false."
            // Step 18.2 chunk steps 1.2: "Set readAgainForBranch2 to false."
            {
                let mut tee = tee_state.borrow_mut();
                tee.read_again_for_branch1 = false;
                tee.read_again_for_branch2 = false;
            }

            // Step 18.2 chunk steps 1.3: "Let chunk1 and chunk2 be chunk."
            let chunk1 = chunk.clone();
            let mut chunk2 = chunk;
            let (branch1, branch2, canceled1, canceled2) = {
                let tee = tee_state.borrow();
                (
                    tee.branch1.clone(),
                    tee.branch2.clone(),
                    tee.canceled1,
                    tee.canceled2,
                )
            };

            // Step 18.2 chunk steps 1.4: "If canceled1 is false and canceled2 is false,"
            if !canceled1 && !canceled2 {
                // Step 18.2 chunk steps 1.4.1: "Let cloneResult be CloneAsUint8Array(chunk)."
                match clone_as_uint8_array(chunk1.clone(), js_engine::boa::context_as_ec(context)) {
                    Ok(cloned_chunk) => {
                        // Step 18.2 chunk steps 1.4.3: "Otherwise, set chunk2 to cloneResult.[[Value]]."
                        chunk2 = cloned_chunk;
                    }
                    Err(error) => {
                        // Step 18.2 chunk steps 1.4.2.1: "Perform ! ReadableByteStreamControllerError(branch1.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch1) = branch1.as_ref() {
                            if let Err(inner_error) =
                                byte_tee_error_branch(branch1, error.clone(), context)
                            {
                                error!(
                                    "[readable-stream] byte tee error branch1 (chunk) failed: {inner_error}"
                                );
                            }
                        }

                        // Step 18.2 chunk steps 1.4.2.2: "Perform ! ReadableByteStreamControllerError(branch2.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch2) = branch2.as_ref() {
                            if let Err(error) =
                                byte_tee_error_branch(branch2, error.clone(), context)
                            {
                                error!(
                                    "[readable-stream] byte tee error branch2 (chunk) failed: {error}"
                                );
                            }
                        }

                        // Step 18.2 chunk steps 1.4.2.3: "Resolve cancelPromise with ! ReadableStreamCancel(stream, cloneResult.[[Value]])."
                        let source_stream = tee_state.borrow().source_stream.clone();
                        let cancel_resolvers = tee_state.borrow().cancel_resolvers.clone();
                        let cancel_result = readable_stream_cancel(source_stream, error, context)?;
                        cancel_resolvers.resolve.call(
                            &JsValue::undefined(),
                            &[JsValue::from(cancel_result)],
                            context,
                        )?;

                        // Step 18.2 chunk steps 1.4.2.4: "Return."
                        return Ok(());
                    }
                }
            }

            // Step 18.2 chunk steps 1.5: "If canceled1 is false, perform ! ReadableByteStreamControllerEnqueue(branch1.[[controller]], chunk1)."
            if !canceled1 {
                if let Some(branch1) = branch1.as_ref() {
                    byte_tee_enqueue_to_branch(branch1, chunk1, context)?;
                }
            }

            // Step 18.2 chunk steps 1.6: "If canceled2 is false, perform ! ReadableByteStreamControllerEnqueue(branch2.[[controller]], chunk2)."
            if !canceled2 {
                if let Some(branch2) = branch2.as_ref() {
                    byte_tee_enqueue_to_branch(branch2, chunk2, context)?;
                }
            }

            // Step 18.2 chunk steps 1.7: "Set reading to false."
            // Step 18.2 chunk steps 1.8: "If readAgainForBranch1 is true, perform pull1Algorithm."
            // Step 18.2 chunk steps 1.9: "Otherwise, if readAgainForBranch2 is true, perform pull2Algorithm."
            let (read_again1, read_again2) = {
                let mut tee = tee_state.borrow_mut();
                tee.reading = false;
                (tee.read_again_for_branch1, tee.read_again_for_branch2)
            };
            if read_again1 {
                byte_tee_ignore_pull_completion(
                    readable_byte_stream_tee_pull1_algorithm(tee_state.clone(), context),
                    context,
                )?;
            } else if read_again2 {
                byte_tee_ignore_pull_completion(
                    readable_byte_stream_tee_pull2_algorithm(tee_state.clone(), context),
                    context,
                )?;
            }

            Ok(())
        },
        context,
    )
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_default_reader_close_steps(
    tee_state: GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<()> {
    let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
        let mut tee = tee_state.borrow_mut();
        tee.reading = false;
        (
            tee.branch1.clone(),
            tee.branch2.clone(),
            tee.canceled1,
            tee.canceled2,
            tee.cancel_resolvers.clone(),
        )
    };

    if !canceled1 {
        if let Some(branch1) = branch1.as_ref() {
            byte_tee_close_branch(branch1, context)?;
        }
    }
    if !canceled2 {
        if let Some(branch2) = branch2.as_ref() {
            byte_tee_close_branch(branch2, context)?;
        }
    }
    if !canceled1 {
        if let Some(branch1) = branch1.as_ref() {
            if let Some(controller) = byte_tee_pending_pull_into_controller(branch1) {
                let ec: &mut dyn ExecutionContext<crate::js::Types> =
                    js_engine::boa::context_as_ec(context);
                crate::js::completion_to_js_result(controller.respond(0, ec))?;
            }
        }
    }
    if !canceled2 {
        if let Some(branch2) = branch2.as_ref() {
            if let Some(controller) = byte_tee_pending_pull_into_controller(branch2) {
                let ec: &mut dyn ExecutionContext<crate::js::Types> =
                    js_engine::boa::context_as_ec(context);
                crate::js::completion_to_js_result(controller.respond(0, ec))?;
            }
        }
    }
    if !canceled1 || !canceled2 {
        cancel_resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
    }
    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_default_reader_error_steps(
    tee_state: GcCell<ByteTeeState>,
    _context: &mut Context,
) -> JsResult<()> {
    tee_state.borrow_mut().reading = false;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn readable_byte_stream_tee_pull_with_default_reader(
    tee_state: GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<()> {
    // Step 18.1: "If reader implements ReadableStreamBYOBReader,"
    byte_tee_switch_to_default_reader(&tee_state, context)?;

    // Step 18.2: "Let readRequest be a read request with the following items:"
    let default_reader = tee_state.borrow().reader.as_default_reader().unwrap();
    let read_request = ReadRequest::ReadableByteStreamTee {
        tee_state: tee_state.clone(),
    };

    // Step 18.3: "Perform ! ReadableStreamDefaultReaderRead(reader, readRequest)."
    crate::js::completion_to_js_result(
        default_reader.read_with_request(read_request, js_engine::boa::context_as_ec(context)),
    )
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn readable_byte_stream_tee_pull_with_byob_reader(
    tee_state: GcCell<ByteTeeState>,
    view_value: JsValue,
    for_branch2: bool,
    context: &mut Context,
) -> JsResult<()> {
    let view = match ArrayBufferViewDescriptor::from_value(
        view_value.clone(),
        js_engine::boa::context_as_ec(context),
    ) {
        Ok(v) => v,
        Err(js_error) => return Err(boa_engine::JsError::from_opaque(js_error)),
    };

    // Step 19.1: "If reader implements ReadableStreamDefaultReader,"
    byte_tee_switch_to_byob_reader(&tee_state, context)?;
    let byob_reader = tee_state.borrow().reader.as_byob_reader().unwrap();

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, args: &[JsValue], captures: &(GcCell<ByteTeeState>, bool), context| {
            let (tee_state, for_branch2) = captures;
            let result = args.get_or_undefined(0).to_object(context)?;
            let done = result.get(js_string!("done"), context)?.to_boolean();
            let chunk = result.get(js_string!("value"), context)?;

            let (byob_branch, other_branch, byob_canceled, other_canceled) = {
                let tee = tee_state.borrow();
                if *for_branch2 {
                    (
                        tee.branch2.clone(),
                        tee.branch1.clone(),
                        tee.canceled2,
                        tee.canceled1,
                    )
                } else {
                    (
                        tee.branch1.clone(),
                        tee.branch2.clone(),
                        tee.canceled1,
                        tee.canceled2,
                    )
                }
            };

            queue_internal_stream_microtask(
                {
                    let tee_state = tee_state.clone();
                    move |context| {
                        // Step 19.4 chunk steps 1.1: "Set readAgainForBranch1 to false."
                        // Step 19.4 chunk steps 1.2: "Set readAgainForBranch2 to false."
                        {
                            let mut tee = tee_state.borrow_mut();
                            tee.read_again_for_branch1 = false;
                            tee.read_again_for_branch2 = false;
                        }

                        if done {
                            // Step 19.4 close steps 1: "Set reading to false."
                            tee_state.borrow_mut().reading = false;

                            // Step 19.4 close steps 4: "If byobCanceled is false, perform ! ReadableByteStreamControllerClose(byobBranch.[[controller]])."
                            if !byob_canceled {
                                if let Some(branch) = byob_branch.as_ref() {
                                    byte_tee_close_branch(branch, context)?;
                                }
                            }

                            // Step 19.4 close steps 5: "If otherCanceled is false, perform ! ReadableByteStreamControllerClose(otherBranch.[[controller]])."
                            if !other_canceled {
                                if let Some(branch) = other_branch.as_ref() {
                                    byte_tee_close_branch(branch, context)?;
                                }
                            }

                            // Step 19.4 close steps 6: "If chunk is not undefined,"
                            if !chunk.is_undefined() {
                                // Step 19.4 close steps 6.2: "If byobCanceled is false, perform ! ReadableByteStreamControllerRespondWithNewView(byobBranch.[[controller]], chunk)."
                                if !byob_canceled {
                                    if let Some(branch) = byob_branch.as_ref() {
                                        let view = match ArrayBufferViewDescriptor::from_value(
                                            chunk.clone(),
                                            js_engine::boa::context_as_ec(context),
                                        ) {
                                            Ok(v) => v,
                                            Err(e) => {
                                                return Err(boa_engine::JsError::from_opaque(e));
                                            }
                                        };
                                        if let Some(view_object) = chunk.as_object() {
                                            if let Some(controller) = branch
                                                .controller_slot()
                                                .and_then(|c| c.as_byte_controller())
                                            {
                                                let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
                                                crate::js::completion_to_js_result(controller.respond_with_new_view(view, view_object, ec))?;
                                            }
                                        }
                                    }
                                }

                                // Step 19.4 close steps 6.3: "If otherCanceled is false and otherBranch.[[controller]].[[pendingPullIntos]] is not empty, perform ! ReadableByteStreamControllerRespond(otherBranch.[[controller]], 0)."
                                if !other_canceled {
                                    if let Some(branch) = other_branch.as_ref() {
                                        if let Some(controller) = byte_tee_pending_pull_into_controller(branch) {
                                            let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
                                            crate::js::completion_to_js_result(controller.respond(0, ec))?;
                                        }
                                    }
                                }
                            }

                            // Step 19.4 close steps 7: "If byobCanceled is false or otherCanceled is false, resolve cancelPromise with undefined."
                            if !byob_canceled || !other_canceled {
                                let cancel_resolvers = tee_state.borrow().cancel_resolvers.clone();
                                cancel_resolvers.resolve.call(
                                    &JsValue::undefined(),
                                    &[JsValue::undefined()],
                                    context,
                                )?;
                            }

                            return Ok(());
                        }

                        // Step 19.4 chunk steps 1.3: "Let byobCanceled be canceled2 if forBranch2 is true, and canceled1 otherwise."
                        // Step 19.4 chunk steps 1.4: "Let otherCanceled be canceled2 if forBranch2 is false, and canceled1 otherwise."
                        if !other_canceled {
                            // Step 19.4 chunk steps 1.5.1: "Let cloneResult be CloneAsUint8Array(chunk)."
                            match clone_as_uint8_array(chunk.clone(), js_engine::boa::context_as_ec(context)) {
                                Ok(cloned_chunk) => {
                                    // Step 19.4 chunk steps 1.5.3: "Otherwise, let clonedChunk be cloneResult.[[Value]]."
                                    // Step 19.4 chunk steps 1.5.4: "If byobCanceled is false, perform ! ReadableByteStreamControllerRespondWithNewView(byobBranch.[[controller]], chunk)."
                                    if !byob_canceled {
                                        if let Some(branch) = byob_branch.as_ref() {
                                            if let Ok(view) = ArrayBufferViewDescriptor::from_value(
                                                chunk.clone(),
                                                js_engine::boa::context_as_ec(context),
                                            ) {
                                                if let Some(view_object) = chunk.as_object() {
                                                    if let Some(controller) = branch
                                                        .controller_slot()
                                                        .and_then(|c| c.as_byte_controller())
                                                    {
                                                        let ec_ref: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
                                                        crate::js::completion_to_js_result(controller.respond_with_new_view(view, view_object, ec_ref))?;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Step 19.4 chunk steps 1.5.5: "Perform ! ReadableByteStreamControllerEnqueue(otherBranch.[[controller]], clonedChunk)."
                                    if let Some(branch) = other_branch.as_ref() {
                                        byte_tee_enqueue_to_branch(branch, cloned_chunk, context)?;
                                    }
                                }
                                Err(error) => {
                                    // Step 19.4 chunk steps 1.5.2.1: "Perform ! ReadableByteStreamControllerError(byobBranch.[[controller]], cloneResult.[[Value]])."
                                    if let Some(branch) = byob_branch.as_ref() {
                                        if let Err(error) = byte_tee_error_branch(branch, error.clone(), context) {
                                            error!("[readable-stream] byte tee error byob-branch (chunk) failed: {error}");
                                        }
                                    }

                                    // Step 19.4 chunk steps 1.5.2.2: "Perform ! ReadableByteStreamControllerError(otherBranch.[[controller]], cloneResult.[[Value]])."
                                    if let Some(branch) = other_branch.as_ref() {
                                        if let Err(error) = byte_tee_error_branch(branch, error.clone(), context) {
                                            error!("[readable-stream] byte tee error other-branch (chunk) failed: {error}");
                                        }
                                    }

                                    // Step 19.4 chunk steps 1.5.2.3: "Resolve cancelPromise with ! ReadableStreamCancel(stream, cloneResult.[[Value]])."
                                    let source_stream = tee_state.borrow().source_stream.clone();
                                    let cancel_resolvers = tee_state.borrow().cancel_resolvers.clone();
                                    let cancel_result = readable_stream_cancel(source_stream, error, context)?;
                                    cancel_resolvers.resolve.call(
                                        &JsValue::undefined(),
                                        &[JsValue::from(cancel_result)],
                                        context,
                                    )?;

                                    // Step 19.4 chunk steps 1.5.2.4: "Return."
                                    tee_state.borrow_mut().reading = false;
                                    return Ok(());
                                }
                            }
                        } else if !byob_canceled {
                            // Step 19.4 chunk steps 1.6: "Otherwise, if byobCanceled is false, perform ! ReadableByteStreamControllerRespondWithNewView(byobBranch.[[controller]], chunk)."
                            if let Some(branch) = byob_branch.as_ref() {
                                if let Ok(view) = ArrayBufferViewDescriptor::from_value(
                                    chunk.clone(),
                                    js_engine::boa::context_as_ec(context),
                                ) {
                                    if let Some(view_object) = chunk.as_object() {
                                        if let Some(controller) = branch
                                            .controller_slot()
                                            .and_then(|c| c.as_byte_controller())
                                        {
                                            let ec: &mut dyn ExecutionContext<crate::js::Types> = js_engine::boa::context_as_ec(context);
                                            crate::js::completion_to_js_result(controller.respond_with_new_view(view, view_object, ec))?;
                                        }
                                    }
                                }
                            }
                        }

                        // Step 19.4 chunk steps 1.7: "Set reading to false."
                        let (read_again1, read_again2) = {
                            let mut tee = tee_state.borrow_mut();
                            tee.reading = false;
                            (tee.read_again_for_branch1, tee.read_again_for_branch2)
                        };

                        // Step 19.4 chunk steps 1.8: "If readAgainForBranch1 is true, perform pull1Algorithm."
                        // Step 19.4 chunk steps 1.9: "Otherwise, if readAgainForBranch2 is true, perform pull2Algorithm."
                        if read_again1 {
                            byte_tee_ignore_pull_completion(
                                readable_byte_stream_tee_pull1_algorithm(tee_state.clone(), context),
                                context,
                            )?;
                        } else if read_again2 {
                            byte_tee_ignore_pull_completion(
                                readable_byte_stream_tee_pull2_algorithm(tee_state.clone(), context),
                                context,
                            )?;
                        } else if matches!(tee_state.borrow().reader, ReadableStreamReader::BYOB(_)) {
                            // Note: Switch back to the default reader when no branch has an outstanding BYOB pull.
                            byte_tee_switch_to_default_reader(&tee_state, context)?;
                        }

                        Ok(())
                    }
                },
                context,
            )?;

            Ok(JsValue::undefined())
        },
        (tee_state.clone(), for_branch2),
    )
    .to_js_function(context.realm());

    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, _, tee_state: &GcCell<ByteTeeState>, _| {
            tee_state.borrow_mut().reading = false;
            Ok(JsValue::undefined())
        },
        tee_state.clone(),
    )
    .to_js_function(context.realm());

    let (read_into_request, promise) = crate::js::completion_to_js_result(ReadIntoRequest::new(
        js_engine::boa::context_as_ec(context),
    ))?;

    // Step 19.5: "Perform ! ReadableStreamBYOBReaderRead(reader, view, 1, readIntoRequest)."
    crate::js::completion_to_js_result(byob_reader.read_steps(
        view,
        1,
        read_into_request,
        js_engine::boa::context_as_ec(context),
    ))?;
    let reaction: JsObject = JsPromise::from_object(promise)?
        .then(Some(on_fulfilled), Some(on_rejected), context)?
        .into();
    mark_promise_as_handled(&reaction, js_engine::boa::context_as_ec(context))
        .map_err(boa_engine::JsError::from_opaque)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_pull1_algorithm(
    tee_state: GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<JsValue> {
    {
        let mut tee = tee_state.borrow_mut();

        // Step 20.1: "If reading is true,"
        if tee.reading {
            // Step 20.1.1: "Set readAgainForBranch1 to true."
            tee.read_again_for_branch1 = true;

            // Step 20.1.2: "Return a promise resolved with undefined."
            return Ok(JsValue::undefined());
        }

        // Step 20.2: "Set reading to true."
        tee.reading = true;
    }

    // Step 20.3: "Let byobRequest be ! ReadableByteStreamControllerGetBYOBRequest(branch1.[[controller]])."
    let byob_request_view = {
        let tee = tee_state.borrow();
        tee.branch1
            .as_ref()
            .and_then(|branch| branch.controller_slot())
            .and_then(|controller| controller.as_byte_controller())
            .map(|controller| {
                let ec: &mut dyn ExecutionContext<crate::js::Types> =
                    js_engine::boa::context_as_ec(context);
                crate::js::completion_to_js_result(controller.byob_request(ec))
            })
            .transpose()?
            .flatten()
            .and_then(|request| request.get(js_string!("view"), context).ok())
            .filter(|value| !value.is_null() && !value.is_undefined())
    };

    // Step 20.4: "If byobRequest is null, perform pullWithDefaultReader."
    if let Some(view) = byob_request_view {
        // Step 20.5: "Otherwise, perform pullWithBYOBReader, given byobRequest.[[view]] and false."
        readable_byte_stream_tee_pull_with_byob_reader(tee_state, view, false, context)?;
    } else {
        readable_byte_stream_tee_pull_with_default_reader(tee_state, context)?;
    }

    // Step 20.6: "Return a promise resolved with undefined."
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_pull2_algorithm(
    tee_state: GcCell<ByteTeeState>,
    context: &mut Context,
) -> JsResult<JsValue> {
    {
        let mut tee = tee_state.borrow_mut();

        // Step 21.1: "If reading is true,"
        if tee.reading {
            // Step 21.1.1: "Set readAgainForBranch2 to true."
            tee.read_again_for_branch2 = true;

            // Step 21.1.2: "Return a promise resolved with undefined."
            return Ok(JsValue::undefined());
        }

        // Step 21.2: "Set reading to true."
        tee.reading = true;
    }

    // Step 21.3: "Let byobRequest be ! ReadableByteStreamControllerGetBYOBRequest(branch2.[[controller]])."
    let byob_request_view = {
        let tee = tee_state.borrow();
        tee.branch2
            .as_ref()
            .and_then(|branch| branch.controller_slot())
            .and_then(|controller| controller.as_byte_controller())
            .map(|controller| {
                let ec: &mut dyn ExecutionContext<crate::js::Types> =
                    js_engine::boa::context_as_ec(context);
                crate::js::completion_to_js_result(controller.byob_request(ec))
            })
            .transpose()?
            .flatten()
            .and_then(|request| request.get(js_string!("view"), context).ok())
            .filter(|value| !value.is_null() && !value.is_undefined())
    };

    // Step 21.4: "If byobRequest is null, perform pullWithDefaultReader."
    if let Some(view) = byob_request_view {
        // Step 21.5: "Otherwise, perform pullWithBYOBReader, given byobRequest.[[view]] and true."
        readable_byte_stream_tee_pull_with_byob_reader(tee_state, view, true, context)?;
    } else {
        readable_byte_stream_tee_pull_with_default_reader(tee_state, context)?;
    }

    // Step 21.6: "Return a promise resolved with undefined."
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_cancel1_algorithm(
    tee_state: GcCell<ByteTeeState>,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    let (source_stream, cancel_promise, canceled2, reason1, reason2, cancel_resolvers) = {
        let mut tee = tee_state.borrow_mut();
        tee.canceled1 = true;
        tee.reason1 = reason;
        (
            tee.source_stream.clone(),
            tee.cancel_promise.clone(),
            tee.canceled2,
            tee.reason1.clone(),
            tee.reason2.clone(),
            tee.cancel_resolvers.clone(),
        )
    };
    if canceled2 {
        let composite_reason =
            JsArray::from_iter([reason1, reason2].into_iter().map(JsValue::from), context);
        let cancel_result =
            readable_stream_cancel(source_stream, JsValue::from(composite_reason), context)?;
        cancel_resolvers.resolve.call(
            &JsValue::undefined(),
            &[JsValue::from(cancel_result)],
            context,
        )?;
    }
    Ok(cancel_promise)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_cancel2_algorithm(
    tee_state: GcCell<ByteTeeState>,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    let (source_stream, cancel_promise, canceled1, reason1, reason2, cancel_resolvers) = {
        let mut tee = tee_state.borrow_mut();
        tee.canceled2 = true;
        tee.reason2 = reason;
        (
            tee.source_stream.clone(),
            tee.cancel_promise.clone(),
            tee.canceled1,
            tee.reason1.clone(),
            tee.reason2.clone(),
            tee.cancel_resolvers.clone(),
        )
    };
    if canceled1 {
        let composite_reason =
            JsArray::from_iter([reason1, reason2].into_iter().map(JsValue::from), context);
        let cancel_result =
            readable_stream_cancel(source_stream, JsValue::from(composite_reason), context)?;
        cancel_resolvers.resolve.call(
            &JsValue::undefined(),
            &[JsValue::from(cancel_result)],
            context,
        )?;
    }
    Ok(cancel_promise)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn readable_byte_stream_tee(
    stream: ReadableStream,
    context: &mut Context,
) -> JsResult<ReadableStreamTeeBranches> {
    // Steps 1-2: Assert stream and stream.[[controller]] (implicit in types).
    // Step 3: Let reader be ? AcquireReadableStreamDefaultReader(stream).
    let reader_object =
        crate::js::completion_to_js_result(acquire_readable_stream_default_reader(
            stream.clone(),
            js_engine::boa::context_as_ec(context),
        ))?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, |r| r.clone())?;
    let reader_closed_promise = reader.closed()?;
    mark_promise_as_handled(
        &reader_closed_promise,
        js_engine::boa::context_as_ec(context),
    )
    .map_err(boa_engine::JsError::from_opaque)?;

    // Step 4: Let reading be false.
    // Step 5: Let readAgainForBranch1 be false.
    // Step 6: Let readAgainForBranch2 be false.
    // Step 7: Let canceled1 be false.
    // Step 8: Let canceled2 be false.
    // Step 9: Let reason1 be undefined.
    // Step 10: Let reason2 be undefined.
    // Step 11: Let branch1 be undefined.
    // Step 12: Let branch2 be undefined.
    // (All steps 4-12 initialized in ByteTeeState below.)

    // Step 13: Let cancelPromise be a new promise.
    let (cancel_promise, cancel_resolvers) = JsPromise::new_pending(context);

    let tee_state = gc_cell_new(ByteTeeState {
        source_stream: stream,
        reader: ReadableStreamReader::Default(reader),
        branch1: None,
        branch2: None,
        cancel_promise: cancel_promise.into(),
        cancel_resolvers,
        reading: false,
        read_again_for_branch1: false,
        read_again_for_branch2: false,
        canceled1: false,
        canceled2: false,
        reason1: JsValue::undefined(),
        reason2: JsValue::undefined(),
        reader_generation: 0,
    });

    // Step 22: "Let startAlgorithm be an algorithm that returns undefined."
    // Step 23: "Set branch1 to ! CreateReadableByteStream(startAlgorithm, pull1Algorithm, cancel1Algorithm)."
    let (branch1, branch1_object) = create_readable_byte_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableByteStreamTeeBranch1(tee_state.clone()),
        CancelAlgorithm::ReadableByteStreamTeeBranch1(tee_state.clone()),
        context,
    )?;

    // Step 24: "Set branch2 to ! CreateReadableByteStream(startAlgorithm, pull2Algorithm, cancel2Algorithm)."
    let (branch2, branch2_object) = create_readable_byte_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableByteStreamTeeBranch2(tee_state.clone()),
        CancelAlgorithm::ReadableByteStreamTeeBranch2(tee_state.clone()),
        context,
    )?;

    {
        let mut tee = tee_state.borrow_mut();
        tee.branch1 = Some(branch1.clone());
        tee.branch2 = Some(branch2.clone());
    }

    // Step 23: Perform forwardReaderError, given reader.
    byte_tee_forward_reader_error(&reader_object, &tee_state, context)?;
    // Step 24: Return « branch1, branch2 ».

    Ok(ReadableStreamTeeBranches {
        _branch1: branch1,
        branch1_object,
        _branch2: branch2,
        branch2_object,
    })
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-cloneasuint8array>
fn clone_as_uint8_array(
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    use boa_engine::object::builtins::JsUint8Array;
    let view = ArrayBufferViewDescriptor::from_value(chunk, ec)?;
    let src_bytes = view.bytes(ec)?;
    // SAFETY: JsUint8Array::from_iter requires Boa's Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let array = JsUint8Array::from_iter(src_bytes, context)
        .map_err(|e| e.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(array.into())
}

/// invocation.
fn underlying_source_type(
    source_object: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<Option<String>> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    if !source_object.has_property(js_string!("type"), context)? {
        return Ok(None);
    }

    let value = source_object.get(js_string!("type"), context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    Ok(Some(value.to_string(context)?.to_std_string_escaped()))
}
fn strategy_has_size(strategy: &JsValue, context: &mut Context) -> JsResult<bool> {
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(false);
    }

    let strategy = strategy.to_object(context)?;
    if !strategy.has_property(js_string!("size"), context)? {
        return Ok(false);
    }

    Ok(!strategy.get(js_string!("size"), context)?.is_undefined())
}

fn extract_abort_signal(
    options_object: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<Option<AbortSignal>> {
    let Some(options_object) = options_object else {
        return Ok(None);
    };

    if !options_object.has_property(js_string!("signal"), context)? {
        return Ok(None);
    }

    let signal = options_object.get(js_string!("signal"), context)?;
    if signal.is_undefined() {
        return Ok(None);
    }

    if signal.is_null() {
        return Err(JsNativeError::typ()
            .with_message("ReadableStream pipe options.signal must be an AbortSignal")
            .into());
    }

    let signal_object = signal.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStream pipe options.signal must be an AbortSignal")
    })?;

    with_abort_signal_ref(&signal_object, |signal| signal.clone()).map(Some)
}

struct PipeOptions {
    prevent_abort: bool,
    prevent_cancel: bool,
    prevent_close: bool,
    signal: Option<AbortSignal>,
}

fn normalize_pipe_options(options: &JsValue, context: &mut Context) -> JsResult<PipeOptions> {
    let options_object = if options.is_undefined() || options.is_null() {
        None
    } else {
        Some(options.to_object(context)?)
    };

    let prevent_abort = match options_object.as_ref() {
        Some(options_object) => options_object
            .get(js_string!("preventAbort"), context)?
            .to_boolean(),
        None => false,
    };

    let prevent_cancel = match options_object.as_ref() {
        Some(options_object) => options_object
            .get(js_string!("preventCancel"), context)?
            .to_boolean(),
        None => false,
    };

    let prevent_close = match options_object.as_ref() {
        Some(options_object) => options_object
            .get(js_string!("preventClose"), context)?
            .to_boolean(),
        None => false,
    };

    let signal = extract_abort_signal(options_object.as_ref(), context)?;

    Ok(PipeOptions {
        prevent_abort,
        prevent_cancel,
        prevent_close,
        signal,
    })
}

fn promise_rejected_with_reason(reason: JsValue, context: &mut Context) -> JsObject {
    crate::webidl::rejected_promise(reason, js_engine::boa::context_as_ec(context)).unwrap_or_else(
        |_| {
            let (promise, resolvers) = JsPromise::new_pending(context);
            let promise_object: JsObject = promise.into();
            if let Err(error) =
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[JsValue::undefined()], context)
            {
                error!("[readable-stream] failed to reject fallback promise: {error}");
            }
            promise_object
        },
    )
}

fn promise_rejected_with_type_error(message: &'static str, context: &mut Context) -> JsObject {
    let reason = match crate::js::completion_to_js_result(type_error_value(
        message,
        js_engine::boa::context_as_ec(context),
    )) {
        Ok(reason) => reason,
        Err(_) => JsValue::undefined(),
    };
    promise_rejected_with_reason(reason, context)
}

fn promise_rejected_with_error(error: JsError, context: &mut Context) -> JsObject {
    rejected_promise_from_error(error, js_engine::boa::context_as_ec(context))
}

fn reject_promise_with_error(
    resolvers: &ResolvingFunctions,
    error: JsError,
    context: &mut Context,
) {
    let reason = error_to_rejection_reason(error, js_engine::boa::context_as_ec(context));
    if let Err(error) = resolvers
        .reject
        .call(&JsValue::undefined(), &[reason], context)
    {
        error!("[readable-stream] failed to reject promise with error: {error}");
    }
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn readable_stream_pipe_to(
    source: ReadableStream,
    dest: super::WritableStream,
    prevent_close: bool,
    prevent_abort: bool,
    prevent_cancel: bool,
    signal: Option<AbortSignal>,
    context: &mut Context,
) -> JsObject {
    // Step 1: "Assert: source implements ReadableStream."

    // Step 2: "Assert: dest implements WritableStream."

    // Step 3: "Assert: preventClose, preventAbort, and preventCancel are all booleans."

    // Step 4: "If signal was not given, let signal be undefined."

    // Step 5: "Assert: either signal is undefined, or signal implements AbortSignal."
    // Note: `pipe_to()` and `pipe_through()` normalize the `signal` argument to `Option<AbortSignal>` before calling this helper.

    // Step 13: "Let promise be a new promise."
    // Note: the promise is allocated before the remaining setup so unexpected internal setup
    // failures are still reported through the same returned promise object.
    let (pipe_promise, pipe_resolvers) = JsPromise::new_pending(context);
    let pipe_promise_obj: JsObject = pipe_promise.into();

    // Step 8: "If source.[[controller]] implements ReadableByteStreamController, let reader be either ! AcquireReadableStreamBYOBReader(source) or ! AcquireReadableStreamDefaultReader(source), at the user agent’s discretion."
    // Note: Readable byte streams are not implemented yet, so the implementation always uses the default reader path.

    // Step 9: "Otherwise, let reader be ! AcquireReadableStreamDefaultReader(source)."
    let reader_object =
        match crate::js::completion_to_js_result(acquire_readable_stream_default_reader(
            source.clone(),
            js_engine::boa::context_as_ec(context),
        )) {
            Ok(reader_object) => reader_object,
            Err(error) => {
                reject_promise_with_error(&pipe_resolvers, error, context);
                return pipe_promise_obj;
            }
        };
    let reader =
        match with_readable_stream_default_reader_ref(&reader_object, |reader| reader.clone()) {
            Ok(reader) => reader,
            Err(error) => {
                reject_promise_with_error(&pipe_resolvers, error, context);
                return pipe_promise_obj;
            }
        };

    // Step 10: "Let writer be ! AcquireWritableStreamDefaultWriter(dest)."
    let writer_object = match super::acquire_writable_stream_default_writer(
        dest.clone(),
        js_engine::boa::context_as_ec(context),
    )
    .map_err(JsError::from_opaque)
    {
        Ok(writer_object) => writer_object,
        Err(error) => {
            if let Err(error) =
                crate::js::completion_to_js_result(readable_stream_default_reader_release(
                    reader.clone(),
                    js_engine::boa::context_as_ec(context),
                ))
            {
                error!("[readable-stream] failed to release reader on pipe setup error: {error}");
            }
            reject_promise_with_error(&pipe_resolvers, error, context);
            return pipe_promise_obj;
        }
    };
    let writer = match super::with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.clone()
    }) {
        Ok(writer) => writer,
        Err(error) => {
            if let Err(error) =
                crate::js::completion_to_js_result(readable_stream_default_reader_release(
                    reader.clone(),
                    js_engine::boa::context_as_ec(context),
                ))
            {
                error!("[readable-stream] failed to release reader on writer error: {error}");
            }
            reject_promise_with_error(&pipe_resolvers, error, context);
            return pipe_promise_obj;
        }
    };

    // Step 11: "Set source.[[disturbed]] to true."
    source.set_disturbed(true);

    // Step 12: "Let shuttingDown be false."

    // Step 15: "In parallel but not really; see #905, using reader and writer, read all chunks from source and write them to dest."
    // Note: The pipe progress below follows a single typed state machine and advances from Boa promise reactions at each microtask.
    let state = PipeToState::new(PipeToStateInner {
        promise: pipe_promise_obj.clone(),
        reader,
        writer,
        pending_writes: VecDeque::new(),
        state: PipePumpState::Starting,
        prevent_close,
        prevent_abort,
        prevent_cancel,
        signal: signal.clone(),
        shutdown_error: None,
        shutdown_action_promise: None,
        resolvers: Some(pipe_resolvers),
        shutting_down: false,
    });

    // Step 14: "If signal is not undefined,"
    if let Some(signal) = signal {
        // Step 14.1: "Let abortAlgorithm be the following steps:"
        let abort_algorithm = SignalAbortAlgorithm::ReadableStreamPipeTo {
            state: state.clone(),
        };

        // Step 14.2: "If signal is aborted, perform abortAlgorithm and return promise."
        if signal.aborted_value() {
            if let Err(error) = state.run_abort_algorithm(context) {
                state.reject_and_finalize_with_error(error, context);
            }
            return state.promise();
        }

        // Step 14.3: "Add abortAlgorithm to signal."
        signal.add_abort_algorithm(abort_algorithm);
    }

    // Step 16: "Return promise."
    if let Err(error) = state.check_and_propagate_errors_forward(context) {
        state.reject_and_finalize_with_error(error, context);
        return state.promise();
    }
    if let Err(error) = state.check_and_propagate_errors_backward(context) {
        state.reject_and_finalize_with_error(error, context);
        return state.promise();
    }
    if let Err(error) = state.check_and_propagate_closing_forward(context) {
        state.reject_and_finalize_with_error(error, context);
        return state.promise();
    }
    if let Err(error) = state.check_and_propagate_closing_backward(context) {
        state.reject_and_finalize_with_error(error, context);
        return state.promise();
    }

    if state.is_shutting_down() {
        return state.promise();
    }

    if let Err(error) = state.wait_for_writer_ready(context) {
        state.reject_and_finalize_with_error(error, context);
    }

    state.promise()
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct PipeToState(GcCell<PipeToStateInner>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipePumpState {
    Starting,
    PendingReady,
    PendingRead,
    ShuttingDownWithPendingWrites(Option<PipeShutdownAction>),
    ShuttingDownPendingAction(PipeShutdownAction),
    Finalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeShutdownAction {
    AbortDestination,
    CancelSource,
    CloseDestination,
    Abort,
}

#[derive(Trace, Finalize)]
pub(crate) struct PipeToStateInner {
    promise: JsObject,
    reader: ReadableStreamDefaultReader,
    writer: super::WritableStreamDefaultWriter,
    pending_writes: VecDeque<JsObject>,

    #[unsafe_ignore_trace]
    state: PipePumpState,

    #[unsafe_ignore_trace]
    prevent_close: bool,

    #[unsafe_ignore_trace]
    prevent_abort: bool,

    #[unsafe_ignore_trace]
    prevent_cancel: bool,

    signal: Option<AbortSignal>,
    shutdown_error: Option<JsValue>,
    shutdown_action_promise: Option<JsObject>,
    resolvers: Option<ResolvingFunctions>,

    #[unsafe_ignore_trace]
    shutting_down: bool,
}

impl PipeToState {
    fn new(state: PipeToStateInner) -> Self {
        Self(gc_cell_new(state))
    }

    fn borrow(&self) -> GcRef<'_, PipeToStateInner> {
        self.0.borrow()
    }

    fn borrow_mut(&self) -> GcRefMut<'_, PipeToStateInner> {
        self.0.borrow_mut()
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Gc::ptr_eq(&self.0, &other.0)
    }

    fn promise(&self) -> JsObject {
        self.borrow().promise.clone()
    }

    pub(crate) fn on_read_request_settled(
        &self,
        result: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        pipe_to_on_promise_settled(self.clone(), result, context)
    }

    fn reject_and_finalize_with_error(&self, error: JsError, context: &mut Context) {
        self.reject_and_finalize_with_reason(
            error_to_rejection_reason(error, js_engine::boa::context_as_ec(context)),
            context,
        );
    }

    fn reject_and_finalize_with_reason(&self, reason: JsValue, context: &mut Context) {
        self.set_shutdown_error(Some(reason));
        if let Err(error) = self.finalize(context) {
            error!("[readable-stream] failed to finalize on rejection: {error}");
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    pub(crate) fn run_abort_algorithm(&self, context: &mut Context) -> JsResult<()> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let error = {
            let state = self.borrow();
            state
                .signal
                .as_ref()
                .map(AbortSignal::reason_value)
                .ok_or_else(|| {
                    JsNativeError::typ().with_message(
                        "ReadableStreamPipeTo abort algorithm ran without an attached AbortSignal",
                    )
                })?
        };

        self.set_shutdown_error(Some(error));
        self.shutdown(Some(PipeShutdownAction::Abort), context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    pub(crate) fn run_abort_algorithm_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        self.run_abort_algorithm(context)
            .map_err(|e| e.into_opaque(context).unwrap_or(JsValue::undefined()))
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn wait_for_writer_ready(&self, context: &mut Context) -> JsResult<()> {
        self.set_state(PipePumpState::PendingReady);

        let (writer, reader) = {
            let state = self.borrow();
            (state.writer.clone(), state.reader.clone())
        };
        let ready_promise = writer.ready()?;
        let reader_closed_promise = reader.closed()?;

        if matches!(
            JsPromise::from_object(ready_promise.clone())?.state(),
            PromiseState::Fulfilled(_)
        ) {
            return self.read_chunk(context);
        }

        self.append_reaction(ready_promise, context)?;
        self.append_reaction(reader_closed_promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn read_chunk(&self, context: &mut Context) -> JsResult<()> {
        self.set_state(PipePumpState::PendingRead);

        let (reader, writer) = {
            let state = self.borrow();
            (state.reader.clone(), state.writer.clone())
        };
        let read_request = ReadRequest::ReadableStreamPipeTo {
            state: self.clone(),
        };
        crate::js::completion_to_js_result(
            reader.read_with_request(read_request, js_engine::boa::context_as_ec(context)),
        )?;
        let writer_closed_promise = writer.closed()?;

        self.append_reaction(writer_closed_promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn write_chunk(&self, result: JsValue, context: &mut Context) -> JsResult<bool> {
        let Some(result_object) = result.as_object() else {
            return Ok(false);
        };

        if !result_object.has_property(js_string!("done"), context)? {
            return Ok(false);
        }

        if result_object.get(js_string!("done"), context)?.to_boolean() {
            return Ok(false);
        }

        let value = result_object.get(js_string!("value"), context)?;
        let writer = {
            let state = self.borrow();
            state.writer.clone()
        };
        let write_promise = writer
            .write(value, js_engine::boa::context_as_ec(context))
            .map_err(JsError::from_opaque)?;
        self.borrow_mut().pending_writes.push_back(write_promise);
        Ok(true)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn wait_on_pending_write(&self, promise: JsObject, context: &mut Context) -> JsResult<()> {
        self.append_reaction(promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_errors_forward(&self, context: &mut Context) -> JsResult<()> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (source, dest, prevent_abort) = {
            let state = self.borrow();
            (
                state.reader.stream_slot_value(),
                state.writer.stream_slot_value(),
                state.prevent_abort,
            )
        };
        let Some(source) = source else {
            return Ok(());
        };

        if source.state() != ReadableStreamState::Errored {
            return Ok(());
        }

        if !prevent_abort
            && dest
                .as_ref()
                .is_some_and(|dest| dest.state() == super::WritableStreamState::Erroring)
        {
            return Ok(());
        }

        self.set_shutdown_error(Some(source.stored_error()));
        if prevent_abort {
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::AbortDestination), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_errors_backward(&self, context: &mut Context) -> JsResult<()> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (dest, source, prevent_cancel) = {
            let state = self.borrow();
            (
                state.writer.stream_slot_value(),
                state.reader.stream_slot_value(),
                state.prevent_cancel,
            )
        };
        let Some(dest) = dest else {
            return Ok(());
        };

        if !matches!(
            dest.state(),
            super::WritableStreamState::Erroring | super::WritableStreamState::Errored
        ) {
            return Ok(());
        }

        self.set_shutdown_error(Some(dest.stored_error()));
        let should_cancel = !prevent_cancel
            && source
                .as_ref()
                .is_some_and(|source| source.state() == ReadableStreamState::Readable);
        if !should_cancel {
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::CancelSource), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_closing_forward(&self, context: &mut Context) -> JsResult<()> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (source, prevent_close) = {
            let state = self.borrow();
            (state.reader.stream_slot_value(), state.prevent_close)
        };
        let Some(source) = source else {
            return Ok(());
        };

        if source.state() != ReadableStreamState::Closed {
            return Ok(());
        }

        if prevent_close {
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::CloseDestination), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_closing_backward(&self, context: &mut Context) -> JsResult<()> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (dest, source, prevent_cancel) = {
            let state = self.borrow();
            (
                state.writer.stream_slot_value(),
                state.reader.stream_slot_value(),
                state.prevent_cancel,
            )
        };
        let Some(dest) = dest else {
            return Ok(());
        };

        let source_is_readable = source
            .as_ref()
            .is_some_and(|source| source.state() == ReadableStreamState::Readable);

        if !source_is_readable {
            return Ok(());
        }

        if dest.state() != super::WritableStreamState::Closed && !dest.close_queued_or_in_flight() {
            return Ok(());
        }

        let ec: &mut dyn ExecutionContext<crate::js::Types> =
            js_engine::boa::context_as_ec(context);
        let error = crate::js::completion_to_js_result(type_error_value(
            "The destination WritableStream closed before the pipe operation completed",
            ec,
        ))?;
        self.set_shutdown_error(Some(error));
        if prevent_cancel {
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::CancelSource), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
    /// Note: This also covers <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown> when `action` is `None`.
    fn shutdown(&self, action: Option<PipeShutdownAction>, context: &mut Context) -> JsResult<()> {
        let pending_write = {
            let mut state = self.borrow_mut();
            if state.shutting_down {
                return Ok(());
            }

            state.shutting_down = true;

            let should_wait = state.writer.stream_slot_value().is_some_and(|dest| {
                dest.state() == super::WritableStreamState::Writable
                    && !dest.close_queued_or_in_flight()
                    && !state.pending_writes.is_empty()
            });
            if should_wait {
                state.state = PipePumpState::ShuttingDownWithPendingWrites(action);
                state.pending_writes.front().cloned()
            } else {
                None
            }
        };

        if let Some(pending_write) = pending_write {
            return self.wait_on_pending_write(pending_write, context);
        }

        if let Some(action) = action {
            return self.perform_action(action, context);
        }

        self.finalize(context)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
    fn perform_action(&self, action: PipeShutdownAction, context: &mut Context) -> JsResult<()> {
        let (writer, source, dest, error, prevent_abort, prevent_cancel) = {
            let mut state = self.borrow_mut();
            state.state = PipePumpState::ShuttingDownPendingAction(action);
            (
                state.writer.clone(),
                state.reader.stream_slot_value(),
                state.writer.stream_slot_value(),
                state
                    .shutdown_error
                    .clone()
                    .unwrap_or_else(JsValue::undefined),
                state.prevent_abort,
                state.prevent_cancel,
            )
        };

        let action_promise = match action {
            PipeShutdownAction::AbortDestination => match dest {
                Some(dest) => crate::js::completion_to_js_result(
                    dest.abort_stream(error, js_engine::boa::context_as_ec(context)),
                )?,
                None => {
                    resolved_promise(JsValue::undefined(), js_engine::boa::context_as_ec(context))
                        .map_err(boa_engine::JsError::from_opaque)?
                }
            },
            PipeShutdownAction::CancelSource => match source {
                Some(source) => readable_stream_cancel(source, error, context)?,
                None => {
                    resolved_promise(JsValue::undefined(), js_engine::boa::context_as_ec(context))
                        .map_err(boa_engine::JsError::from_opaque)?
                }
            },
            PipeShutdownAction::CloseDestination => match dest {
                Some(dest)
                    if dest.state() == super::WritableStreamState::Closed
                        || dest.close_queued_or_in_flight() =>
                {
                    resolved_promise(JsValue::undefined(), js_engine::boa::context_as_ec(context))
                        .map_err(boa_engine::JsError::from_opaque)?
                }
                _ => writer
                    .close(js_engine::boa::context_as_ec(context))
                    .map_err(JsError::from_opaque)?,
            },
            PipeShutdownAction::Abort => {
                let abort_promise = if !prevent_abort {
                    match dest {
                        Some(dest) if dest.state() == super::WritableStreamState::Writable => {
                            Some(crate::js::completion_to_js_result(dest.abort_stream(
                                error.clone(),
                                js_engine::boa::context_as_ec(context),
                            ))?)
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                let cancel_source = if !prevent_cancel {
                    match source {
                        Some(source) if source.state() == ReadableStreamState::Readable => {
                            Some(source)
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                match (abort_promise, cancel_source) {
                    (Some(abort_promise), Some(source)) => {
                        abort_destination_then_cancel_source(abort_promise, source, error, context)?
                    }
                    (Some(abort_promise), None) => abort_promise,
                    (None, Some(source)) => readable_stream_cancel(source, error, context)?,
                    (None, None) => resolved_promise(
                        JsValue::undefined(),
                        js_engine::boa::context_as_ec(context),
                    )
                    .map_err(boa_engine::JsError::from_opaque)?,
                }
            }
        };

        self.borrow_mut().shutdown_action_promise = Some(action_promise.clone());
        self.append_reaction(action_promise, context)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-finalize>
    fn finalize(&self, context: &mut Context) -> JsResult<()> {
        if self.current_state() == PipePumpState::Finalized {
            return Ok(());
        }

        let (writer, reader, signal, mut error, resolvers) = {
            let mut state = self.borrow_mut();
            state.state = PipePumpState::Finalized;
            (
                state.writer.clone(),
                state.reader.clone(),
                state.signal.clone(),
                state.shutdown_error.clone(),
                state.resolvers.take(),
            )
        };

        if let Err(release_error) = super::writable_stream_default_writer_release(
            writer,
            js_engine::boa::context_as_ec(context),
        )
        .map_err(JsError::from_opaque)
        {
            if error.is_none() {
                error = Some(error_to_rejection_reason(
                    release_error,
                    js_engine::boa::context_as_ec(context),
                ));
            }
        }
        if let Err(release_error) =
            crate::js::completion_to_js_result(super::readable_stream_default_reader_release(
                reader,
                js_engine::boa::context_as_ec(context),
            ))
        {
            if error.is_none() {
                error = Some(error_to_rejection_reason(
                    release_error,
                    js_engine::boa::context_as_ec(context),
                ));
            }
        }

        if let Some(signal) = signal {
            signal.remove_abort_algorithm(&SignalAbortAlgorithm::ReadableStreamPipeTo {
                state: self.clone(),
            });
        }

        if let Some(resolvers) = resolvers {
            match error {
                Some(error) => {
                    resolvers
                        .reject
                        .call(&JsValue::undefined(), &[error], context)?;
                }
                None => {
                    resolvers.resolve.call(
                        &JsValue::undefined(),
                        &[JsValue::undefined()],
                        context,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn current_state(&self) -> PipePumpState {
        self.borrow().state.clone()
    }

    fn set_state(&self, state: PipePumpState) {
        self.borrow_mut().state = state;
    }

    fn is_shutting_down(&self) -> bool {
        self.borrow().shutting_down
    }

    fn set_shutdown_error(&self, error: Option<JsValue>) {
        self.borrow_mut().shutdown_error = error;
    }

    fn update_pending_shutdown_action(
        &self,
        action: Option<PipeShutdownAction>,
        context: &mut Context,
    ) -> JsResult<Option<PipeShutdownAction>> {
        if action != Some(PipeShutdownAction::CloseDestination) {
            return Ok(action);
        }

        let (source, dest, prevent_cancel) = {
            let state = self.borrow();
            (
                state.reader.stream_slot_value(),
                state.writer.stream_slot_value(),
                state.prevent_cancel,
            )
        };

        let Some(dest) = dest else {
            return Ok(action);
        };

        let source_is_readable = source
            .as_ref()
            .is_some_and(|source| source.state() == ReadableStreamState::Readable);

        if matches!(
            dest.state(),
            super::WritableStreamState::Erroring | super::WritableStreamState::Errored
        ) {
            self.set_shutdown_error(Some(dest.stored_error()));
            return Ok(
                (!prevent_cancel && source_is_readable).then_some(PipeShutdownAction::CancelSource)
            );
        }

        if dest.state() == super::WritableStreamState::Closed || dest.close_queued_or_in_flight() {
            if !source_is_readable {
                return Ok(None);
            }

            let ec: &mut dyn ExecutionContext<crate::js::Types> =
                js_engine::boa::context_as_ec(context);
            let error = crate::js::completion_to_js_result(type_error_value(
                "The destination WritableStream closed before the pipe operation completed",
                ec,
            ))?;
            self.set_shutdown_error(Some(error));
            return Ok(
                (!prevent_cancel && source_is_readable).then_some(PipeShutdownAction::CancelSource)
            );
        }

        Ok(action)
    }

    fn pending_write_front(&self) -> Option<JsObject> {
        self.borrow().pending_writes.front().cloned()
    }

    fn shutdown_action_promise_state(&self) -> JsResult<Option<PromiseState>> {
        self.borrow()
            .shutdown_action_promise
            .clone()
            .map(|promise| Ok(JsPromise::from_object(promise)?.state()))
            .transpose()
    }

    fn prune_settled_pending_writes(&self, context: &mut Context) -> JsResult<()> {
        let mut handled = Vec::new();
        {
            let mut state = self.borrow_mut();
            state.pending_writes.retain(|promise_object| {
                let promise = match JsPromise::from_object(promise_object.clone()) {
                    Ok(promise) => promise,
                    Err(_) => {
                        debug_assert!(false, "pipeTo tracked a non-promise write handle");
                        return false;
                    }
                };
                let pending = matches!(promise.state(), PromiseState::Pending);
                if !pending {
                    handled.push(promise_object.clone());
                }
                pending
            });
        }

        for promise in handled {
            mark_promise_as_handled(&promise, js_engine::boa::context_as_ec(context))
                .map_err(boa_engine::JsError::from_opaque)?;
        }

        Ok(())
    }

    fn append_reaction(&self, promise: JsObject, context: &mut Context) -> JsResult<()> {
        let on_fulfilled = pipe_reaction_function(self.clone(), context);
        let on_rejected = pipe_reaction_function(self.clone(), context);
        let _ = JsPromise::from_object(promise)?.then(
            Some(on_fulfilled),
            Some(on_rejected),
            context,
        )?;
        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Trace, Finalize)]
struct WaitForAllState {
    #[unsafe_ignore_trace]
    remaining: usize,

    #[unsafe_ignore_trace]
    settled: bool,

    #[unsafe_ignore_trace]
    first_rejection_index: Option<usize>,

    first_rejection_reason: Option<JsValue>,

    resolvers: ResolvingFunctions,
}

#[derive(Trace, Finalize)]
struct AbortThenCancelState {
    source: Option<ReadableStream>,
    error: JsValue,
    abort_rejection: Option<JsValue>,
    resolvers: ResolvingFunctions,
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_to_on_promise_settled(
    state: PipeToState,
    result: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    state.prune_settled_pending_writes(context)?;

    let state_before_checks = state.current_state();

    if state_before_checks == PipePumpState::PendingRead {
        let (source, dest) = {
            let state_ref = state.borrow();
            (
                state_ref.reader.stream_slot_value(),
                state_ref.writer.stream_slot_value(),
            )
        };

        if let Some(source) = source {
            if source.state() == ReadableStreamState::Closed {
                if let Some(dest) = dest {
                    if dest.state() == super::WritableStreamState::Writable
                        && !dest.close_queued_or_in_flight()
                    {
                        let Some(done) = pipe_read_result_done(&result, context)? else {
                            return Ok(());
                        };

                        if !done {
                            let _ = state.write_chunk(result.clone(), context)?;
                        }
                    }
                }
            }
        }
    }

    state.check_and_propagate_errors_forward(context)?;
    state.check_and_propagate_errors_backward(context)?;
    state.check_and_propagate_closing_forward(context)?;
    state.check_and_propagate_closing_backward(context)?;

    let current_state = state.current_state();
    if current_state != state_before_checks {
        return Ok(());
    }

    match current_state {
        PipePumpState::Starting => {
            debug_assert!(
                false,
                "ReadableStream pipeTo callback reached the Starting state"
            );
        }
        PipePumpState::PendingReady => {
            state.read_chunk(context)?;
        }
        PipePumpState::PendingRead => {
            let _ = state.write_chunk(result, context)?;
            if state.is_shutting_down() {
                return Ok(());
            }
            state.wait_for_writer_ready(context)?;
        }
        PipePumpState::ShuttingDownWithPendingWrites(action) => {
            let action = state.update_pending_shutdown_action(action, context)?;
            state.set_state(PipePumpState::ShuttingDownWithPendingWrites(action));

            if let Some(pending_write) = state.pending_write_front() {
                state.wait_on_pending_write(pending_write, context)?;
            } else if let Some(action) = action {
                state.perform_action(action, context)?;
            } else {
                state.finalize(context)?;
            }
        }
        PipePumpState::ShuttingDownPendingAction(action) => {
            match state.shutdown_action_promise_state()? {
                Some(PromiseState::Pending) => return Ok(()),
                Some(PromiseState::Rejected(error)) => state.set_shutdown_error(Some(error)),
                Some(PromiseState::Fulfilled(value)) => {
                    if action != PipeShutdownAction::Abort && !value.is_undefined() {
                        state.set_shutdown_error(Some(value));
                    }
                }
                None => {}
            }

            state.finalize(context)?;
        }
        PipePumpState::Finalized => {}
    }

    Ok(())
}

fn pipe_reaction_function(state: PipeToState, context: &mut Context) -> JsFunction {
    NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &PipeToState, context| {
            pipe_to_on_promise_settled(state.clone(), args.get_or_undefined(0).clone(), context)?;
            Ok(JsValue::undefined())
        },
        state,
    )
    .to_js_function(context.realm())
}

fn pipe_read_result_done(result: &JsValue, context: &mut Context) -> JsResult<Option<bool>> {
    let Some(result_object) = result.as_object() else {
        return Ok(None);
    };

    if !result_object.has_property(js_string!("done"), context)? {
        return Ok(None);
    }

    Ok(Some(
        result_object.get(js_string!("done"), context)?.to_boolean(),
    ))
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
#[allow(dead_code)]
fn wait_for_all_promises(promises: Vec<JsObject>, context: &mut Context) -> JsResult<JsObject> {
    if promises.is_empty() {
        return crate::js::completion_to_js_result(resolved_promise(
            JsValue::undefined(),
            js_engine::boa::context_as_ec(context),
        ));
    }

    if promises.len() == 1 {
        if let Some(promise) = promises.into_iter().next() {
            return Ok(promise);
        }

        return crate::js::completion_to_js_result(resolved_promise(
            JsValue::undefined(),
            js_engine::boa::context_as_ec(context),
        ));
    }

    let (promise, resolvers) = JsPromise::new_pending(context);
    let aggregate = gc_cell_new(WaitForAllState {
        remaining: promises.len(),
        settled: false,
        first_rejection_index: None,
        first_rejection_reason: None,
        resolvers,
    });

    for (index, promise) in promises.into_iter().enumerate() {
        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, aggregate: &GcCell<WaitForAllState>, context| {
                let resolution = {
                    let mut aggregate = aggregate.borrow_mut();
                    if aggregate.settled {
                        return Ok(JsValue::undefined());
                    }

                    if aggregate.remaining > 0 {
                        aggregate.remaining -= 1;
                    }

                    if aggregate.remaining == 0 {
                        aggregate.settled = true;
                        Some((
                            aggregate.resolvers.clone(),
                            aggregate.first_rejection_reason.clone(),
                        ))
                    } else {
                        None
                    }
                };

                if let Some((resolvers, rejection_reason)) = resolution {
                    if let Some(rejection_reason) = rejection_reason {
                        resolvers.reject.call(
                            &JsValue::undefined(),
                            &[rejection_reason],
                            context,
                        )?;
                    } else {
                        resolvers.resolve.call(
                            &JsValue::undefined(),
                            &[JsValue::undefined()],
                            context,
                        )?;
                    }
                }

                Ok(JsValue::undefined())
            },
            aggregate.clone(),
        )
        .to_js_function(context.realm());

        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args, capture: &(usize, GcCell<WaitForAllState>), context| {
                let reason = args.get_or_undefined(0).clone();
                let resolution = {
                    let (index, aggregate) = capture;
                    let mut aggregate = aggregate.borrow_mut();
                    if aggregate.settled {
                        return Ok(JsValue::undefined());
                    }

                    if aggregate
                        .first_rejection_index
                        .is_none_or(|current_index| *index < current_index)
                    {
                        aggregate.first_rejection_index = Some(*index);
                        aggregate.first_rejection_reason = Some(reason);
                    }

                    if aggregate.remaining > 0 {
                        aggregate.remaining -= 1;
                    }

                    if aggregate.remaining == 0 {
                        aggregate.settled = true;
                        Some((
                            aggregate.resolvers.clone(),
                            aggregate.first_rejection_reason.clone(),
                        ))
                    } else {
                        None
                    }
                };

                if let Some((resolvers, rejection_reason)) = resolution {
                    if let Some(rejection_reason) = rejection_reason {
                        resolvers.reject.call(
                            &JsValue::undefined(),
                            &[rejection_reason],
                            context,
                        )?;
                    } else {
                        resolvers.resolve.call(
                            &JsValue::undefined(),
                            &[JsValue::undefined()],
                            context,
                        )?;
                    }
                }

                Ok(JsValue::undefined())
            },
            (index, aggregate.clone()),
        )
        .to_js_function(context.realm());

        let _ = JsPromise::from_object(promise)?.then(
            Some(on_fulfilled),
            Some(on_rejected),
            context,
        )?;
    }

    Ok(promise.into())
}

fn abort_destination_then_cancel_source(
    abort_promise: JsObject,
    source: ReadableStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    let (promise, resolvers) = JsPromise::new_pending(context);
    let state = gc_cell_new(AbortThenCancelState {
        source: Some(source),
        error,
        abort_rejection: None,
        resolvers,
    });

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, state: &GcCell<AbortThenCancelState>, context| {
            start_abort_cancel_source(state.clone(), None, context)
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &GcCell<AbortThenCancelState>, context| {
            start_abort_cancel_source(
                state.clone(),
                Some(args.get_or_undefined(0).clone()),
                context,
            )
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(abort_promise)?.then(
        Some(on_fulfilled),
        Some(on_rejected),
        context,
    )?;

    Ok(promise.into())
}

fn start_abort_cancel_source(
    state: GcCell<AbortThenCancelState>,
    abort_rejection: Option<JsValue>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (source, error) = {
        let mut state_ref = state.borrow_mut();
        state_ref.abort_rejection = abort_rejection;
        (state_ref.source.take(), state_ref.error.clone())
    };

    let cancel_promise = match source {
        Some(source) => readable_stream_cancel(source, error, context)?,
        None => resolved_promise(JsValue::undefined(), js_engine::boa::context_as_ec(context))
            .map_err(boa_engine::JsError::from_opaque)?,
    };

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, state: &GcCell<AbortThenCancelState>, context| {
            finalize_abort_cancel_source(state.clone(), None, context)
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &GcCell<AbortThenCancelState>, context| {
            finalize_abort_cancel_source(
                state.clone(),
                Some(args.get_or_undefined(0).clone()),
                context,
            )
        },
        state,
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(cancel_promise)?.then(
        Some(on_fulfilled),
        Some(on_rejected),
        context,
    )?;
    Ok(JsValue::undefined())
}

fn finalize_abort_cancel_source(
    state: GcCell<AbortThenCancelState>,
    cancel_rejection: Option<JsValue>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (abort_rejection, resolvers) = {
        let state_ref = state.borrow();
        (
            state_ref.abort_rejection.clone(),
            state_ref.resolvers.clone(),
        )
    };

    if let Some(reason) = abort_rejection.or(cancel_rejection) {
        resolvers
            .reject
            .call(&JsValue::undefined(), &[reason], context)?;
    } else {
        resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
    }

    Ok(JsValue::undefined())
}
