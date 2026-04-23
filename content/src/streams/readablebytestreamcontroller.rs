use std::{cell::Cell, collections::VecDeque, rc::Rc};

use boa_engine::{
    Context, JsData, JsNativeError, JsResult, JsValue,
    builtins::typed_array::TypedArrayKind,
    class::Class,
    js_string,
    native_function::NativeFunction,
    object::{
        JsObject,
        builtins::{JsArrayBuffer, JsDataView, JsPromise, JsTypedArray},
    },
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::webidl::resolved_promise;

use super::{
    CancelAlgorithm, PullAlgorithm, ReadIntoRequest, ReadRequest, ReadableStream,
    ReadableStreamController, ReadableStreamState, StartAlgorithm, extract_source_method,
    readable_stream_add_read_request,
    readable_stream_close, readable_stream_error, readable_stream_fulfill_read_request,
    readable_stream_get_num_read_requests, type_error_value,
};

#[derive(Clone, Trace, Finalize)]
pub(crate) enum ArrayBufferViewKind {
    DataView,
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    BigInt64Array,
    BigUint64Array,
    Float32Array,
    Float64Array,
}

impl ArrayBufferViewKind {
    fn from_typed_array_kind(kind: TypedArrayKind) -> JsResult<Self> {
        Ok(match kind {
            TypedArrayKind::Int8 => Self::Int8Array,
            TypedArrayKind::Uint8 => Self::Uint8Array,
            TypedArrayKind::Uint8Clamped => Self::Uint8ClampedArray,
            TypedArrayKind::Int16 => Self::Int16Array,
            TypedArrayKind::Uint16 => Self::Uint16Array,
            TypedArrayKind::Int32 => Self::Int32Array,
            TypedArrayKind::Uint32 => Self::Uint32Array,
            TypedArrayKind::BigInt64 => Self::BigInt64Array,
            TypedArrayKind::BigUint64 => Self::BigUint64Array,
            TypedArrayKind::Float32 => Self::Float32Array,
            TypedArrayKind::Float64 => Self::Float64Array,
            _ => {
                return Err(JsNativeError::typ()
                    .with_message("Unsupported typed array view kind")
                    .into())
            }
        })
    }

    fn element_size(&self) -> usize {
        match self {
            Self::DataView | Self::Int8Array | Self::Uint8Array | Self::Uint8ClampedArray => 1,
            Self::Int16Array | Self::Uint16Array => 2,
            Self::Int32Array | Self::Uint32Array | Self::Float32Array => 4,
            Self::BigInt64Array | Self::BigUint64Array | Self::Float64Array => 8,
        }
    }
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct ArrayBufferViewDescriptor {
    buffer: JsArrayBuffer,
    kind: ArrayBufferViewKind,
    #[unsafe_ignore_trace]
    byte_offset: usize,
    #[unsafe_ignore_trace]
    byte_length: usize,
}

impl ArrayBufferViewDescriptor {
    pub(crate) fn from_value(value: JsValue, context: &mut Context) -> JsResult<Self> {
        let object = value.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("Expected an ArrayBufferView object")
        })?;

        if let Ok(data_view) = JsDataView::from_object(object.clone()) {
            let buffer_value = data_view.buffer(context)?;
            let buffer_object = buffer_value.as_object().ok_or_else(|| {
                JsNativeError::typ().with_message("DataView buffer is not an object")
            })?;
            let buffer = JsArrayBuffer::from_object(buffer_object.clone())?;
            if buffer.data().is_none() {
                return Err(JsNativeError::typ()
                    .with_message("ArrayBufferView buffer is detached")
                    .into());
            }
            return Ok(Self {
                buffer,
                kind: ArrayBufferViewKind::DataView,
                byte_offset: data_view.byte_offset(context)? as usize,
                byte_length: data_view.byte_length(context)? as usize,
            });
        }

        let typed_array = JsTypedArray::from_object(object.clone())?;
        let kind = typed_array.kind().ok_or_else(|| {
            JsNativeError::typ().with_message("TypedArray view is missing its kind")
        })?;
        let buffer_value = typed_array.buffer(context)?;
        let buffer_object = buffer_value.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("TypedArray buffer is not an object")
        })?;
        let buffer = JsArrayBuffer::from_object(buffer_object.clone())?;
        if buffer.data().is_none() {
            return Err(JsNativeError::typ()
                .with_message("ArrayBufferView buffer is detached")
                .into());
        }
        Ok(Self {
            buffer,
            kind: ArrayBufferViewKind::from_typed_array_kind(kind)?,
            byte_offset: typed_array.byte_offset(context)?,
            byte_length: typed_array.byte_length(context)?,
        })
    }

    pub(crate) fn new_uint8(
        buffer: JsArrayBuffer,
        byte_offset: usize,
        byte_length: usize,
    ) -> Self {
        Self {
            buffer,
            kind: ArrayBufferViewKind::Uint8Array,
            byte_offset,
            byte_length,
        }
    }

    pub(crate) fn byte_length(&self) -> usize {
        self.byte_length
    }

    pub(crate) fn byte_offset(&self) -> usize {
        self.byte_offset
    }

    pub(crate) fn element_size(&self) -> usize {
        self.kind.element_size()
    }

    pub(crate) fn element_length(&self) -> usize {
        self.byte_length / self.element_size()
    }

    pub(crate) fn is_data_view(&self) -> bool {
        matches!(self.kind, ArrayBufferViewKind::DataView)
    }

    pub(crate) fn bytes(&self) -> JsResult<Vec<u8>> {
        let data = self.buffer.data().ok_or_else(|| {
            JsNativeError::typ().with_message("ArrayBufferView buffer is detached")
        })?;
        Ok(data[self.byte_offset..self.byte_offset + self.byte_length].to_vec())
    }

    pub(crate) fn create_result_view(
        &self,
        byte_length: usize,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        create_view_object(
            &self.kind,
            self.buffer.clone(),
            self.byte_offset,
            byte_length,
            context,
        )
    }

    pub(crate) fn create_remaining_view(
        &self,
        bytes_filled: usize,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        create_uint8_view_object(
            self.buffer.clone(),
            self.byte_offset + bytes_filled,
            self.byte_length.saturating_sub(bytes_filled),
            context,
        )
    }

    fn replace_with(&mut self, other: Self) {
        *self = other;
    }

    /// Spec step: `firstDescriptor.[[buffer]] = TransferArrayBuffer(view.[[ViewedArrayBuffer]])`.
    /// Updates only the backing buffer; `byte_offset` and `byte_length` are unchanged.
    fn transfer_buffer_from(&mut self, other: &Self) {
        self.buffer = other.buffer.clone();
    }
}

