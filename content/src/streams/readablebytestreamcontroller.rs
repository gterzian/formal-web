use std::{cell::Cell, collections::VecDeque, rc::Rc};

use boa_engine::{
    Context, JsValue,
    builtins::typed_array::TypedArrayKind,
    object::{JsObject, builtins::JsArrayBuffer},
};

use js_engine::{Completion, ExecutionContext, JsTypes, TypedArrayElementType};

use crate::webidl::bindings::create_interface_instance;
use crate::webidl::resolved_promise;
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

use super::{
    CancelAlgorithm, PullAlgorithm, ReadIntoRequest, ReadRequest, ReadableStream,
    ReadableStreamState, StartAlgorithm, extract_source_method, readable_stream_add_read_request,
    readable_stream_close, readable_stream_error, readable_stream_fulfill_read_request,
    readable_stream_get_num_read_requests, type_error_value,
};

#[gc_struct]
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
    fn to_typed_array_element_type(&self) -> Option<TypedArrayElementType> {
        Some(match self {
            Self::Int8Array => TypedArrayElementType::Int8,
            Self::Uint8Array => TypedArrayElementType::Uint8,
            Self::Uint8ClampedArray => TypedArrayElementType::Uint8Clamped,
            Self::Int16Array => TypedArrayElementType::Int16,
            Self::Uint16Array => TypedArrayElementType::Uint16,
            Self::Int32Array => TypedArrayElementType::Int32,
            Self::Uint32Array => TypedArrayElementType::Uint32,
            Self::BigInt64Array => TypedArrayElementType::BigInt64,
            Self::BigUint64Array => TypedArrayElementType::BigUint64,
            Self::Float32Array => TypedArrayElementType::Float32,
            Self::Float64Array => TypedArrayElementType::Float64,
            Self::DataView => return None,
        })
    }

    fn from_typed_array_kind(kind: TypedArrayKind) -> Self {
        match kind {
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
        }
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

#[gc_struct]
pub(crate) struct ArrayBufferViewDescriptor {
    buffer: JsArrayBuffer,
    kind: ArrayBufferViewKind,
    #[ignore_trace]
    byte_offset: usize,
    #[ignore_trace]
    byte_length: usize,
}

impl ArrayBufferViewDescriptor {
    pub(crate) fn from_value(
        value: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let object = <crate::js::Types as JsTypes>::value_as_object(&value)
            .ok_or_else(|| ec.new_type_error("Expected an ArrayBufferView object"))?;

        if let Some(data_view) = <crate::js::Types as JsTypes>::object_as_data_view(&object) {
            let buffer = ec.data_view_buffer(&data_view)?;
            if ec.array_buffer_data(&buffer).is_none() {
                return Err(ec.new_type_error("ArrayBufferView buffer is detached"));
            }
            return Ok(Self {
                buffer,
                kind: ArrayBufferViewKind::DataView,
                byte_offset: ec.data_view_byte_offset(&data_view)? as usize,
                byte_length: ec.data_view_byte_length(&data_view)? as usize,
            });
        }

        if let Some(typed_array) = <crate::js::Types as JsTypes>::object_as_typed_array(&object) {
            let kind = typed_array
                .kind()
                .ok_or_else(|| ec.new_type_error("TypedArray view is missing its kind"))?;
            let buffer = ec.typed_array_buffer(&typed_array)?;
            if ec.array_buffer_data(&buffer).is_none() {
                return Err(ec.new_type_error("ArrayBufferView buffer is detached"));
            }
            Ok(Self {
                buffer,
                kind: ArrayBufferViewKind::from_typed_array_kind(kind),
                byte_offset: ec.typed_array_byte_offset(&typed_array)? as usize,
                byte_length: ec.typed_array_byte_length(&typed_array)? as usize,
            })
        } else {
            Err(ec.new_type_error("Expected an ArrayBufferView object"))
        }
    }

    pub(crate) fn new_uint8(buffer: JsArrayBuffer, byte_offset: usize, byte_length: usize) -> Self {
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

    pub(crate) fn bytes(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Vec<u8>, crate::js::Types> {
        let data = ec
            .array_buffer_data(&self.buffer)
            .ok_or_else(|| ec.new_type_error("ArrayBufferView buffer is detached"))?;
        Ok(data[self.byte_offset..self.byte_offset + self.byte_length].to_vec())
    }

    pub(crate) fn create_result_view(
        &self,
        byte_length: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        create_view_object(
            &self.kind,
            self.buffer.clone(),
            self.byte_offset,
            byte_length,
            ec,
        )
    }

    pub(crate) fn create_remaining_view(
        &self,
        bytes_filled: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        create_uint8_view_object(
            self.buffer.clone(),
            self.byte_offset + bytes_filled,
            self.byte_length.saturating_sub(bytes_filled),
            ec,
        )
    }

    #[allow(dead_code)]
    fn replace_with(&mut self, other: Self) {
        *self = other;
    }

    /// Spec step: `firstDescriptor.[[buffer]] = TransferArrayBuffer(view.[[ViewedArrayBuffer]])`.
    /// Updates only the backing buffer; `byte_offset` and `byte_length` are unchanged.
    fn transfer_buffer_from(&mut self, other: &Self) {
        self.buffer = other.buffer.clone();
    }
}

#[gc_struct]
enum PullRequest {
    Default(ReadRequest),
    Byob(ReadIntoRequest),
}

/// <https://streams.spec.whatwg.org/#pull-into-descriptor>
#[gc_struct]
struct PullIntoDescriptor {
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-buffer>
    view: ArrayBufferViewDescriptor,
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-bytes-filled>
    #[ignore_trace]
    bytes_filled: usize,
    /// <https://streams.spec.whatwg.org/#pull-into-descriptor-minimum-fill>
    #[ignore_trace]
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

    fn filled_view(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.view.create_result_view(self.bytes_filled, ec)
    }

    fn close(
        self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Default(read_request) => {
                let value = if self.bytes_filled == 0 {
                    None
                } else {
                    Some(JsValue::from(self.filled_view(ec)?))
                };
                let read_request = read_request.clone();
                if let Some(value) = value {
                    read_request.chunk_steps(value, ec)
                } else {
                    read_request.close_steps(ec)
                }
            }
            PullRequest::Byob(read_into_request) => {
                let value = JsValue::from(self.filled_view(ec)?);
                read_into_request.clone().close_steps(Some(value), ec)
            }
        }
    }

    fn cancel(
        self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Default(read_request) => read_request.clone().close_steps(ec),
            PullRequest::Byob(read_into_request) => read_into_request.clone().close_steps(None, ec),
        }
    }

    fn commit(
        self,
        done: bool,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let value = JsValue::from(self.filled_view(ec)?);
        self.commit_with_value(value, done, ec)
    }

    fn commit_with_value(
        self,
        value: JsValue,
        done: bool,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Default(read_request) => {
                let read_request = read_request.clone();
                if done {
                    read_request.chunk_steps(value, ec)
                } else {
                    read_request.chunk_steps(value, ec)
                }
            }
            PullRequest::Byob(read_into_request) => {
                let read_into_request = read_into_request.clone();
                if done {
                    read_into_request.close_steps(Some(value), ec)
                } else {
                    read_into_request.chunk_steps(value, ec)
                }
            }
        }
    }

    fn error(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Default(read_request) => read_request.clone().error_steps(error, ec),
            PullRequest::Byob(read_into_request) => {
                read_into_request.clone().error_steps(error, ec)
            }
        }
    }
}