#[derive(Clone, Trace, Finalize)]
enum PullRequest {
    Default(ReadRequest),
    Byob(ReadIntoRequest),
}

/// <https://streams.spec.whatwg.org/#pull-into-descriptor>
#[derive(Clone, Trace, Finalize)]
struct PullIntoDescriptor {
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-buffer>
    view: ArrayBufferViewDescriptor,
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-bytes-filled>
    #[unsafe_ignore_trace]
    bytes_filled: usize,
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-minimum-fill>
    #[unsafe_ignore_trace]
    minimum_fill: usize,
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-reader-type>
    request: PullRequest,
}

impl PullIntoDescriptor {
    fn remaining_byte_length(&self) -> usize {
        self.view.byte_length().saturating_sub(self.bytes_filled)
    }

    fn can_commit(&self) -> bool {
        self.bytes_filled >= self.minimum_fill && self.bytes_filled % self.view.element_size() == 0
    }

    fn filled_view(&self, context: &mut Context) -> JsResult<JsObject> {
        self.view.create_result_view(self.bytes_filled, context)
    }

    fn close(self, context: &mut Context) -> JsResult<()> {
        match &self.request {
            PullRequest::Default(read_request) => {
                let value = if self.bytes_filled == 0 {
                    None
                } else {
                    Some(JsValue::from(self.filled_view(context)?))
                };
                let read_request = read_request.clone();
                if let Some(value) = value {
                    read_request.chunk_steps(value, context)
                } else {
                    read_request.close_steps(context)
                }
            }
            PullRequest::Byob(read_into_request) => {
                let value = JsValue::from(self.filled_view(context)?);
                read_into_request
                    .clone()
                    .close_steps(Some(value), context)
            }
        }
    }

    fn cancel(self, context: &mut Context) -> JsResult<()> {
        match &self.request {
            PullRequest::Default(read_request) => read_request.clone().close_steps(context),
            PullRequest::Byob(read_into_request) => {
                read_into_request.clone().close_steps(None, context)
            }
        }
    }

    fn commit(self, done: bool, context: &mut Context) -> JsResult<()> {
        let value = JsValue::from(self.filled_view(context)?);
        self.commit_with_value(value, done, context)
    }

    fn commit_with_value(self, value: JsValue, done: bool, context: &mut Context) -> JsResult<()> {
        match &self.request {
            PullRequest::Default(read_request) => {
                let read_request = read_request.clone();
                if done {
                    read_request.chunk_steps(value, context)
                } else {
                    read_request.chunk_steps(value, context)
                }
            }
            PullRequest::Byob(read_into_request) => {
                let read_into_request = read_into_request.clone();
                if done {
                    read_into_request.close_steps(Some(value), context)
                } else {
                    read_into_request.chunk_steps(value, context)
                }
            }
        }
    }

    fn error(self, error: JsValue, context: &mut Context) -> JsResult<()> {
        match &self.request {
            PullRequest::Default(read_request) => {
                read_request.clone().error_steps(error, context)
            }
            PullRequest::Byob(read_into_request) => {
                read_into_request.clone().error_steps(error, context)
            }
        }
    }
}

#[derive(Clone, Trace, Finalize)]
struct ByteQueueEntry {
    buffer: JsArrayBuffer,
    #[unsafe_ignore_trace]
    byte_offset: usize,
    #[unsafe_ignore_trace]
    byte_length: usize,
    #[unsafe_ignore_trace]
    offset: usize,
}

impl ByteQueueEntry {
    fn new(view: ArrayBufferViewDescriptor) -> Self {
        Self {
            buffer: view.buffer.clone(),
            byte_offset: view.byte_offset(),
            byte_length: view.byte_length(),
            offset: 0,
        }
    }

    fn remaining_len(&self) -> usize {
        self.byte_length.saturating_sub(self.offset)
    }

    fn remaining_byte_offset(&self) -> usize {
        self.byte_offset + self.offset
    }

    fn remaining_view(&self) -> ArrayBufferViewDescriptor {
        ArrayBufferViewDescriptor::new_uint8(
            self.buffer.clone(),
            self.remaining_byte_offset(),
            self.remaining_len(),
        )
    }
}

/// <https://streams.spec.whatwg.org/#readablestreambyobrequest>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStreamBYOBRequest {
    /// <https://streams.spec.whatwg.org/#readablestreambyobrequest-controller>
    controller: Gc<GcRefCell<Option<ReadableByteStreamController>>>,
    /// <https://streams.spec.whatwg.org/#readablestreambyobrequest-view>
    view: Gc<GcRefCell<Option<JsObject>>>,
}

impl ReadableStreamBYOBRequest {
    pub(crate) fn new(controller: ReadableByteStreamController) -> Self {
        Self {
            controller: Gc::new(GcRefCell::new(Some(controller))),
            view: Gc::new(GcRefCell::new(None)),
        }
    }

    fn controller_slot(&self) -> JsResult<ReadableByteStreamController> {
        self.controller.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamBYOBRequest is missing its controller")
                .into()
        })
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-view>
    pub(crate) fn view(&self) -> Option<JsObject> {
        self.view.borrow().clone()
    }

    pub(crate) fn set_view_slot(&self, view: Option<JsObject>) {
        *self.view.borrow_mut() = view;
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-respond>
    pub(crate) fn respond(&self, bytes_written: usize, context: &mut Context) -> JsResult<()> {
        self.controller_slot()?.respond(bytes_written, context)
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-respond-with-new-view>
    pub(crate) fn respond_with_new_view(
        &self,
        view: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        let view_object = view.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("respondWithNewView() requires an ArrayBufferView object")
        })?;
        let view = ArrayBufferViewDescriptor::from_value(view, context)?;
        self.controller_slot()?
            .respond_with_new_view(view, view_object, context)
    }
}

/// <https://streams.spec.whatwg.org/#readablebytestreamcontroller>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableByteStreamController {
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-stream>
    stream: Gc<GcRefCell<Option<ReadableStream>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-queue>
    queue: Gc<GcRefCell<VecDeque<ByteQueueEntry>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-queuetotalsize>
    #[unsafe_ignore_trace]
    queue_total_size: Rc<Cell<usize>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-started>
    #[unsafe_ignore_trace]
    started: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-closerequested>
    #[unsafe_ignore_trace]
    close_requested: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pullagain>
    #[unsafe_ignore_trace]
    pull_again: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pulling>
    #[unsafe_ignore_trace]
    pulling: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-strategyhwm>
    #[unsafe_ignore_trace]
    strategy_high_water_mark: Rc<Cell<f64>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-autoallocatechunksize>
    #[unsafe_ignore_trace]
    auto_allocate_chunk_size: Rc<Cell<Option<usize>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pullalgorithm>
    pull_algorithm: Gc<GcRefCell<Option<PullAlgorithm>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-cancelalgorithm>
    cancel_algorithm: Gc<GcRefCell<Option<CancelAlgorithm>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pendingpullintos>
    pending_pull_intos: Gc<GcRefCell<VecDeque<PullIntoDescriptor>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-byobrequest>
    byob_request_object: Gc<GcRefCell<Option<JsObject>>>,
}

impl ReadableByteStreamController {
    pub(crate) fn new() -> Self {
        Self {
            stream: Gc::new(GcRefCell::new(None)),
            queue: Gc::new(GcRefCell::new(VecDeque::new())),
            queue_total_size: Rc::new(Cell::new(0)),
            started: Rc::new(Cell::new(false)),
            close_requested: Rc::new(Cell::new(false)),
            pull_again: Rc::new(Cell::new(false)),
            pulling: Rc::new(Cell::new(false)),
            strategy_high_water_mark: Rc::new(Cell::new(0.0)),
            auto_allocate_chunk_size: Rc::new(Cell::new(None)),
            pull_algorithm: Gc::new(GcRefCell::new(None)),
            cancel_algorithm: Gc::new(GcRefCell::new(None)),
            pending_pull_intos: Gc::new(GcRefCell::new(VecDeque::new())),
            byob_request_object: Gc::new(GcRefCell::new(None)),
        }
    }