#[gc_struct]
struct ByteQueueEntry {
    buffer: JsArrayBuffer,
    #[ignore_trace]
    byte_offset: usize,
    #[ignore_trace]
    byte_length: usize,
    #[ignore_trace]
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
#[gc_struct]
pub struct ReadableStreamBYOBRequest {
    /// <https://streams.spec.whatwg.org/#readablestreambyobrequest-controller>
    controller: GcCell<Option<ReadableByteStreamController>>,
    /// <https://streams.spec.whatwg.org/#readablestreambyobrequest-view>
    view: GcCell<Option<JsObject>>,
}

impl ReadableStreamBYOBRequest {
    pub(crate) fn new(controller: ReadableByteStreamController) -> Self {
        Self {
            controller: gc_cell_new(Some(controller)),
            view: gc_cell_new(None),
        }
    }

    fn controller_slot(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<ReadableByteStreamController, crate::js::Types> {
        self.controller
            .borrow()
            .clone()
            .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest is missing its controller"))
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-view>
    pub(crate) fn view(&self) -> Option<JsObject> {
        self.view.borrow().clone()
    }

    pub(crate) fn set_view_slot(&self, view: Option<JsObject>) {
        *self.view.borrow_mut() = view;
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-respond>
    pub(crate) fn respond(
        &self,
        bytes_written: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let controller = self.controller_slot(ec)?;
        controller.respond(bytes_written, ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-respond-with-new-view>
    pub(crate) fn respond_with_new_view(
        &self,
        view: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let view_object = view.as_object().ok_or_else(|| {
            ec.new_type_error("respondWithNewView() requires an ArrayBufferView object")
        })?;
        let controller = self.controller_slot(ec)?;
        let view_descriptor = ArrayBufferViewDescriptor::from_value(view, ec)?;
        controller.respond_with_new_view(view_descriptor, view_object, ec)
    }
}

/// <https://streams.spec.whatwg.org/#readablebytestreamcontroller>
#[gc_struct]
pub struct ReadableByteStreamController {
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-stream>
    stream: GcCell<Option<ReadableStream>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-queue>
    queue: GcCell<VecDeque<ByteQueueEntry>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-queuetotalsize>
    #[ignore_trace]
    queue_total_size: Rc<Cell<usize>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-started>
    #[ignore_trace]
    started: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-closerequested>
    #[ignore_trace]
    close_requested: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pullagain>
    #[ignore_trace]
    pull_again: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pulling>
    #[ignore_trace]
    pulling: Rc<Cell<bool>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-strategyhwm>
    #[ignore_trace]
    strategy_high_water_mark: Rc<Cell<f64>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-autoallocatechunksize>
    #[ignore_trace]
    auto_allocate_chunk_size: Rc<Cell<Option<usize>>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pullalgorithm>
    pull_algorithm: GcCell<Option<PullAlgorithm>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-cancelalgorithm>
    cancel_algorithm: GcCell<Option<CancelAlgorithm>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-pendingpullintos>
    pending_pull_intos: GcCell<VecDeque<PullIntoDescriptor>>,
    /// <https://streams.spec.whatwg.org/#readablebytestreamcontroller-byobrequest>
    byob_request_object: GcCell<Option<JsObject>>,
}

impl ReadableByteStreamController {
    pub(crate) fn new() -> Self {
        Self {
            stream: gc_cell_new(None),
            queue: gc_cell_new(VecDeque::new()),
            queue_total_size: Rc::new(Cell::new(0)),
            started: Rc::new(Cell::new(false)),
            close_requested: Rc::new(Cell::new(false)),
            pull_again: Rc::new(Cell::new(false)),
            pulling: Rc::new(Cell::new(false)),
            strategy_high_water_mark: Rc::new(Cell::new(0.0)),
            auto_allocate_chunk_size: Rc::new(Cell::new(None)),
            pull_algorithm: gc_cell_new(None),
            cancel_algorithm: gc_cell_new(None),
            pending_pull_intos: gc_cell_new(VecDeque::new()),
            byob_request_object: gc_cell_new(None),
        }
    }

    fn stream_slot(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<ReadableStream, crate::js::Types> {
        self.stream
            .borrow()
            .clone()
            .ok_or_else(|| ec.new_type_error("ReadableByteStreamController is missing its stream"))
    }

    fn controller_object(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.stream_slot(ec)?
            .controller_object_slot()
            .ok_or_else(|| {
                ec.new_type_error("ReadableByteStreamController is missing its JavaScript object")
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
    fn invalidate_byob_request(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if let Some(object) = self.byob_request_object.borrow_mut().take() {
            with_readable_stream_byob_request_ref(&object, ec, |request| {
                request.set_view_slot(None)
            })?;
        }
        Ok(())
    }

    fn update_byob_request_view(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let Some(object) = self.byob_request_object.borrow().clone() else {
            return Ok(());
        };
        let maybe_view = if let Some(descriptor) = self.pending_pull_intos.borrow().front() {
            Some(
                descriptor
                    .view
                    .create_remaining_view(descriptor.bytes_filled, ec)?,
            )
        } else {
            None
        };
        with_readable_stream_byob_request_ref(&object, ec, |request| {
            request.set_view_slot(maybe_view)
        })
    }

    pub(crate) fn pending_pull_intos_len(&self) -> usize {
        self.pending_pull_intos.borrow().len()
    }

    /// Returns a snapshot of the current BYOB request view as a JS value, without
    /// materialising a new BYOB request object.  Used by the byte-stream tee to
    /// inspect the pending pull-into view synchronously (non-spec helper).
    #[allow(dead_code)]
    pub(crate) fn byob_request_immediate(&self) -> Option<JsValue> {
        let pending = self.pending_pull_intos.borrow();
        let descriptor = pending.front()?;
        if let Some(ref obj) = *self.byob_request_object.borrow() {
            return Some(JsValue::from(obj.clone()));
        }
        let _ = descriptor;
        None
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-desired-size>
    pub(crate) fn desired_size(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<f64>, crate::js::Types> {
        match self.stream_slot(ec)?.state() {
            ReadableStreamState::Errored => Ok(None),
            ReadableStreamState::Closed => Ok(Some(0.0)),
            ReadableStreamState::Readable => Ok(Some(
                self.strategy_high_water_mark.get() - self.queue_total_size.get() as f64,
            )),
        }
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-byob-request>
    pub(crate) fn byob_request(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<JsObject>, crate::js::Types> {
        if self.pending_pull_intos.borrow().is_empty() {
            self.invalidate_byob_request(ec)?;
            return Ok(None);
        }

        if let Some(object) = self.byob_request_object.borrow().clone() {
            return Ok(Some(object));
        }

        let request = ReadableStreamBYOBRequest::new(self.clone());
        let object: JsObject =
            create_interface_instance::<crate::js::Types, ReadableStreamBYOBRequest>(request, ec)?
                .into();
        *self.byob_request_object.borrow_mut() = Some(object.clone());
        self.update_byob_request_view(ec)?;
        Ok(Some(object))
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-close>
    pub(crate) fn close(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        if self.close_requested.get() || stream.state() != ReadableStreamState::Readable {
            return Err(ec.new_type_error("The stream is not in a state that permits close"));
        }
        self.close_steps(ec)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-enqueue>
    pub(crate) fn enqueue(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        if self.close_requested.get() || stream.state() != ReadableStreamState::Readable {
            return Err(ec.new_type_error("The stream is not in a state that permits enqueue"));
        }
        self.enqueue_steps(chunk, ec)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-error>
    pub(crate) fn error(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        self.error_steps(error, ec)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-private-cancel>
    pub(crate) fn cancel_steps(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.reset_queue();
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut());
        self.invalidate_byob_request(ec)?;
        for descriptor in pending {
            descriptor.cancel(ec)?;
        }

        let cancel_algorithm = self.cancel_algorithm.borrow().clone();
        let result = match cancel_algorithm {
            Some(cancel_algorithm) => cancel_algorithm.call(reason, ec)?,
            None => resolved_promise(ec.value_undefined(), ec)?,
        };
        self.clear_algorithms();
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-private-pull>
    pub(crate) fn pull_steps(
        &self,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        if self.queue_total_size.get() > 0 {
            return self.fill_read_request_from_queue(stream, read_request, ec);
        }

        if let Some(auto_allocate_chunk_size) = self.auto_allocate_chunk_size.get() {
            let realm = ec.current_realm();
            let intrinsics = ec.realm_intrinsics(&realm);
            let buffer = ec.allocate_array_buffer(
                intrinsics.array_buffer,
                auto_allocate_chunk_size as u64,
                None,
            )?;
            let descriptor = PullIntoDescriptor {
                view: ArrayBufferViewDescriptor::new_uint8(buffer, 0, auto_allocate_chunk_size),
                bytes_filled: 0,
                minimum_fill: 1,
                request: PullRequest::Default(read_request),
            };
            self.pending_pull_intos.borrow_mut().push_back(descriptor);
            let _ = self.byob_request(ec)?;
            return self.call_pull_if_needed(ec);
        }

        readable_stream_add_read_request(stream, read_request, ec)?;
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-pull-into>
    pub(crate) fn pull_into(
        &self,
        view: ArrayBufferViewDescriptor,
        min: usize,
        read_into_request: ReadIntoRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        let mut descriptor = PullIntoDescriptor {
            minimum_fill: min * view.element_size(),
            view,
            bytes_filled: 0,
            request: PullRequest::Byob(read_into_request),
        };

        self.fill_pull_into_from_queue(&mut descriptor, ec)?;
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.

        if descriptor.can_commit() {
            return descriptor.commit(false, ec);
        }

        if self.close_requested.get() && self.queue_total_size.get() == 0 {
            if descriptor.bytes_filled % descriptor.view.element_size() != 0 {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    ec,
                )?;
                descriptor.error(error.clone(), ec)?;
                self.clear_algorithms();
                readable_stream_error(stream, error, ec)?;
                return Ok(());
            }

            self.clear_algorithms();
            descriptor.close(ec)?;
            readable_stream_close(stream, ec)?;
            return Ok(());
        }

        self.pending_pull_intos.borrow_mut().push_back(descriptor);
        let _ = self.byob_request(ec)?;
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamcontroller-releasesteps>
    pub(crate) fn release_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut());
        self.invalidate_byob_request(ec)?;
        let release_error = type_error_value("Reader was released", ec)?;
        for descriptor in pending {
            descriptor.error(release_error.clone(), ec)?;
        }
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-close>
    pub(crate) fn close_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
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
                let ec: &mut dyn ExecutionContext<crate::js::Types> = ec;
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    ec,
                )?;
                self.error_steps(error.clone(), ec)?;
                return Err(ec.new_type_error(
                    "Cannot close a byte stream with a partially filled typed array element",
                ));
            }

            self.close_requested.set(true);
            return Ok(());
        }

        self.clear_algorithms();
        readable_stream_close(stream, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue>
    pub(crate) fn enqueue_steps(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let view = ArrayBufferViewDescriptor::from_value(chunk, ec)?;
        let empty_view_err =
            ec.new_type_error("ReadableByteStreamController.enqueue() requires a non-empty view");
        if view.byte_length() == 0 {
            return Err(empty_view_err);
        }

        self.enqueue_chunk(view);
        self.process_pending_pull_intos_using_queue(ec)?;
        self.process_read_requests_using_queue(ec)?;
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-error>
    pub(crate) fn error_steps(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
        let stream = self.stream_slot(ec)?;
        if stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        self.reset_queue();
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut());
        self.invalidate_byob_request(ec)?;
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.

        for descriptor in pending {
            descriptor.error(error.clone(), ec)?;
        }
        self.clear_algorithms();
        readable_stream_error(stream, error, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond>
    pub(crate) fn respond(
        &self,
        bytes_written: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let err_no_pending = ec.new_type_error("There is no pending BYOB request to respond to");
        let err_too_large = ec.new_range_error("bytesWritten exceeds the available view size");
        let descriptor = {
            let mut pending = self.pending_pull_intos.borrow_mut();
            let descriptor = match pending.front_mut() {
                Some(desc) => desc,
                None => {
                    return Err(err_no_pending);
                }
            };

            if bytes_written > descriptor.remaining_byte_length() {
                return Err(err_too_large);
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
                self.update_byob_request_view(ec)?;
                self.call_pull_if_needed(ec)?;
                return Ok(());
            }
        };

        self.invalidate_byob_request(ec)?;
        let stream = self.stream_slot(ec)?;
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.

        if self.close_requested.get() {
            if descriptor.bytes_filled % descriptor.view.element_size() != 0 {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    ec,
                )?;
                self.error_steps(error, ec)?;
                return Ok(());
            }
            descriptor.close(ec)?;
        } else {
            descriptor.commit(false, ec)?;
        }

        if self.close_requested.get()
            && self.queue_total_size.get() == 0
            && self.pending_pull_intos.borrow().is_empty()
        {
            self.clear_algorithms();
            readable_stream_close(stream, ec)?;
            return Ok(());
        }

        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond-with-new-view>
    pub(crate) fn respond_with_new_view(
        &self,
        view: ArrayBufferViewDescriptor,
        _view_object: JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let bytes_written = view.byte_length();
        let err_no_pending = ec.new_type_error("There is no pending BYOB request to respond to");
        let err_offset =
            ec.new_range_error("respondWithNewView() must preserve the current byte offset");
        let err_large =
            ec.new_range_error("respondWithNewView() view is larger than the remaining request");
        let descriptor_to_commit = {
            let mut pending = self.pending_pull_intos.borrow_mut();
            let descriptor = match pending.front_mut() {
                Some(desc) => desc,
                None => {
                    return Err(err_no_pending);
                }
            };
            if view.byte_offset() != descriptor.view.byte_offset() + descriptor.bytes_filled {
                return Err(err_offset);
            }
            if view.byte_length() > descriptor.remaining_byte_length() {
                return Err(err_large);
            }

            descriptor.bytes_filled += bytes_written;
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
            self.update_byob_request_view(ec)?;
            self.call_pull_if_needed(ec)?;
            return Ok(());
        };

        self.invalidate_byob_request(ec)?;
        let stream = self.stream_slot(ec)?;
        let close_requested = self.close_requested.get();
        // Compute the result view before ec_to_ctx since create_result_view now takes ec.
        let result_view = descriptor
            .view
            .create_result_view(descriptor.bytes_filled, ec)?;
        let result_view_val = JsValue::from(result_view);
        if close_requested {
            if descriptor.bytes_filled % descriptor.view.element_size() != 0 {
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    ec,
                )?;
                self.error_steps(error, ec)?;
                return Ok(());
            }
            descriptor.commit_with_value(result_view_val, true, ec)?;
        } else {
            descriptor.commit_with_value(result_view_val, false, ec)?;
        }

        if self.close_requested.get()
            && self.queue_total_size.get() == 0
            && self.pending_pull_intos.borrow().is_empty()
        {
            self.clear_algorithms();
            readable_stream_close(stream, ec)?;
            return Ok(());
        }

        let ec: &mut dyn ExecutionContext<crate::js::Types> = ec;
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-call-pull-if-needed>
    pub(crate) fn call_pull_if_needed(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if !self.should_call_pull(ec)? {
            return Ok(());
        }
        if self.pulling.get() {
            self.pull_again.set(true);
            return Ok(());
        }

        self.pulling.set(true);
        let controller_object = self.controller_object(ec)?;
        let pull_algorithm = self.pull_algorithm.borrow().clone();
        let pull_promise = match pull_algorithm {
            Some(pull_algorithm) => pull_algorithm.call(&controller_object, ec)?,
            None => resolved_promise(ec.value_undefined(), ec)?,
        };

        let captured_controller = self.clone();
        let on_fulfilled =
            crate::js::builtin_with_captures(ec, captured_controller, pull_steps_on_fulfilled, 1);
        let on_rejected =
            crate::js::builtin_with_captures(ec, self.clone(), pull_steps_on_rejected, 1);

        let promise = <crate::js::Types as JsTypes>::object_as_promise(&pull_promise)
            .ok_or_else(|| ec.new_type_error("pull result is not a Promise"))?;
        ec.perform_promise_then(promise, Some(on_fulfilled), Some(on_rejected), None)?;
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-should-call-pull>
    fn should_call_pull(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        let stream = self.stream_slot(ec)?;
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

        Ok(self.desired_size(ec)?.is_some_and(|size| size > 0.0))
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue-chunk-to-queue>
    fn enqueue_chunk(&self, view: ArrayBufferViewDescriptor) {
        self.queue_total_size
            .set(self.queue_total_size.get() + view.byte_length());
        self.queue.borrow_mut().push_back(ByteQueueEntry::new(view));
    }

    fn dequeue_chunk_as_value(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let entry = self
            .queue
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| ec.new_type_error("Readable byte stream queue is empty"))?;
        let remaining_len = entry.remaining_len();
        let remaining_view = entry.remaining_view();
        self.queue_total_size
            .set(self.queue_total_size.get().saturating_sub(remaining_len));
        let result_view = remaining_view.create_result_view(remaining_len, ec)?;
        Ok(JsValue::from(result_view))
    }

    fn fill_read_request_from_queue(
        &self,
        stream: ReadableStream,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Use ec directly (no ec_to_ctx bridge needed)
        let chunk = self.dequeue_chunk_as_value(ec)?;
        read_request.chunk_steps(chunk, ec)?;
        if self.close_requested.get() && self.queue_total_size.get() == 0 {
            self.clear_algorithms();
            readable_stream_close(stream, ec)?;
        }
        Ok(())
    }

    fn process_read_requests_using_queue(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
        // readable_stream_fulfill_read_request and readable_stream_close still require &mut Context.
        while self.queue_total_size.get() > 0
            && stream
                .reader_slot()
                .and_then(|reader| reader.as_default_reader())
                .is_some()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            let chunk = self.dequeue_chunk_as_value(ec)?;
            readable_stream_fulfill_read_request(stream.clone(), chunk, false, ec)?;
        }

        if self.close_requested.get()
            && self.queue_total_size.get() == 0
            && self.pending_pull_intos.borrow().is_empty()
        {
            self.clear_algorithms();
            readable_stream_close(stream, ec)?;
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-fill-pull-into-descriptor-from-queue>
    fn fill_pull_into_from_queue(
        &self,
        descriptor: &mut PullIntoDescriptor,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let total_to_copy = descriptor
            .remaining_byte_length()
            .min(self.queue_total_size.get());
        if total_to_copy == 0 {
            return Ok(());
        }

        let mut copied = Vec::with_capacity(total_to_copy);
        let mut remaining = total_to_copy;
        {
            let mut queue = self.queue.borrow_mut();
            while remaining > 0 {
                let mut entry = queue
                    .pop_front()
                    .ok_or_else(|| ec.new_type_error("Readable byte stream queue is empty"))?;
                let to_take = remaining.min(entry.remaining_len());
                let start = entry.remaining_byte_offset();
                let bytes = {
                    let data = entry.buffer.data().ok_or_else(|| {
                        ec.new_type_error("Readable byte stream queue entry buffer is detached")
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
        let mut data = descriptor
            .view
            .buffer
            .data_mut()
            .ok_or_else(|| ec.new_type_error("BYOB request buffer is detached"))?;
        let start = descriptor.view.byte_offset() + descriptor.bytes_filled;
        let end = start + copied.len();
        data[start..end].copy_from_slice(&copied);
        descriptor.bytes_filled += copied.len();
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-process-pull-into-descriptors-using-queue>
    fn process_pending_pull_intos_using_queue(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        loop {
            if self.queue_total_size.get() == 0 {
                break;
            }
            let Some(mut descriptor) = self.pending_pull_intos.borrow_mut().pop_front() else {
                break;
            };
            self.fill_pull_into_from_queue(&mut descriptor, ec)?;
            if descriptor.can_commit() {
                self.invalidate_byob_request(ec)?;
                descriptor.commit(false, ec)?;
                continue;
            }
            self.pending_pull_intos.borrow_mut().push_front(descriptor);
            self.update_byob_request_view(ec)?;
            break;
        }
        Ok(())
    }
}

pub(crate) fn with_readable_byte_stream_controller_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableByteStreamController) -> R,
) -> Completion<R, crate::js::Types> {
    let ctrl_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableByteStreamController>());
    let controller = match ctrl_ref {
        Some(c) => c,
        None => return Err(ec.new_type_error("object is not a ReadableByteStreamController")),
    };
    Ok(f(controller))
}

pub(crate) fn with_readable_stream_byob_request_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableStreamBYOBRequest) -> R,
) -> Completion<R, crate::js::Types> {
    let req_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStreamBYOBRequest>());
    let request = match req_ref {
        Some(r) => r,
        None => return Err(ec.new_type_error("object is not a ReadableStreamBYOBRequest")),
    };
    Ok(f(request))
}

/// <https://streams.spec.whatwg.org/#set-up-readable-byte-stream-controller-from-underlying-source>
pub(crate) fn set_up_readable_byte_stream_controller_from_underlying_source(
    stream: ReadableStream,
    underlying_source_object: Option<JsObject>,
    high_water_mark: f64,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let controller = ReadableByteStreamController::new();
    let controller_object: JsObject = create_interface_instance::<
        crate::js::Types,
        ReadableByteStreamController,
    >(controller.clone(), ec)?
    .into();

    let mut start_algorithm = StartAlgorithm::ReturnUndefined;
    let mut pull_algorithm = PullAlgorithm::ReturnUndefined;
    let mut cancel_algorithm = CancelAlgorithm::ReturnUndefined;

    if let Some(start_method) =
        extract_source_method(underlying_source_object.as_ref(), "start", ec)?
    {
        start_algorithm = StartAlgorithm::JavaScript(start_method);
    }
    if let Some(pull_method) = extract_source_method(underlying_source_object.as_ref(), "pull", ec)?
    {
        pull_algorithm = PullAlgorithm::JavaScript(pull_method);
    }
    if let Some(cancel_method) =
        extract_source_method(underlying_source_object.as_ref(), "cancel", ec)?
    {
        cancel_algorithm = CancelAlgorithm::JavaScript(cancel_method);
    }

    let auto_allocate_chunk_size =
        extract_auto_allocate_chunk_size(underlying_source_object.as_ref(), ec)?;

    set_up_readable_byte_stream_controller(
        stream,
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        high_water_mark,
        auto_allocate_chunk_size,
        ec,
    )
}

/// <https://streams.spec.whatwg.org/#set-up-readable-byte-stream-controller>
pub(crate) fn set_up_readable_byte_stream_controller(
    _stream: ReadableStream,
    controller: ReadableByteStreamController,
    controller_object: &JsObject,
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: f64,
    auto_allocate_chunk_size: Option<usize>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    controller.close_requested.set(false);
    controller.started.set(false);
    controller.pull_again.set(false);
    controller.pulling.set(false);
    controller.strategy_high_water_mark.set(high_water_mark);
    controller
        .auto_allocate_chunk_size
        .set(auto_allocate_chunk_size);
    *controller.pull_algorithm.borrow_mut() = Some(pull_algorithm.clone());
    *controller.cancel_algorithm.borrow_mut() = Some(cancel_algorithm.clone());
    controller.pending_pull_intos.borrow_mut().clear();
    let start_result = start_algorithm.call(controller_object, ec)?;
    let start_promise = resolved_promise(start_result, ec)?;

    let on_fulfilled =
        crate::js::builtin_with_captures(ec, controller.clone(), setup_on_fulfilled, 1);
    let on_rejected = crate::js::builtin_with_captures(ec, controller, setup_on_rejected, 1);
    let start_js_promise = <crate::js::Types as JsTypes>::object_as_promise(&start_promise)
        .ok_or_else(|| ec.new_type_error("start result is not a Promise"))?;
    ec.perform_promise_then(
        start_js_promise,
        Some(on_fulfilled),
        Some(on_rejected),
        None,
    )?;
    Ok(())
}

pub(crate) fn extract_auto_allocate_chunk_size(
    source_object: Option<&JsObject>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<usize>, crate::js::Types> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    let value = js_engine::ExecutionContext::get(
        ec,
        source_object.clone(),
        ec.property_key_from_str("autoAllocateChunkSize"),
    )?;
    if value.is_undefined() {
        return Ok(None);
    }

    let number = ec.to_number(value.clone())?;
    if !number.is_finite() || number <= 0.0 || number.fract() != 0.0 {
        return Err(ec.new_type_error("autoAllocateChunkSize must be a positive integer"));
    }

    Ok(Some(number as usize))
}

fn create_view_object(
    kind: &ArrayBufferViewKind,
    buffer: JsArrayBuffer,
    byte_offset: usize,
    byte_length: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    match kind {
        ArrayBufferViewKind::DataView => {
            let dv =
                ec.construct_data_view_from_buffer(buffer, byte_offset as u64, byte_length as u64)?;
            Ok(JsObject::from(dv))
        }
        _ => {
            let element_type = kind
                .to_typed_array_element_type()
                .ok_or_else(|| ec.new_type_error("DataView cannot be constructed as TypedArray"))?;
            let ta = ec.construct_typed_array_view(
                element_type,
                buffer,
                byte_offset as u64,
                byte_length as u64,
            )?;
            Ok(JsObject::from(ta))
        }
    }
}

fn create_uint8_view_object(
    buffer: JsArrayBuffer,
    byte_offset: usize,
    byte_length: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let ta = ec.construct_typed_array_view(
        TypedArrayElementType::Uint8,
        buffer,
        byte_offset as u64,
        byte_length as u64,
    )?;
    Ok(JsObject::from(ta))
}

fn pull_steps_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.pulling.set(false);
    if captures.pull_again.get() {
        captures.pull_again.set(false);
        captures.call_pull_if_needed(ec)?;
    }
    Ok(JsValue::undefined())
}

fn pull_steps_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.error_steps(args.first().cloned().unwrap_or_default(), ec)?;
    Ok(JsValue::undefined())
}

fn setup_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.started.set(true);
    captures.call_pull_if_needed(ec)?;
    Ok(JsValue::undefined())
}

fn setup_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.error_steps(args.first().cloned().unwrap_or_default(), ec)?;
    Ok(JsValue::undefined())
}