    fn stream_slot(&self) -> JsResult<ReadableStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController is missing its stream")
                .into()
        })
    }

    fn controller_object(&self) -> JsResult<JsObject> {
        self.stream_slot()?.controller_object_slot().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController is missing its JavaScript object")
                .into()
        })
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-clear-algorithms>
    fn clear_algorithms(&self) {
        *self.pull_algorithm.borrow_mut() = None;
        *self.cancel_algorithm.borrow_mut() = None;
    }

    /// <https://streams.spec.whatwg.org/#reset-queue>
    fn reset_queue(&self) {
        self.queue.borrow_mut().clear();
        self.queue_total_size.set(0);
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-invalidate-byob-request>
    fn invalidate_byob_request(&self) -> JsResult<()> {
        if let Some(object) = self.byob_request_object.borrow_mut().take() {
            with_readable_stream_byob_request_ref(&object, |request| request.set_view_slot(None))?;
        }
        Ok(())
    }

    fn update_byob_request_view(&self, context: &mut Context) -> JsResult<()> {
        let Some(object) = self.byob_request_object.borrow().clone() else {
            return Ok(());
        };
        let maybe_view = if let Some(descriptor) = self.pending_pull_intos.borrow().front() {
            Some(descriptor.view.create_remaining_view(descriptor.bytes_filled, context)?)
        } else {
            None
        };
        with_readable_stream_byob_request_ref(&object, |request| request.set_view_slot(maybe_view))?;
        Ok(())
    }

    pub(crate) fn pending_pull_intos_len(&self) -> usize {
        self.pending_pull_intos.borrow().len()
    }

    /// Returns a snapshot of the current BYOB request view as a JS value, without
    /// materialising a new BYOB request object.  Used by the byte-stream tee to
    /// inspect the pending pull-into view synchronously (non-spec helper).
    pub(crate) fn byob_request_immediate(&self) -> Option<JsValue> {
        let pending = self.pending_pull_intos.borrow();
        let descriptor = pending.front()?;
        // Return the cached BYOB request object's view if it exists, or signal
        // to the caller that there is a BYOB request (they can materialise it
        // via byob_request()).
        if let Some(ref obj) = *self.byob_request_object.borrow() {
            return Some(JsValue::from(obj.clone()));
        }
        // No cached object yet; just indicate there IS a pending pull-into
        // by returning a sentinel so the tee pull algorithm will call byob_request().
        let _ = descriptor;
        None
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-desired-size>
    pub(crate) fn desired_size(&self) -> JsResult<Option<f64>> {
        match self.stream_slot()?.state() {
            ReadableStreamState::Errored => Ok(None),
            ReadableStreamState::Closed => Ok(Some(0.0)),
            ReadableStreamState::Readable => Ok(Some(
                self.strategy_high_water_mark.get() - self.queue_total_size.get() as f64,
            )),
        }
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-byob-request>
    pub(crate) fn byob_request(&self, context: &mut Context) -> JsResult<Option<JsObject>> {
        if self.pending_pull_intos.borrow().is_empty() {
            self.invalidate_byob_request()?;
            return Ok(None);
        }

        if let Some(object) = self.byob_request_object.borrow().clone() {
            return Ok(Some(object));
        }

        let request = ReadableStreamBYOBRequest::new(self.clone());
        let object: JsObject = ReadableStreamBYOBRequest::from_data(request, context)?.into();
        *self.byob_request_object.borrow_mut() = Some(object.clone());
        self.update_byob_request_view(context)?;
        Ok(Some(object))
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-close>
    pub(crate) fn close(&self, context: &mut Context) -> JsResult<()> {
        if self.close_requested.get() || self.stream_slot()?.state() != ReadableStreamState::Readable {
            return Err(JsNativeError::typ()
                .with_message("The stream is not in a state that permits close")
                .into());
        }
        self.close_steps(context)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-enqueue>
    pub(crate) fn enqueue(&self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        if self.close_requested.get() || self.stream_slot()?.state() != ReadableStreamState::Readable {
            return Err(JsNativeError::typ()
                .with_message("The stream is not in a state that permits enqueue")
                .into());
        }
        self.enqueue_steps(chunk, context)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-error>
    pub(crate) fn error(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        self.error_steps(error, context)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-private-cancel>
    pub(crate) fn cancel_steps(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        self.reset_queue();
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut());
        self.invalidate_byob_request()?;
        for descriptor in pending {
            descriptor.cancel(context)?;
        }

        let cancel_algorithm = self.cancel_algorithm.borrow().clone();
        let result = match cancel_algorithm {
            Some(cancel_algorithm) => cancel_algorithm.call(reason, context)?,
            None => resolved_promise(JsValue::undefined(), context)?,
        };
        self.clear_algorithms();
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-private-pull>
    pub(crate) fn pull_steps(&self, read_request: ReadRequest, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot()?;
        if self.queue_total_size.get() > 0 {
            return self.fill_read_request_from_queue(stream, read_request, context);
        }

        if let Some(auto_allocate_chunk_size) = self.auto_allocate_chunk_size.get() {
            let buffer = JsArrayBuffer::new(auto_allocate_chunk_size, context)?;
            let descriptor = PullIntoDescriptor {
                view: ArrayBufferViewDescriptor::new_uint8(buffer, 0, auto_allocate_chunk_size),
                bytes_filled: 0,
                minimum_fill: 1,
                request: PullRequest::Default(read_request),
            };
            self.pending_pull_intos.borrow_mut().push_back(descriptor);
            let _ = self.byob_request(context)?;
            return self.call_pull_if_needed(context);
        }

        readable_stream_add_read_request(stream, read_request)?;
        self.call_pull_if_needed(context)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-pull-into>
    pub(crate) fn pull_into(
        &self,
        view: ArrayBufferViewDescriptor,
        min: usize,
        read_into_request: ReadIntoRequest,
        context: &mut Context,
    ) -> JsResult<()> {
        let stream = self.stream_slot()?;
        let mut descriptor = PullIntoDescriptor {
            minimum_fill: min * view.element_size(),
            view,
            bytes_filled: 0,
            request: PullRequest::Byob(read_into_request),
        };

        self.fill_pull_into_from_queue(&mut descriptor)?;
        if descriptor.can_commit() {
            return descriptor.commit(false, context);
        }

        if self.close_requested.get() && self.queue_total_size.get() == 0 {
            if descriptor.bytes_filled % descriptor.view.element_size() != 0 {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    context,
                )?;
                descriptor.error(error.clone(), context)?;
                self.clear_algorithms();
                readable_stream_error(stream, error, context)?;
                return Ok(());
            }

            self.clear_algorithms();
            descriptor.close(context)?;
            readable_stream_close(stream, context)?;
            return Ok(());
        }

        self.pending_pull_intos.borrow_mut().push_back(descriptor);
        let _ = self.byob_request(context)?;
        self.call_pull_if_needed(context)
    }

    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamcontroller-releasesteps>
    pub(crate) fn release_steps(&self, context: &mut Context) -> JsResult<()> {
        let release_error = type_error_value("Reader was released", context)?;
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut());
        self.invalidate_byob_request()?;
        for descriptor in pending {
            descriptor.error(release_error.clone(), context)?;
        }
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-close>
    pub(crate) fn close_steps(&self, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot()?;
        if self.close_requested.get() || stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        if self.queue_total_size.get() > 0 {
            self.close_requested.set(true);
            return Ok(());
        }

        if !self.pending_pull_intos.borrow().is_empty() {
            let has_misaligned_pending = {
                let pending_pull_intos = self.pending_pull_intos.borrow();
                pending_pull_intos.front().is_some_and(|descriptor| {
                    descriptor.bytes_filled > 0
                        && descriptor.bytes_filled % descriptor.view.element_size() != 0
                })
            };

            if has_misaligned_pending {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    context,
                )?;
                self.error_steps(error.clone(), context)?;
                return Err(JsNativeError::typ()
                    .with_message(
                        "Cannot close a byte stream with a partially filled typed array element",
                    )
                    .into());
            }

            self.close_requested.set(true);
            return Ok(());
        }

        self.clear_algorithms();
        readable_stream_close(stream, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue>
    pub(crate) fn enqueue_steps(&self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        let view = ArrayBufferViewDescriptor::from_value(chunk, context)?;
        if view.byte_length() == 0 {
            return Err(JsNativeError::typ()
                .with_message("ReadableByteStreamController.enqueue() requires a non-empty view")
                .into());
        }

        self.enqueue_chunk(view);
        self.process_pending_pull_intos_using_queue(context)?;
        self.process_read_requests_using_queue(context)?;
        self.call_pull_if_needed(context)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-error>
    pub(crate) fn error_steps(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        if self.stream_slot()?.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        self.reset_queue();
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut());
        self.invalidate_byob_request()?;
        for descriptor in pending {
            descriptor.error(error.clone(), context)?;
        }
        self.clear_algorithms();
        readable_stream_error(self.stream_slot()?, error, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond>
    pub(crate) fn respond(&self, bytes_written: usize, context: &mut Context) -> JsResult<()> {
        let descriptor = {
            let mut pending = self.pending_pull_intos.borrow_mut();
            let descriptor = pending.front_mut().ok_or_else(|| {
                JsNativeError::typ().with_message("There is no pending BYOB request to respond to")
            })?;

            if bytes_written > descriptor.remaining_byte_length() {
                return Err(JsNativeError::range()
                    .with_message("bytesWritten exceeds the available view size")
                    .into());
            }

            descriptor.bytes_filled += bytes_written;

            let should_commit = if self.close_requested.get() {
                true
            } else {
                descriptor.can_commit()
            };

            if should_commit {
                pending.pop_front().expect("front descriptor must exist")
            } else {
                drop(pending);
                self.update_byob_request_view(context)?;
                self.call_pull_if_needed(context)?;
                return Ok(());
            }
        };

        self.invalidate_byob_request()?;
        if self.close_requested.get() {
            if descriptor.bytes_filled % descriptor.view.element_size() != 0 {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    context,
                )?;
                self.error_steps(error, context)?;
                return Ok(());
            }
            descriptor.close(context)?;
        } else {
            descriptor.commit(false, context)?;
        }

        if self.close_requested.get()
            && self.queue_total_size.get() == 0
            && self.pending_pull_intos.borrow().is_empty()
        {
            self.clear_algorithms();
            readable_stream_close(self.stream_slot()?, context)?;
            return Ok(());
        }

        self.call_pull_if_needed(context)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond-with-new-view>
    pub(crate) fn respond_with_new_view(
        &self,
        view: ArrayBufferViewDescriptor,
        view_object: JsObject,
        context: &mut Context,
    ) -> JsResult<()> {
        let bytes_written = view.byte_length();
        let descriptor_to_commit = {
            let mut pending = self.pending_pull_intos.borrow_mut();
            let descriptor = pending.front_mut().ok_or_else(|| {
                JsNativeError::typ().with_message("There is no pending BYOB request to respond to")
            })?;
            if view.byte_offset() != descriptor.view.byte_offset() + descriptor.bytes_filled {
                return Err(JsNativeError::range()
                    .with_message("respondWithNewView() must preserve the current byte offset")
                    .into());
            }
            if view.byte_length() > descriptor.remaining_byte_length() {
                return Err(JsNativeError::range()
                    .with_message("respondWithNewView() view is larger than the remaining request")
                    .into());
            }

            descriptor.bytes_filled += bytes_written;
            // Spec: set firstDescriptor.[[buffer]] to TransferArrayBuffer(view.[[ViewedArrayBuffer]]).
            // Only the buffer is updated; byte_offset and byte_length remain from the original pull-into.
            descriptor.view.transfer_buffer_from(&view);

            let should_commit = if self.close_requested.get() {
                true
            } else {
                descriptor.can_commit()
            };

            if should_commit {
                Some(pending.pop_front().expect("front descriptor must exist"))
            } else {
                None
            }
        };

        let Some(descriptor) = descriptor_to_commit else {
            self.update_byob_request_view(context)?;
            self.call_pull_if_needed(context)?;
            return Ok(());
        };

        self.invalidate_byob_request()?;
        if self.close_requested.get() {
            if descriptor.bytes_filled % descriptor.view.element_size() != 0 {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    context,
                )?;
                self.error_steps(error, context)?;
                return Ok(());
            }
            // Spec: ConvertPullIntoDescriptor — result view covers [byteOffset .. bytesFilled].
            let result_view = descriptor.view.create_result_view(descriptor.bytes_filled, context)?;
            descriptor.commit_with_value(JsValue::from(result_view), true, context)?;
        } else {
            // Spec: ConvertPullIntoDescriptor — result view covers [byteOffset .. bytesFilled].
            let result_view = descriptor.view.create_result_view(descriptor.bytes_filled, context)?;
            descriptor.commit_with_value(JsValue::from(result_view), false, context)?;
        }

        if self.close_requested.get()
            && self.queue_total_size.get() == 0
            && self.pending_pull_intos.borrow().is_empty()
        {
            self.clear_algorithms();
            readable_stream_close(self.stream_slot()?, context)?;
            return Ok(());
        }

        self.call_pull_if_needed(context)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-call-pull-if-needed>
    pub(crate) fn call_pull_if_needed(&self, context: &mut Context) -> JsResult<()> {
        if !self.should_call_pull()? {
            return Ok(());
        }
        if self.pulling.get() {
            self.pull_again.set(true);
            return Ok(());
        }

        self.pulling.set(true);
        let controller_object = self.controller_object()?;
        let pull_algorithm = self.pull_algorithm.borrow().clone();
        let pull_promise = match pull_algorithm {
            Some(pull_algorithm) => pull_algorithm.call(&controller_object, context)?,
            None => resolved_promise(JsValue::undefined(), context)?,
        };

        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, controller: &ReadableByteStreamController, context| {
                controller.pulling.set(false);
                if controller.pull_again.get() {
                    controller.pull_again.set(false);
                    controller.call_pull_if_needed(context)?;
                }
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args, controller: &ReadableByteStreamController, context| {
                controller.error_steps(args.first().cloned().unwrap_or_default(), context)?;
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());

        let _ = JsPromise::from_object(pull_promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)?;
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-should-call-pull>
    fn should_call_pull(&self) -> JsResult<bool> {
        let stream = self.stream_slot()?;
        if !self.started.get()
            || self.close_requested.get()
            || stream.state() != ReadableStreamState::Readable
        {
            return Ok(false);
        }

        if !self.pending_pull_intos.borrow().is_empty() {
            return Ok(true);
        }

        if stream
            .reader_slot()
            .and_then(|reader| reader.as_default_reader())
            .is_some()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            return Ok(self.queue_total_size.get() == 0);
        }

        Ok(self.desired_size()?.is_some_and(|size| size > 0.0))
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue-chunk-to-queue>
    fn enqueue_chunk(&self, view: ArrayBufferViewDescriptor) {
        self.queue_total_size
            .set(self.queue_total_size.get() + view.byte_length());
        self.queue.borrow_mut().push_back(ByteQueueEntry::new(view));
    }

    fn dequeue_chunk_as_value(&self, context: &mut Context) -> JsResult<JsValue> {
        let entry = self.queue.borrow_mut().pop_front().ok_or_else(|| {
            JsNativeError::typ().with_message("Readable byte stream queue is empty")
        })?;
        let remaining_len = entry.remaining_len();
        let remaining_view = entry.remaining_view();
        self.queue_total_size
            .set(self.queue_total_size.get().saturating_sub(remaining_len));
        Ok(JsValue::from(
            remaining_view.create_result_view(remaining_len, context)?,
        ))
    }

    fn fill_read_request_from_queue(
        &self,
        stream: ReadableStream,
        read_request: ReadRequest,
        context: &mut Context,
    ) -> JsResult<()> {
        let chunk = self.dequeue_chunk_as_value(context)?;
        read_request.chunk_steps(chunk, context)?;
        if self.close_requested.get() && self.queue_total_size.get() == 0 {
            self.clear_algorithms();
            readable_stream_close(stream, context)?;
        }
        Ok(())
    }

    fn process_read_requests_using_queue(&self, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot()?;
        while self.queue_total_size.get() > 0
            && stream
                .reader_slot()
                .and_then(|reader| reader.as_default_reader())
                .is_some()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            let chunk = self.dequeue_chunk_as_value(context)?;
            readable_stream_fulfill_read_request(stream.clone(), chunk, false, context)?;
        }

        if self.close_requested.get() && self.queue_total_size.get() == 0 && self.pending_pull_intos.borrow().is_empty() {
            self.clear_algorithms();
            readable_stream_close(stream, context)?;
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-fill-pull-into-descriptor-from-queue>
    fn fill_pull_into_from_queue(&self, descriptor: &mut PullIntoDescriptor) -> JsResult<()> {
        let total_to_copy = descriptor.remaining_byte_length().min(self.queue_total_size.get());
        if total_to_copy == 0 {
            return Ok(());
        }

        let mut copied = Vec::with_capacity(total_to_copy);
        let mut remaining = total_to_copy;
        {
            let mut queue = self.queue.borrow_mut();
            while remaining > 0 {
                let mut entry = queue.pop_front().ok_or_else(|| {
                    JsNativeError::typ().with_message("Readable byte stream queue is empty")
                })?;
                let to_take = remaining.min(entry.remaining_len());
                let start = entry.remaining_byte_offset();
                let bytes = {
                    let data = entry.buffer.data().ok_or_else(|| {
                        JsNativeError::typ().with_message("Readable byte stream queue entry buffer is detached")
                    })?;
                    data[start..start + to_take].to_vec()
                };
                copied.extend_from_slice(&bytes);
                entry.offset += to_take;
                if entry.remaining_len() > 0 {
                    queue.push_front(entry);
                }
                remaining -= to_take;
            }
        }

        self.queue_total_size
            .set(self.queue_total_size.get().saturating_sub(copied.len()));
        let mut data = descriptor.view.buffer.data_mut().ok_or_else(|| {
            JsNativeError::typ().with_message("BYOB request buffer is detached")
        })?;
        let start = descriptor.view.byte_offset() + descriptor.bytes_filled;
        let end = start + copied.len();
        data[start..end].copy_from_slice(&copied);
        descriptor.bytes_filled += copied.len();
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-process-pull-into-descriptors-using-queue>
    fn process_pending_pull_intos_using_queue(&self, context: &mut Context) -> JsResult<()> {
        loop {
            if self.queue_total_size.get() == 0 {
                break;
            }
            let Some(mut descriptor) = self.pending_pull_intos.borrow_mut().pop_front() else {
                break;
            };
            self.fill_pull_into_from_queue(&mut descriptor)?;
            if descriptor.can_commit() {
                self.invalidate_byob_request()?;
                descriptor.commit(false, context)?;
                continue;
            }
            self.pending_pull_intos.borrow_mut().push_front(descriptor);
            self.update_byob_request_view(context)?;
            break;
        }
        Ok(())
    }
}

pub(crate) fn with_readable_byte_stream_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableByteStreamController) -> R,
) -> JsResult<R> {
    let controller = object.downcast_ref::<ReadableByteStreamController>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not a ReadableByteStreamController")
    })?;
    Ok(f(&controller))
}

pub(crate) fn with_readable_stream_byob_request_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStreamBYOBRequest) -> R,
) -> JsResult<R> {
    let request = object.downcast_ref::<ReadableStreamBYOBRequest>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not a ReadableStreamBYOBRequest")
    })?;
    Ok(f(&request))
}

/// <https://streams.spec.whatwg.org/#set-up-readable-byte-stream-controller-from-underlying-source>
pub(crate) fn set_up_readable_byte_stream_controller_from_underlying_source(
    stream: ReadableStream,
    underlying_source_object: Option<JsObject>,
    high_water_mark: f64,
    context: &mut Context,
) -> JsResult<()> {
    let controller = ReadableByteStreamController::new();
    let controller_object: JsObject = ReadableByteStreamController::from_data(controller.clone(), context)?.into();

    let mut start_algorithm = StartAlgorithm::ReturnUndefined;
    let mut pull_algorithm = PullAlgorithm::ReturnUndefined;
    let mut cancel_algorithm = CancelAlgorithm::ReturnUndefined;

    if let Some(start_method) = extract_source_method(underlying_source_object.as_ref(), "start", context)? {
        start_algorithm = StartAlgorithm::JavaScript(start_method);
    }
    if let Some(pull_method) = extract_source_method(underlying_source_object.as_ref(), "pull", context)? {
        pull_algorithm = PullAlgorithm::JavaScript(pull_method);
    }
    if let Some(cancel_method) = extract_source_method(underlying_source_object.as_ref(), "cancel", context)? {
        cancel_algorithm = CancelAlgorithm::JavaScript(cancel_method);
    }

    let auto_allocate_chunk_size = extract_auto_allocate_chunk_size(underlying_source_object.as_ref(), context)?;

    set_up_readable_byte_stream_controller(
        stream,
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        high_water_mark,
        auto_allocate_chunk_size,
        context,
    )
}

/// <https://streams.spec.whatwg.org/#set-up-readable-byte-stream-controller>
pub(crate) fn set_up_readable_byte_stream_controller(
    stream: ReadableStream,
    controller: ReadableByteStreamController,
    controller_object: &JsObject,
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: f64,
    auto_allocate_chunk_size: Option<usize>,
    context: &mut Context,
) -> JsResult<()> {
    *controller.stream.borrow_mut() = Some(stream.clone());
    stream.set_controller_slot(Some(ReadableStreamController::Byte(controller.clone())));
    stream.set_controller_object_slot(Some(controller_object.clone()));
    controller.reset_queue();
    controller.close_requested.set(false);
    controller.started.set(false);
    controller.pull_again.set(false);
    controller.pulling.set(false);
    controller.strategy_high_water_mark.set(high_water_mark);
    controller.auto_allocate_chunk_size.set(auto_allocate_chunk_size);
    *controller.pull_algorithm.borrow_mut() = Some(pull_algorithm);
    *controller.cancel_algorithm.borrow_mut() = Some(cancel_algorithm);
    controller.pending_pull_intos.borrow_mut().clear();

    let start_result = start_algorithm.call(controller_object, context)?;
    let start_promise = JsPromise::resolve(start_result, context)?;

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, controller: &ReadableByteStreamController, context| {
            controller.started.set(true);
            controller.call_pull_if_needed(context)?;
            Ok(JsValue::undefined())
        },
        controller.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, controller: &ReadableByteStreamController, context| {
            controller.error_steps(args.first().cloned().unwrap_or_default(), context)?;
            Ok(JsValue::undefined())
        },
        controller,
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(start_promise.into())?
        .then(Some(on_fulfilled), Some(on_rejected), context)?;
    Ok(())
}

fn extract_auto_allocate_chunk_size(
    source_object: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<Option<usize>> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    let value = source_object.get(js_string!("autoAllocateChunkSize"), context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    let number = value.to_number(context)?;
    if !number.is_finite() || number <= 0.0 || number.fract() != 0.0 {
        return Err(JsNativeError::typ()
            .with_message("autoAllocateChunkSize must be a positive integer")
            .into());
    }

    Ok(Some(number as usize))
}

fn create_view_object(
    kind: &ArrayBufferViewKind,
    buffer: JsArrayBuffer,
    byte_offset: usize,
    byte_length: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    match kind {
        ArrayBufferViewKind::DataView => Ok(JsDataView::from_js_array_buffer(
            buffer,
            Some(byte_offset as u64),
            Some(byte_length as u64),
            context,
        )?
        .into()),
        _ => create_typed_array_view_object(kind, buffer, byte_offset, byte_length, context),
    }
}

fn create_uint8_view_object(
    buffer: JsArrayBuffer,
    byte_offset: usize,
    byte_length: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    create_typed_array_view_object(
        &ArrayBufferViewKind::Uint8Array,
        buffer,
        byte_offset,
        byte_length,
        context,
    )
}

fn create_typed_array_view_object(
    kind: &ArrayBufferViewKind,
    buffer: JsArrayBuffer,
    byte_offset: usize,
    byte_length: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    let length = byte_length / kind.element_size();
    let constructor = match kind {
        ArrayBufferViewKind::Int8Array => context.intrinsics().constructors().typed_int8_array().constructor(),
        ArrayBufferViewKind::Uint8Array => context.intrinsics().constructors().typed_uint8_array().constructor(),
        ArrayBufferViewKind::Uint8ClampedArray => context.intrinsics().constructors().typed_uint8clamped_array().constructor(),
        ArrayBufferViewKind::Int16Array => context.intrinsics().constructors().typed_int16_array().constructor(),
        ArrayBufferViewKind::Uint16Array => context.intrinsics().constructors().typed_uint16_array().constructor(),
        ArrayBufferViewKind::Int32Array => context.intrinsics().constructors().typed_int32_array().constructor(),
        ArrayBufferViewKind::Uint32Array => context.intrinsics().constructors().typed_uint32_array().constructor(),
        ArrayBufferViewKind::BigInt64Array => context.intrinsics().constructors().typed_bigint64_array().constructor(),
        ArrayBufferViewKind::BigUint64Array => context.intrinsics().constructors().typed_biguint64_array().constructor(),
        ArrayBufferViewKind::Float32Array => context.intrinsics().constructors().typed_float32_array().constructor(),
        ArrayBufferViewKind::Float64Array => context.intrinsics().constructors().typed_float64_array().constructor(),
        ArrayBufferViewKind::DataView => {
            return Err(JsNativeError::typ()
                .with_message("DataView uses a separate constructor path")
                .into())
        }
    };

    constructor.construct(
        &[
            JsValue::from(buffer),
            JsValue::from(byte_offset as u64),
            JsValue::from(length as u64),
        ],
        None,
        context,
    )
}