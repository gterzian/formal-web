use std::{cell::Cell, collections::VecDeque, rc::Rc};

use crate::js::{Types, create_builtin_fn_with_traced_captures};
type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
type ArrayBuffer = <Types as JsTypes>::ArrayBuffer;

use js_engine::{Completion, ExecutionContext, JsTypes, SharedMemoryOrder, TypedArrayElementType};

use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{rejected_promise, resolved_promise};
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

use super::{
    CancelAlgorithm, PullAlgorithm, ReadIntoRequest, ReadRequest, ReadableStream,
    ReadableStreamController, ReadableStreamState, StartAlgorithm, extract_source_method,
    readable_stream_add_read_request, readable_stream_close, readable_stream_error,
    readable_stream_fulfill_read_request, readable_stream_get_num_read_requests, type_error_value,
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
    Float16Array,
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
            Self::Float16Array => TypedArrayElementType::Float16,
            Self::DataView => return None,
        })
    }

    fn from_element_type(element_type: TypedArrayElementType) -> Self {
        match element_type {
            TypedArrayElementType::Int8 => Self::Int8Array,
            TypedArrayElementType::Uint8 => Self::Uint8Array,
            TypedArrayElementType::Uint8Clamped => Self::Uint8ClampedArray,
            TypedArrayElementType::Int16 => Self::Int16Array,
            TypedArrayElementType::Uint16 => Self::Uint16Array,
            TypedArrayElementType::Int32 => Self::Int32Array,
            TypedArrayElementType::Uint32 => Self::Uint32Array,
            TypedArrayElementType::BigInt64 => Self::BigInt64Array,
            TypedArrayElementType::BigUint64 => Self::BigUint64Array,
            TypedArrayElementType::Float32 => Self::Float32Array,
            TypedArrayElementType::Float64 => Self::Float64Array,
            TypedArrayElementType::Float16 => Self::Float16Array,
        }
    }

    fn element_size(&self) -> usize {
        match self {
            Self::DataView | Self::Int8Array | Self::Uint8Array | Self::Uint8ClampedArray => 1,
            Self::Int16Array | Self::Uint16Array => 2,
            Self::Int32Array | Self::Uint32Array | Self::Float32Array => 4,
            Self::BigInt64Array
            | Self::BigUint64Array
            | Self::Float64Array
            | Self::Float16Array => 8,
        }
    }
}

#[gc_struct]
pub(crate) struct ArrayBufferViewDescriptor {
    buffer: ArrayBuffer,
    kind: ArrayBufferViewKind,
    #[ignore_trace]
    byte_offset: usize,
    #[ignore_trace]
    byte_length: usize,
    /// <https://tc39.es/ecma262/#sec-arraybufferbytelength>
    #[ignore_trace]
    buffer_byte_length: usize,
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
            let byte_offset = ec.data_view_byte_offset(&data_view)? as usize;
            let byte_length = ec.data_view_byte_length(&data_view)? as usize;
            let buffer_byte_length = ec.array_buffer_byte_length(&buffer) as usize;
            return Ok(Self {
                buffer,
                kind: ArrayBufferViewKind::DataView,
                byte_offset,
                byte_length,
                buffer_byte_length,
            });
        }

        if let Some(typed_array) = <crate::js::Types as JsTypes>::object_as_typed_array(&object) {
            let element_type = ec
                .typed_array_element_type(&typed_array)
                .ok_or_else(|| ec.new_type_error("TypedArray view is missing its kind"))?;
            let buffer = ec.typed_array_buffer(&typed_array)?;
            if ec.array_buffer_data(&buffer).is_none() {
                return Err(ec.new_type_error("ArrayBufferView buffer is detached"));
            }
            let byte_offset = ec.typed_array_byte_offset(&typed_array)? as usize;
            let byte_length = ec.typed_array_byte_length(&typed_array)? as usize;
            let buffer_byte_length = ec.array_buffer_byte_length(&buffer) as usize;
            Ok(Self {
                buffer,
                kind: ArrayBufferViewKind::from_element_type(element_type),
                byte_offset,
                byte_length,
                buffer_byte_length,
            })
        } else {
            Err(ec.new_type_error("Expected an ArrayBufferView object"))
        }
    }

    pub(crate) fn new_uint8(buffer: ArrayBuffer, byte_offset: usize, byte_length: usize) -> Self {
        let buffer_byte_length = byte_offset + byte_length;
        Self {
            buffer,
            kind: ArrayBufferViewKind::Uint8Array,
            byte_offset,
            byte_length,
            buffer_byte_length,
        }
    }

    pub(crate) fn byte_length(&self) -> usize {
        self.byte_length
    }

    pub(crate) fn buffer_byte_length(&self) -> usize {
        self.buffer_byte_length
    }

    pub(crate) fn buffer(&self) -> &ArrayBuffer {
        &self.buffer
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

    /// Creates a result view over a specific (already transferred) buffer.
    /// Used by ConvertPullIntoDescriptor, which transfers the descriptor's
    /// buffer before constructing the view.
    pub(crate) fn create_result_view_on(
        &self,
        buffer: ArrayBuffer,
        byte_length: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        create_view_object(&self.kind, buffer, self.byte_offset, byte_length, ec)
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
}

#[gc_struct]
enum PullRequest {
    /// Reader type "default" — an auto-allocated pull-into descriptor whose
    /// read request lives on the stream's default reader.
    Default,
    /// Reader type "byob" — a BYOB read whose read-into request is attached
    /// to the descriptor.
    Byob(ReadIntoRequest),
    /// Reader type "none" — set by [[ReleaseSteps]] when a reader is released
    /// with a pending pull-into; respond()/enqueue() then feed the descriptor's
    /// filled bytes into the queue instead of resolving a reader.
    None,
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

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-convert-pull-into-descriptor>
    fn filled_view(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        // Step 5: Let buffer be ! TransferArrayBuffer(pullIntoDescriptor's buffer).
        let buffer = transfer_array_buffer(self.view.buffer.clone(), ec)?;
        // Step 6: Return ! Construct(view constructor, « buffer, byte offset, bytesFilled ÷ elementSize »).
        self.view
            .create_result_view_on(buffer, self.bytes_filled, ec)
    }

    fn close_with_value(
        self,
        value: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Byob(read_into_request) => {
                read_into_request.clone().close_steps(Some(value), ec)
            }
            PullRequest::Default | PullRequest::None => Ok(()),
        }
    }

    fn cancel(
        self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Byob(read_into_request) => read_into_request.clone().close_steps(None, ec),
            // Default-reader requests live on the stream and are resolved by
            // ReadableStreamClose during cancel.
            PullRequest::Default | PullRequest::None => Ok(()),
        }
    }

    fn error(
        self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        match &self.request {
            PullRequest::Byob(read_into_request) => {
                read_into_request.clone().error_steps(error, ec)
            }
            // Default-reader requests live on the stream and are resolved by
            // ReadableStreamError during erroring.
            PullRequest::Default | PullRequest::None => Ok(()),
        }
    }
}

#[gc_struct]
struct ByteQueueEntry {
    buffer: ArrayBuffer,
    #[ignore_trace]
    byte_offset: usize,
    #[ignore_trace]
    byte_length: usize,
    #[ignore_trace]
    offset: usize,
}

impl ByteQueueEntry {
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
    pub(crate) fn new(
        controller: ReadableByteStreamController,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Self {
        Self {
            controller: gc_cell_new(Some(controller), ec),
            view: gc_cell_new(None, ec),
        }
    }

    fn controller_slot(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<ReadableByteStreamController, crate::js::Types> {
        self.controller
            .borrow(ec)
            .clone()
            .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest is missing its controller"))
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-view>
    pub(crate) fn view(&self, ec: &mut dyn ExecutionContext<crate::js::Types>) -> Option<JsObject> {
        self.view.borrow(ec).clone()
    }

    pub(crate) fn set_view_slot(
        &self,
        view: Option<JsObject>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {
        *self.view.borrow_mut(ec) = view;
    }

    /// <https://streams.spec.whatwg.org/#rs-byob-request-respond>
    pub(crate) fn respond(
        &self,
        bytes_written: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: If this.[[controller]] is undefined, throw a TypeError exception.
        let controller = self.controller_slot(ec)?;
        // Step 2: If ! IsDetachedBuffer(this.[[view]].[[ArrayBuffer]]) is true, throw a TypeError exception.
        if let Some(view) = self.view.borrow(ec).clone()
            && let Some(buffer) = view_buffer_of_js_object(&view, ec)?
            && ec.array_buffer_data(&buffer).is_none()
        {
            return Err(ec.new_type_error(
                "ReadableStreamBYOBRequest.respond() requires a non-detached view buffer",
            ));
        }
        // Step 5: Perform ? ReadableByteStreamControllerRespond(this.[[controller]], bytesWritten).
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
        // Step 1: If this.[[controller]] is undefined, throw a TypeError exception.
        let controller = self.controller_slot(ec)?;
        let view_descriptor = ArrayBufferViewDescriptor::from_value(view, ec)?;
        // Step 2: If ! IsDetachedBuffer(view.[[ViewedArrayBuffer]]) is true, throw a TypeError exception.
        if ec.array_buffer_data(&view_descriptor.buffer).is_none() {
            return Err(ec.new_type_error(
                "ReadableStreamBYOBRequest.respondWithNewView() requires a non-detached view buffer",
            ));
        }
        // Step 3: Return ? ReadableByteStreamControllerRespondWithNewView(this.[[controller]], view).
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
    pub(crate) fn new(ec: &mut dyn ExecutionContext<crate::js::Types>) -> Self {
        Self {
            stream: gc_cell_new(None, ec),
            queue: gc_cell_new(VecDeque::new(), ec),
            queue_total_size: Rc::new(Cell::new(0)),
            started: Rc::new(Cell::new(false)),
            close_requested: Rc::new(Cell::new(false)),
            pull_again: Rc::new(Cell::new(false)),
            pulling: Rc::new(Cell::new(false)),
            strategy_high_water_mark: Rc::new(Cell::new(0.0)),
            auto_allocate_chunk_size: Rc::new(Cell::new(None)),
            pull_algorithm: gc_cell_new(None, ec),
            cancel_algorithm: gc_cell_new(None, ec),
            pending_pull_intos: gc_cell_new(VecDeque::new(), ec),
            byob_request_object: gc_cell_new(None, ec),
        }
    }

    fn stream_slot(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<ReadableStream, crate::js::Types> {
        self.stream
            .borrow(ec)
            .clone()
            .ok_or_else(|| ec.new_type_error("ReadableByteStreamController is missing its stream"))
    }

    fn controller_object(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.stream_slot(ec)?
            .controller_object_slot(ec)
            .ok_or_else(|| {
                ec.new_type_error("ReadableByteStreamController is missing its JavaScript object")
            })
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-clear-algorithms>
    fn clear_algorithms(&self, ec: &mut dyn ExecutionContext<crate::js::Types>) {
        *self.pull_algorithm.borrow_mut(ec) = None;
        *self.cancel_algorithm.borrow_mut(ec) = None;
    }

    /// <https://streams.spec.whatwg.org/#reset-queue>
    fn reset_queue(&self, ec: &mut dyn ExecutionContext<crate::js::Types>) {
        self.queue.borrow_mut(ec).clear();
        self.queue_total_size.set(0);
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-invalidate-byob-request>
    fn invalidate_byob_request(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if let Some(object) = { self.byob_request_object.borrow_mut(ec).take() } {
            with_readable_stream_byob_request_ref(&object, ec, |request, ec| {
                request.set_view_slot(None, ec)
            })?;
        }
        Ok(())
    }

    fn update_byob_request_view(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let Some(object) = self.byob_request_object.borrow(ec).clone() else {
            return Ok(());
        };
        // Clone the descriptor out of the borrow so the guard is released
        // before `create_remaining_view` runs: constructing the typed-array
        // view allocates, and a cppgc trace must not read the cell while a
        // borrow is live.
        let pending_view = self
            .pending_pull_intos
            .borrow(ec)
            .front()
            .map(|descriptor| (descriptor.view.clone(), descriptor.bytes_filled));
        let maybe_view = if let Some((view, bytes_filled)) = pending_view {
            Some(view.create_remaining_view(bytes_filled, ec)?)
        } else {
            None
        };
        with_readable_stream_byob_request_ref(&object, ec, |request, ec| {
            request.set_view_slot(maybe_view, ec)
        })
    }

    pub(crate) fn pending_pull_intos_len(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> usize {
        self.pending_pull_intos.borrow(ec).len()
    }

    /// Returns a snapshot of the current BYOB request view as a JS value, without
    /// materialising a new BYOB request object.  Used by the byte-stream tee to
    /// inspect the pending pull-into view synchronously (non-spec helper).
    #[allow(dead_code)]
    pub(crate) fn byob_request_immediate(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Option<JsValue> {
        let pending = self.pending_pull_intos.borrow(ec);
        let descriptor = pending.front()?;
        if let Some(ref obj) = *self.byob_request_object.borrow(ec) {
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
        if self.pending_pull_intos.borrow(ec).is_empty() {
            self.invalidate_byob_request(ec)?;
            return Ok(None);
        }

        if let Some(object) = self.byob_request_object.borrow(ec).clone() {
            return Ok(Some(object));
        }

        let request = ReadableStreamBYOBRequest::new(self.clone(), ec);
        let object: JsObject =
            create_interface_instance::<crate::js::Types, ReadableStreamBYOBRequest>(request, ec)?
                .into();
        *self.byob_request_object.borrow_mut(ec) = Some(object.clone());
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
        self.reset_queue(ec);
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut(ec));
        self.invalidate_byob_request(ec)?;
        for descriptor in pending {
            descriptor.cancel(ec)?;
        }

        let cancel_algorithm = self.cancel_algorithm.borrow(ec).clone();
        let result = match cancel_algorithm {
            Some(cancel_algorithm) => match cancel_algorithm.call(reason, ec) {
                Ok(promise) => promise,
                Err(error) => rejected_promise(error, ec)?,
            },
            None => resolved_promise(ec.value_undefined(), ec)?,
        };
        self.clear_algorithms(ec);
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#rbs-controller-private-pull>
    pub(crate) fn pull_steps(
        &self,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;

        // Step 3: If this.[[queueTotalSize]] > 0,
        if self.queue_total_size.get() > 0 {
            // Step 3.2: Perform ! ReadableByteStreamControllerFillReadRequestFromQueue(this, readRequest).
            return self.fill_read_request_from_queue(read_request, ec);
        }

        // Step 5: If autoAllocateChunkSize is not undefined,
        if let Some(auto_allocate_chunk_size) = self.auto_allocate_chunk_size.get() {
            // Step 5.1: Let buffer be Construct(%ArrayBuffer%, « autoAllocateChunkSize »).
            let realm = ec.current_realm();
            let intrinsics = ec.realm_intrinsics(&realm);
            let buffer = ec.allocate_array_buffer(
                intrinsics.array_buffer,
                auto_allocate_chunk_size as u64,
                None,
            )?;
            // Step 5.3: Let pullIntoDescriptor be a new pull-into descriptor with reader type "default".
            let descriptor = PullIntoDescriptor {
                view: ArrayBufferViewDescriptor::new_uint8(buffer, 0, auto_allocate_chunk_size),
                bytes_filled: 0,
                minimum_fill: 1,
                request: PullRequest::Default,
            };
            // Step 5.4: Append pullIntoDescriptor to this.[[pendingPullIntos]].
            self.pending_pull_intos.borrow_mut(ec).push_back(descriptor);
        }

        // Step 6: Perform ! ReadableStreamAddReadRequest(stream, readRequest).
        readable_stream_add_read_request(stream, read_request, ec)?;

        // Step 7: Perform ! ReadableByteStreamControllerCallPullIfNeeded(this).
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

        // Steps 2-7: Let elementSize be 1; ctor = %DataView%; if the view is a
        // typed array use its element size and constructor.  Let minimumFill be
        // min × elementSize.
        let element_size = view.element_size();
        let minimum_fill = min * element_size;
        let byte_offset = view.byte_offset();
        let byte_length = view.byte_length();

        // Step 10: Let bufferResult be TransferArrayBuffer(view.[[ViewedArrayBuffer]]).
        let buffer = match transfer_array_buffer(view.buffer.clone(), ec) {
            Ok(buffer) => buffer,
            // Step 11: If bufferResult is an abrupt completion,
            Err(error) => {
                // Step 11.1: Perform readIntoRequest's error steps, given bufferResult.[[Value]].
                read_into_request.error_steps(error, ec)?;
                // Step 11.2: Return.
                return Ok(());
            }
        };

        // Step 13: Let pullIntoDescriptor be a new pull-into descriptor with
        // reader type "byob".
        let mut descriptor = PullIntoDescriptor {
            minimum_fill,
            view: ArrayBufferViewDescriptor {
                buffer,
                kind: view.kind.clone(),
                byte_offset,
                byte_length,
                buffer_byte_length: view.buffer_byte_length(),
            },
            bytes_filled: 0,
            request: PullRequest::Byob(read_into_request),
        };

        // Step 14: If controller.[[pendingPullIntos]] is not empty,
        if !self.pending_pull_intos.borrow(ec).is_empty() {
            // Step 14.1: Append pullIntoDescriptor to controller.[[pendingPullIntos]].
            self.pending_pull_intos.borrow_mut(ec).push_back(descriptor);
            // Step 14.2: Perform ! ReadableStreamAddReadIntoRequest(stream, readIntoRequest).
            // Note: read-into requests live inside the descriptors in this
            // implementation, so the request is already attached.
            // Step 14.3: Return.
            return Ok(());
        }

        // Step 15: If stream.[[state]] is "closed",
        if stream.state() == ReadableStreamState::Closed {
            // Step 15.1: Let emptyView be ! Construct(ctor, « buffer, byte offset, 0 »).
            let empty_view = descriptor.view.create_result_view(0, ec)?;
            // Step 15.2: Perform readIntoRequest's close steps, given emptyView.
            descriptor.close_with_value(JsValue::from(empty_view), ec)?;
            return Ok(());
        }

        // Step 16: If controller.[[queueTotalSize]] > 0,
        if self.queue_total_size.get() > 0 {
            // Step 16.1: If ! FillPullIntoDescriptorFromQueue(controller, pullIntoDescriptor) is true,
            if self.fill_pull_into_from_queue(&mut descriptor, ec)? {
                // Step 16.1.1: Let filledView be ! ConvertPullIntoDescriptor(pullIntoDescriptor).
                // Step 16.1.2: Perform ! HandleQueueDrain(controller).
                self.handle_queue_drain(ec)?;
                // Step 16.1.3: Perform readIntoRequest's chunk steps, given filledView.
                // Note: done is always false here; the descriptor was never
                // committed through CommitPullIntoDescriptor.
                let filled_view = descriptor.filled_view(ec)?;
                if let PullRequest::Byob(read_into_request) = &descriptor.request {
                    read_into_request
                        .clone()
                        .chunk_steps(JsValue::from(filled_view), ec)?;
                }
                return Ok(());
            }

            // Step 16.2: If controller.[[closeRequested]] is true,
            if self.close_requested.get() {
                // Step 16.2.1: Let e be a TypeError exception.
                let error = type_error_value(
                    "Cannot fulfill a read request when the stream is closing",
                    ec,
                )?;
                // Step 16.2.2: Perform ! ReadableByteStreamControllerError(controller, e).
                self.error_steps(error.clone(), ec)?;
                // Step 16.2.3: Perform readIntoRequest's error steps, given e.
                descriptor.error(error, ec)?;
                return Ok(());
            }
        }

        // Step 17: Append pullIntoDescriptor to controller.[[pendingPullIntos]].
        self.pending_pull_intos.borrow_mut(ec).push_back(descriptor);
        // Step 19: Perform ! ReadableByteStreamControllerCallPullIfNeeded(controller).
        self.call_pull_if_needed(ec)
    }

    /// Errors all pending "byob" read-into requests with the given error.
    /// Corresponds to ! ReadableStreamBYOBReaderErrorReadIntoRequests in a
    /// model where the requests live inside the descriptors.  The descriptors
    /// themselves are left in [[pendingPullIntos]].
    pub(crate) fn error_pending_read_into_requests(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let pending = self.pending_pull_intos.borrow(ec).clone();
        for descriptor in pending {
            if matches!(descriptor.request, PullRequest::Byob(_)) {
                descriptor.error(error.clone(), ec)?;
            }
        }
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamcontroller-releasesteps>
    pub(crate) fn release_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: If this.[[pendingPullIntos]] is not empty,
        if !self.pending_pull_intos.borrow(ec).is_empty() {
            // Step 1.1: Let firstPendingPullInto be this.[[pendingPullIntos]][0].
            let mut pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut(ec));
            let mut first = pending.pop_front().expect("pending pull intos not empty");
            // Step 1.2: Set firstPendingPullInto's reader type to "none".
            first.request = PullRequest::None;
            // Step 1.3: Set this.[[pendingPullIntos]] to the list « firstPendingPullInto ».
            pending.clear();
            pending.push_front(first);
            *self.pending_pull_intos.borrow_mut(ec) = pending;
        }
        // Note: [[ReleaseSteps]] does not invalidate the BYOB request; the
        // pending request keeps its view so the underlying source can keep
        // filling the released reader's buffer.
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-close>
    pub(crate) fn close_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;

        // Step 2: If controller.[[closeRequested]] is true or stream.[[state]] is not "readable", return.
        if self.close_requested.get() || stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        // Step 3: If controller.[[queueTotalSize]] > 0,
        if self.queue_total_size.get() > 0 {
            // Step 3.1: Set controller.[[closeRequested]] to true.
            self.close_requested.set(true);
            return Ok(());
        }

        // Step 4: If controller.[[pendingPullIntos]] is not empty,
        if !self.pending_pull_intos.borrow(ec).is_empty() {
            // Step 4.1: Let firstPendingPullInto be controller.[[pendingPullIntos]][0].
            let misaligned = {
                let pending_pull_intos = self.pending_pull_intos.borrow(ec);
                pending_pull_intos.front().is_some_and(|descriptor| {
                    descriptor.bytes_filled % descriptor.view.element_size() != 0
                })
            };
            // Step 4.2: If the remainder after dividing firstPendingPullInto's
            // bytes filled by firstPendingPullInto's element size is not 0,
            if misaligned {
                // Step 4.2.1: Let e be a new TypeError exception.
                let error = type_error_value(
                    "Cannot close a byte stream with a partially filled typed array element",
                    ec,
                )?;
                // Step 4.2.2: Perform ! ReadableByteStreamControllerError(controller, e).
                self.error_steps(error.clone(), ec)?;
                // Step 4.2.3: Throw e.
                return Err(ec.new_type_error(
                    "Cannot close a byte stream with a partially filled typed array element",
                ));
            }
        }

        // Step 5: Perform ! ReadableByteStreamControllerClearAlgorithms(controller).
        self.clear_algorithms(ec);
        // Step 6: Perform ! ReadableStreamClose(stream).
        readable_stream_close(stream, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue>
    pub(crate) fn enqueue_steps(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;

        // Step 2: If controller.[[closeRequested]] is true or stream.[[state]] is not "readable", return.
        if self.close_requested.get() || stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        // Steps 3-5: Let buffer be chunk.[[ViewedArrayBuffer]]; byteOffset; byteLength.
        let view = ArrayBufferViewDescriptor::from_value(chunk, ec)?;
        let byte_offset = view.byte_offset();
        let byte_length = view.byte_length();

        // Step 6: If ! IsDetachedBuffer(buffer) is true, throw a TypeError exception.
        // (from_value above already rejects detached buffers.)

        // Zero-length chunks cannot be enqueued into a byte stream.
        if byte_length == 0 {
            return Err(ec.new_type_error(
                "ReadableByteStreamController.enqueue() requires a non-empty view",
            ));
        }

        // Step 7: Let transferredBuffer be ? TransferArrayBuffer(buffer).
        let transferred_buffer = transfer_array_buffer(view.buffer.clone(), ec)?;

        // Step 8: If controller.[[pendingPullIntos]] is not empty,
        if !self.pending_pull_intos.borrow(ec).is_empty() {
            // Clone the first descriptor's buffer and reader type out of the
            // cell so no borrow is live across the engine calls below.
            let (first_buffer, first_is_none) = {
                let pending = self.pending_pull_intos.borrow(ec);
                let first_pending = pending.front().expect("pending pull intos not empty");
                (
                    first_pending.view.buffer.clone(),
                    matches!(first_pending.request, PullRequest::None),
                )
            };

            // Step 8.2: If ! IsDetachedBuffer(firstPendingPullInto's buffer) is true,
            //           throw a TypeError exception.
            if ec.array_buffer_data(&first_buffer).is_none() {
                return Err(ec.new_type_error("Cannot enqueue with a detached BYOB request buffer"));
            }

            // Step 8.3: Perform ! ReadableByteStreamControllerInvalidateBYOBRequest(controller).
            self.invalidate_byob_request(ec)?;

            // Step 8.4: Set firstPendingPullInto's buffer to ! TransferArrayBuffer(firstPendingPullInto's buffer).
            let new_buffer = transfer_array_buffer(first_buffer, ec)?;
            let descriptor = {
                let mut pending = self.pending_pull_intos.borrow_mut(ec);
                let first_pending = pending.front_mut().expect("pending pull intos not empty");
                first_pending.view.buffer = new_buffer;
                if first_is_none {
                    // Step 8.5: If firstPendingPullInto's reader type is "none",
                    //           perform ? EnqueueDetachedPullIntoToQueue(controller, firstPendingPullInto).
                    Some(pending.pop_front().expect("pending pull intos not empty"))
                } else {
                    None
                }
            };
            if let Some(descriptor) = descriptor {
                self.enqueue_detached_pull_into_to_queue(descriptor, ec)?;
            }
        }

        // Step 9: If ! ReadableStreamHasDefaultReader(stream) is true,
        if stream
            .reader_slot(ec)
            .and_then(|reader| reader.as_default_reader())
            .is_some()
        {
            // Step 9.1: Perform ! ReadableByteStreamControllerProcessReadRequestsUsingQueue(controller).
            self.process_read_requests_using_queue(ec)?;

            // Step 9.2: If ! ReadableStreamGetNumReadRequests(stream) is 0,
            if readable_stream_get_num_read_requests(stream.clone(), ec) == 0 {
                // Step 9.2.2: Perform ! EnqueueChunkToQueue(controller, transferredBuffer, byteOffset, byteLength).
                self.enqueue_chunk_to_queue(transferred_buffer, byte_offset, byte_length, ec);
            } else {
                // Step 9.3: Otherwise,
                // Step 9.3.2: If controller.[[pendingPullIntos]] is not empty,
                if !self.pending_pull_intos.borrow(ec).is_empty() {
                    // Step 9.3.2.2: Perform ! ShiftPendingPullInto(controller).
                    let descriptor = self.shift_pending_pull_into(ec);
                    let _ = descriptor;
                }
                // Step 9.3.3: Let transferredView be ! Construct(%Uint8Array%, « transferredBuffer, byteOffset, byteLength »).
                let transferred_view =
                    create_uint8_view_object(transferred_buffer, byte_offset, byte_length, ec)?;
                // Step 9.3.4: Perform ! ReadableStreamFulfillReadRequest(stream, transferredView, false).
                readable_stream_fulfill_read_request(
                    stream,
                    JsValue::from(transferred_view),
                    false,
                    ec,
                )?;
            }
        } else if stream
            .reader_slot(ec)
            .and_then(|reader| reader.as_byob_reader())
            .is_some()
        {
            // Step 10: Otherwise, if ! ReadableStreamHasBYOBReader(stream) is true,
            // Step 10.1: Perform ! EnqueueChunkToQueue(controller, transferredBuffer, byteOffset, byteLength).
            self.enqueue_chunk_to_queue(transferred_buffer, byte_offset, byte_length, ec);

            // Step 10.2: Let filledPullIntos be the result of performing
            //            ! ProcessPullIntoDescriptorsUsingQueue(controller).
            let filled_pull_intos = self.process_pending_pull_intos_using_queue(ec)?;

            // Step 10.3: For each filledPullInto of filledPullIntos,
            //            perform ! CommitPullIntoDescriptor(stream, filledPullInto).
            for descriptor in filled_pull_intos {
                self.commit_pull_into_descriptor(descriptor, ec)?;
            }
        } else {
            // Step 11: Otherwise,
            // Step 11.2: Perform ! EnqueueChunkToQueue(controller, transferredBuffer, byteOffset, byteLength).
            self.enqueue_chunk_to_queue(transferred_buffer, byte_offset, byte_length, ec);
        }

        // Step 12: Perform ! ReadableByteStreamControllerCallPullIfNeeded(controller).
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-error>
    pub(crate) fn error_steps(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        if stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        self.reset_queue(ec);
        let pending = std::mem::take(&mut *self.pending_pull_intos.borrow_mut(ec));
        self.invalidate_byob_request(ec)?;

        for descriptor in pending {
            descriptor.error(error.clone(), ec)?;
        }
        self.clear_algorithms(ec);
        readable_stream_error(stream, error, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond>
    pub(crate) fn respond(
        &self,
        bytes_written: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: Assert: controller.[[pendingPullIntos]] is not empty.
        if self.pending_pull_intos.borrow(ec).is_empty() {
            return Err(ec.new_type_error("There is no pending BYOB request to respond to"));
        }

        // Step 2: Let firstDescriptor be controller.[[pendingPullIntos]][0].
        let (state, first_view) = {
            let pending = self.pending_pull_intos.borrow(ec);
            let first = pending.front().expect("pending pull intos not empty");
            (self.stream_slot(ec)?.state(), first.view.clone())
        };

        // Step 4: If state is "closed",
        if state == ReadableStreamState::Closed {
            // Step 4.1: If bytesWritten is not 0, throw a TypeError exception.
            if bytes_written != 0 {
                return Err(
                    ec.new_type_error("Cannot respond with a non-zero value to a closed stream")
                );
            }
        } else {
            // Step 5.2: If bytesWritten is 0, throw a TypeError exception.
            if bytes_written == 0 {
                return Err(ec.new_type_error("bytesWritten must be a positive integer"));
            }
            // Step 5.3: If firstDescriptor's bytes filled + bytesWritten >
            //           firstDescriptor's byte length, throw a RangeError exception.
            let bytes_filled = {
                let pending = self.pending_pull_intos.borrow(ec);
                pending
                    .front()
                    .expect("pending pull intos not empty")
                    .bytes_filled
            };
            if bytes_filled + bytes_written > first_view.byte_length() {
                return Err(ec.new_range_error("bytesWritten exceeds the available view size"));
            }
        }

        // Step 6: Set firstDescriptor's buffer to ! TransferArrayBuffer(firstDescriptor's buffer).
        let new_buffer = transfer_array_buffer(first_view.buffer.clone(), ec)?;
        {
            let mut pending = self.pending_pull_intos.borrow_mut(ec);
            pending
                .front_mut()
                .expect("pending pull intos not empty")
                .view
                .buffer = new_buffer;
        }

        // Step 7: Perform ? ReadableByteStreamControllerRespondInternal(controller, bytesWritten).
        self.respond_internal(bytes_written, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond-internal>
    fn respond_internal(
        &self,
        bytes_written: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: Let firstDescriptor be controller.[[pendingPullIntos]][0].
        let (first_descriptor, state) = {
            let pending = self.pending_pull_intos.borrow(ec);
            let first = pending.front().expect("pending pull intos not empty");
            (first.clone(), self.stream_slot(ec)?.state())
        };

        // Step 3: Perform ! ReadableByteStreamControllerInvalidateBYOBRequest(controller).
        self.invalidate_byob_request(ec)?;

        // Step 5: If state is "closed",
        if state == ReadableStreamState::Closed {
            // Step 5.2: Perform ! ReadableByteStreamControllerRespondInClosedState(controller, firstDescriptor).
            self.respond_in_closed_state(first_descriptor, ec)?;
        } else {
            // Step 6.3: Perform ? ReadableByteStreamControllerRespondInReadableState(controller, bytesWritten, firstDescriptor).
            self.respond_in_readable_state(bytes_written, first_descriptor, ec)?;
        }

        // Step 7: Perform ! ReadableByteStreamControllerCallPullIfNeeded(controller).
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond-in-closed-state>
    fn respond_in_closed_state(
        &self,
        first_descriptor: PullIntoDescriptor,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 2: If firstDescriptor's reader type is "none",
        //         perform ! ReadableByteStreamControllerShiftPendingPullInto(controller).
        if matches!(first_descriptor.request, PullRequest::None) {
            let _ = self.shift_pending_pull_into(ec);
        }

        // Step 3: Let stream be controller.[[stream]].
        let stream = self.stream_slot(ec)?;

        // Step 4: If ! ReadableStreamHasBYOBReader(stream) is true,
        let has_byob_reader = stream
            .reader_slot(ec)
            .and_then(|reader| reader.as_byob_reader())
            .is_some();
        if has_byob_reader {
            // Step 4.1: Let filledPullIntos be a new empty list.
            // Step 4.2: While filledPullIntos's size < ! ReadableStreamGetNumReadIntoRequests(stream),
            let num_read_into_requests = self.num_byob_pending_descriptors(ec);
            let mut filled_pull_intos = Vec::new();
            while filled_pull_intos.len() < num_read_into_requests {
                // Step 4.2.1: Let pullIntoDescriptor be ! ShiftPendingPullInto(controller).
                let descriptor = self.shift_pending_pull_into(ec);
                // Step 4.2.2: Append pullIntoDescriptor to filledPullIntos.
                filled_pull_intos.push(descriptor);
            }

            // Step 4.3: For each filledPullInto of filledPullIntos,
            //           perform ! CommitPullIntoDescriptor(stream, filledPullInto).
            for descriptor in filled_pull_intos {
                self.commit_pull_into_descriptor(descriptor, ec)?;
            }
        }
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond-in-readable-state>
    fn respond_in_readable_state(
        &self,
        bytes_written: usize,
        pull_into_descriptor: PullIntoDescriptor,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 2: Perform ! ReadableByteStreamControllerFillHeadPullIntoDescriptor(controller, bytesWritten, pullIntoDescriptor).
        let mut pull_into_descriptor = pull_into_descriptor;
        pull_into_descriptor.bytes_filled += bytes_written;
        {
            let mut pending = self.pending_pull_intos.borrow_mut(ec);
            let head = pending.front_mut().expect("pending pull intos not empty");
            head.bytes_filled = pull_into_descriptor.bytes_filled;
        }

        // Step 3: If pullIntoDescriptor's reader type is "none",
        if matches!(pull_into_descriptor.request, PullRequest::None) {
            // Step 3.1: Perform ? EnqueueDetachedPullIntoToQueue(controller, pullIntoDescriptor).
            // Note: the spec's algorithm shifts the descriptor first; it is at
            // the head of [[pendingPullIntos]] here.
            let _ = self.shift_pending_pull_into(ec);
            self.enqueue_detached_pull_into_to_queue(pull_into_descriptor, ec)?;
            // Step 3.2: Let filledPullIntos be the result of performing
            //           ! ProcessPullIntoDescriptorsUsingQueue(controller).
            let filled_pull_intos = self.process_pending_pull_intos_using_queue(ec)?;
            // Step 3.3: For each filledPullInto of filledPullIntos,
            //           perform ! CommitPullIntoDescriptor(controller.[[stream]], filledPullInto).
            for descriptor in filled_pull_intos {
                self.commit_pull_into_descriptor(descriptor, ec)?;
            }
            // Step 3.4: Return.
            return Ok(());
        }

        // Step 4: If pullIntoDescriptor's bytes filled < pullIntoDescriptor's minimum fill, return.
        let (bytes_filled, minimum_fill, byte_offset, element_size, buffer) = {
            let pending = self.pending_pull_intos.borrow(ec);
            let head = pending.front().expect("pending pull intos not empty");
            (
                head.bytes_filled,
                head.minimum_fill,
                head.view.byte_offset(),
                head.view.element_size(),
                head.view.buffer.clone(),
            )
        };
        if bytes_filled < minimum_fill {
            return Ok(());
        }

        // Step 5: Perform ! ReadableByteStreamControllerShiftPendingPullInto(controller).
        let descriptor = self.shift_pending_pull_into(ec);

        // Step 6: Let remainderSize be the remainder after dividing pullIntoDescriptor's
        //         bytes filled by pullIntoDescriptor's element size.
        let bytes_filled = descriptor.bytes_filled;
        let remainder_size = bytes_filled % element_size;

        // Step 7: If remainderSize > 0,
        if remainder_size > 0 {
            // Step 7.1: Let end be pullIntoDescriptor's byte offset + pullIntoDescriptor's bytes filled.
            let end = byte_offset + bytes_filled;
            // Step 7.2: Perform ? EnqueueClonedChunkToQueue(controller, buffer, end − remainderSize, remainderSize).
            self.enqueue_cloned_chunk_to_queue(buffer, end - remainder_size, remainder_size, ec)?;
        }

        // Step 8: Set pullIntoDescriptor's bytes filled to pullIntoDescriptor's bytes filled − remainderSize.
        let mut descriptor = descriptor;
        descriptor.bytes_filled = bytes_filled - remainder_size;

        // Step 9: Let filledPullIntos be the result of performing
        //         ! ProcessPullIntoDescriptorsUsingQueue(controller).
        let filled_pull_intos = self.process_pending_pull_intos_using_queue(ec)?;

        // Step 10: Perform ! CommitPullIntoDescriptor(controller.[[stream]], pullIntoDescriptor).
        self.commit_pull_into_descriptor(descriptor, ec)?;

        // Step 11: For each filledPullInto of filledPullIntos,
        //          perform ! CommitPullIntoDescriptor(controller.[[stream]], filledPullInto).
        for filled in filled_pull_intos {
            self.commit_pull_into_descriptor(filled, ec)?;
        }
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-respond-with-new-view>
    pub(crate) fn respond_with_new_view(
        &self,
        view: ArrayBufferViewDescriptor,
        _view_object: JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 2: Assert: ! IsDetachedBuffer(view.[[ViewedArrayBuffer]]) is false.
        if ec.array_buffer_data(&view.buffer).is_none() {
            return Err(ec.new_type_error(
                "ReadableStreamBYOBRequest.respondWithNewView() requires a non-detached view buffer",
            ));
        }

        // Step 3: Let firstDescriptor be controller.[[pendingPullIntos]][0].
        if self.pending_pull_intos.borrow(ec).is_empty() {
            return Err(ec.new_type_error("There is no pending BYOB request to respond to"));
        }

        // Step 4: Let state be controller.[[stream]].[[state]].
        let state = self.stream_slot(ec)?.state();

        // Step 5: If state is "closed",
        if state == ReadableStreamState::Closed {
            // Step 5.1: If view.[[ByteLength]] is not 0, throw a TypeError exception.
            if view.byte_length() != 0 {
                return Err(ec.new_type_error(
                    "Cannot respondWithNewView() with a non-empty view to a closed stream",
                ));
            }
        } else {
            // Step 6.2: If view.[[ByteLength]] is 0, throw a TypeError exception.
            if view.byte_length() == 0 {
                return Err(ec.new_type_error("respondWithNewView() requires a non-empty view"));
            }
        }

        let (byte_offset, bytes_filled, byte_length, buffer_byte_length) = {
            let pending = self.pending_pull_intos.borrow(ec);
            let first = pending.front().expect("pending pull intos not empty");
            (
                first.view.byte_offset(),
                first.bytes_filled,
                first.view.byte_length(),
                first.view.buffer_byte_length(),
            )
        };

        // Step 7: If firstDescriptor's byte offset + firstDescriptor's bytes filled
        //         is not view.[[ByteOffset]], throw a RangeError exception.
        if byte_offset + bytes_filled != view.byte_offset() {
            return Err(
                ec.new_range_error("respondWithNewView() must preserve the current byte offset")
            );
        }

        // Step 8: If firstDescriptor's buffer byte length is not
        //         view.[[ViewedArrayBuffer]].[[ByteLength]], throw a RangeError exception.
        if buffer_byte_length != view.buffer_byte_length() {
            return Err(
                ec.new_range_error("respondWithNewView() must preserve the buffer byte length")
            );
        }

        // Step 9: If firstDescriptor's bytes filled + view.[[ByteLength]] >
        //         firstDescriptor's byte length, throw a RangeError exception.
        if bytes_filled + view.byte_length() > byte_length {
            return Err(ec.new_range_error(
                "respondWithNewView() view is larger than the remaining request",
            ));
        }

        // Step 10: Let viewByteLength be view.[[ByteLength]].
        let view_byte_length = view.byte_length();

        // Step 11: Set firstDescriptor's buffer to ? TransferArrayBuffer(view.[[ViewedArrayBuffer]]).
        let new_buffer = transfer_array_buffer(view.buffer.clone(), ec)?;
        let new_buffer_byte_length = view.buffer_byte_length();
        {
            let mut pending = self.pending_pull_intos.borrow_mut(ec);
            let first = pending.front_mut().expect("pending pull intos not empty");
            first.view.buffer = new_buffer;
            first.view.buffer_byte_length = new_buffer_byte_length;
        }

        // Step 12: Perform ? ReadableByteStreamControllerRespondInternal(controller, viewByteLength).
        self.respond_internal(view_byte_length, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-shift-pending-pull-into>
    fn shift_pending_pull_into(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> PullIntoDescriptor {
        self.pending_pull_intos
            .borrow_mut(ec)
            .pop_front()
            .expect("pending pull intos must not be empty")
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-commit-pull-into-descriptor>
    fn commit_pull_into_descriptor(
        &self,
        descriptor: PullIntoDescriptor,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;

        // Step 3: Let done be false.
        // Step 4: If stream.[[state]] is "closed", set done to true.
        let done = stream.state() == ReadableStreamState::Closed;

        // Step 5: Let filledView be ! ConvertPullIntoDescriptor(pullIntoDescriptor).
        let filled_view = descriptor.filled_view(ec)?;

        match &descriptor.request {
            // Step 6: If reader type is "default",
            //         perform ! ReadableStreamFulfillReadRequest(stream, filledView, done).
            PullRequest::Default => {
                readable_stream_fulfill_read_request(stream, JsValue::from(filled_view), done, ec)
            }
            // Step 7: Otherwise (byob), perform ! ReadableStreamFulfillReadIntoRequest(stream, filledView, done).
            PullRequest::Byob(read_into_request) => {
                let read_into_request = read_into_request.clone();
                let value = JsValue::from(filled_view);
                if done {
                    read_into_request.close_steps(Some(value), ec)
                } else {
                    read_into_request.chunk_steps(value, ec)
                }
            }
            PullRequest::None => Ok(()),
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue-detached-pull-into-to-queue>
    ///
    /// Note: the spec's algorithm shifts the descriptor from
    /// [[pendingPullIntos]]; the caller performs that shift (the descriptor
    /// arrives here already removed), so the borrow never spans the engine
    /// calls inside EnqueueClonedChunkToQueue.
    fn enqueue_detached_pull_into_to_queue(
        &self,
        pull_into_descriptor: PullIntoDescriptor,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 2: If pullIntoDescriptor's bytes filled > 0, perform ?
        //         EnqueueClonedChunkToQueue(controller, buffer, byte offset, bytes filled).
        if pull_into_descriptor.bytes_filled > 0 {
            self.enqueue_cloned_chunk_to_queue(
                pull_into_descriptor.view.buffer.clone(),
                pull_into_descriptor.view.byte_offset(),
                pull_into_descriptor.bytes_filled,
                ec,
            )?;
        }
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue-cloned-chunk-to-queue>
    fn enqueue_cloned_chunk_to_queue(
        &self,
        buffer: ArrayBuffer,
        byte_offset: usize,
        byte_length: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: Let cloneResult be CloneArrayBuffer(buffer, byteOffset, byteLength, %ArrayBuffer%).
        let realm = ec.current_realm();
        let intrinsics = ec.realm_intrinsics(&realm);
        let cloned = ec.clone_array_buffer(
            buffer,
            byte_offset as u64,
            byte_length as u64,
            intrinsics.array_buffer,
        )?;

        // Step 3: Perform ! EnqueueChunkToQueue(controller, cloneResult.[[Value]], 0, byteLength).
        self.enqueue_chunk_to_queue(cloned, 0, byte_length, ec);
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-handle-queue-drain>
    fn handle_queue_drain(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 2: If controller.[[queueTotalSize]] is 0 and controller.[[closeRequested]] is true,
        if self.queue_total_size.get() == 0 && self.close_requested.get() {
            // Step 2.1: Perform ! ReadableByteStreamControllerClearAlgorithms(controller).
            self.clear_algorithms(ec);
            // Step 2.2: Perform ! ReadableStreamClose(controller.[[stream]]).
            let stream = self.stream_slot(ec)?;
            readable_stream_close(stream, ec)?;
        } else {
            // Step 3.1: Perform ! ReadableByteStreamControllerCallPullIfNeeded(controller).
            self.call_pull_if_needed(ec)?;
        }
        Ok(())
    }

    /// Number of pending pull-into descriptors with a "byob" read-into request.
    /// Corresponds to ! ReadableStreamGetNumReadIntoRequests(stream) in a
    /// model where the requests live inside the descriptors.
    fn num_byob_pending_descriptors(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> usize {
        self.pending_pull_intos
            .borrow(ec)
            .iter()
            .filter(|descriptor| matches!(descriptor.request, PullRequest::Byob(_)))
            .count()
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
        let pull_algorithm = self.pull_algorithm.borrow(ec).clone();
        let pull_promise_result = match pull_algorithm {
            Some(pull_algorithm) => pull_algorithm.call(&controller_object, ec),
            None => Ok(resolved_promise(ec.value_undefined(), ec)?),
        };
        let pull_promise = match pull_promise_result {
            Ok(promise) => promise,
            Err(error) => {
                self.error_steps(error.clone(), ec)?;
                rejected_promise(error, ec)?
            }
        };

        let name_key = ec.property_key_from_str("");
        let on_fulfilled = create_builtin_fn_with_traced_captures(
            ec,
            self.clone(),
            pull_steps_on_fulfilled,
            1,
            name_key.clone(),
            false,
        );
        let on_rejected = create_builtin_fn_with_traced_captures(
            ec,
            self.clone(),
            pull_steps_on_rejected,
            1,
            name_key,
            false,
        );

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

        // Step 1: If stream.[[state]] is not "readable", return false.
        // Step 2: If controller.[[closeRequested]] is true, return false.
        // Step 3: If controller.[[started]] is false, return false.
        if !self.started.get()
            || self.close_requested.get()
            || stream.state() != ReadableStreamState::Readable
        {
            return Ok(false);
        }

        // Step 4: If ! ReadableStreamHasDefaultReader(stream) is true and
        //         ! ReadableStreamGetNumReadRequests(stream) > 0, return true.
        if stream
            .reader_slot(ec)
            .and_then(|reader| reader.as_default_reader())
            .is_some()
            && readable_stream_get_num_read_requests(stream.clone(), ec) > 0
        {
            return Ok(true);
        }

        // Step 5: If ! ReadableStreamHasBYOBReader(stream) is true and
        //         ! ReadableStreamGetNumReadIntoRequests(stream) > 0, return true.
        if stream
            .reader_slot(ec)
            .and_then(|reader| reader.as_byob_reader())
            .is_some()
            && self.num_byob_pending_descriptors(ec) > 0
        {
            return Ok(true);
        }

        // Steps 6-8: If desiredSize > 0, return true.
        Ok(self.desired_size(ec)?.is_some_and(|size| size > 0.0))
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-enqueue-chunk-to-queue>
    fn enqueue_chunk_to_queue(
        &self,
        buffer: ArrayBuffer,
        byte_offset: usize,
        byte_length: usize,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {
        self.queue_total_size
            .set(self.queue_total_size.get() + byte_length);
        self.queue.borrow_mut(ec).push_back(ByteQueueEntry {
            buffer,
            byte_offset,
            byte_length,
            offset: 0,
        });
    }

    fn dequeue_chunk_as_value(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let entry = self.queue.borrow_mut(ec).pop_front();
        let entry =
            entry.ok_or_else(|| ec.new_type_error("Readable byte stream queue is empty"))?;
        let remaining_len = entry.remaining_len();
        let remaining_view = entry.remaining_view();
        self.queue_total_size
            .set(self.queue_total_size.get().saturating_sub(remaining_len));
        let result_view = remaining_view.create_result_view(remaining_len, ec)?;
        Ok(JsValue::from(result_view))
    }

    fn fill_read_request_from_queue(
        &self,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let chunk = self.dequeue_chunk_as_value(ec)?;
        self.handle_queue_drain(ec)?;
        read_request.chunk_steps(chunk, ec)
    }

    fn process_read_requests_using_queue(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        while self.queue_total_size.get() > 0
            && stream
                .reader_slot(ec)
                .and_then(|reader| reader.as_default_reader())
                .is_some()
            && readable_stream_get_num_read_requests(stream.clone(), ec) > 0
        {
            let chunk = self.dequeue_chunk_as_value(ec)?;
            readable_stream_fulfill_read_request(stream.clone(), chunk, false, ec)?;
        }

        // Note: unlike HandleQueueDrain, this algorithm must not trigger a new
        // pull while a read request is still pending; the caller (Enqueue)
        // fulfils pending requests immediately after this returns and then
        // performs CallPullIfNeeded itself.
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-fill-pull-into-descriptor-from-queue>
    fn fill_pull_into_from_queue(
        &self,
        descriptor: &mut PullIntoDescriptor,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        // Step 1: Let maxBytesToCopy be min(queueTotalSize, byte length − bytes filled).
        let max_bytes_to_copy = descriptor
            .remaining_byte_length()
            .min(self.queue_total_size.get());
        // Step 2: Let maxBytesFilled be bytes filled + maxBytesToCopy.
        let max_bytes_filled = descriptor.bytes_filled + max_bytes_to_copy;
        // Step 3: Let totalBytesToCopyRemaining be maxBytesToCopy.
        let mut total_bytes_to_copy_remaining = max_bytes_to_copy;
        // Step 4: Let ready be false.
        let mut ready = false;
        // Step 7: Let remainderBytes be maxBytesFilled % element size.
        let remainder_bytes = max_bytes_filled % descriptor.view.element_size();
        // Step 8: Let maxAlignedBytes be maxBytesFilled − remainderBytes.
        let max_aligned_bytes = max_bytes_filled - remainder_bytes;
        // Step 9: If maxAlignedBytes ≥ minimum fill,
        if max_aligned_bytes >= descriptor.minimum_fill {
            // Step 9.1: Set totalBytesToCopyRemaining to maxAlignedBytes − bytes filled.
            total_bytes_to_copy_remaining = max_aligned_bytes - descriptor.bytes_filled;
            // Step 9.2: Set ready to true.
            ready = true;
        }

        // Step 11: While totalBytesToCopyRemaining > 0,
        let mut copied_total = 0;
        while total_bytes_to_copy_remaining > 0 {
            // Pop one entry with the borrow held, then release it before any
            // engine call below: domain code must not depend on which engine
            // operations allocate (an allocating call can trigger a cppgc
            // trace that must not read the cell while the mutable borrow is
            // live), so the borrow never spans an engine call.
            let mut entry = {
                let mut queue = self.queue.borrow_mut(ec);
                match queue.pop_front() {
                    Some(entry) => entry,
                    None => {
                        drop(queue);
                        return Err(ec.new_type_error("Readable byte stream queue is empty"));
                    }
                }
            };
            // Step 11.2: Let bytesToCopy be min(totalBytesToCopyRemaining, headOfQueue's byte length).
            let to_take = total_bytes_to_copy_remaining.min(entry.remaining_len());
            // Step 11.8: Perform ! CopyDataBlockBytes(descriptorBuffer, destStart, queueBuffer, queueByteOffset, bytesToCopy).
            let start = entry.remaining_byte_offset();
            let bytes = match ec.array_buffer_data(&entry.buffer) {
                Some(data) => data[start..start + to_take].to_vec(),
                None => {
                    return Err(
                        ec.new_type_error("Readable byte stream queue entry buffer is detached")
                    );
                }
            };
            entry.offset += to_take;
            if entry.remaining_len() > 0 {
                self.queue.borrow_mut(ec).push_front(entry);
            }
            total_bytes_to_copy_remaining -= to_take;

            let dest_start = descriptor.view.byte_offset() + descriptor.bytes_filled;
            for (relative_index, byte) in bytes.iter().copied().enumerate() {
                let value = ec.value_from_number(f64::from(byte));
                ec.set_value_in_buffer(
                    &descriptor.view.buffer,
                    (dest_start + relative_index) as u64,
                    TypedArrayElementType::Uint8,
                    value,
                    true,
                    SharedMemoryOrder::Unordered,
                )?;
            }
            // Step 11.12: Perform ! FillHeadPullIntoDescriptor(controller, bytesToCopy, pullIntoDescriptor).
            descriptor.bytes_filled += to_take;
            copied_total += to_take;
        }

        // Step 11.11: Set controller.[[queueTotalSize]] to controller.[[queueTotalSize]] − bytesToCopy.
        self.queue_total_size
            .set(self.queue_total_size.get().saturating_sub(copied_total));

        // Step 13: Return ready.
        Ok(ready)
    }

    /// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-process-pull-into-descriptors-using-queue>
    fn process_pending_pull_intos_using_queue(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Vec<PullIntoDescriptor>, crate::js::Types> {
        // Step 2: Let filledPullIntos be a new empty list.
        let mut filled_pull_intos: Vec<PullIntoDescriptor> = Vec::new();
        loop {
            // Step 3.1: If controller.[[queueTotalSize]] is 0, break.
            if self.queue_total_size.get() == 0 {
                break;
            }
            // Step 3.2: Let pullIntoDescriptor be controller.[[pendingPullIntos]][0].
            let mut popped = self.pending_pull_intos.borrow_mut(ec).pop_front();
            let Some(descriptor) = popped.as_mut() else {
                break;
            };
            // Step 3.3: If ! FillPullIntoDescriptorFromQueue(controller, pullIntoDescriptor) is true,
            if self.fill_pull_into_from_queue(descriptor, ec)? {
                // Step 3.3.1: Perform ! ShiftPendingPullInto(controller).
                // Step 3.3.2: Append pullIntoDescriptor to filledPullIntos.
                filled_pull_intos.push(popped.take().unwrap());
                continue;
            }
            // Not ready — push back and stop.
            self.pending_pull_intos
                .borrow_mut(ec)
                .push_front(popped.take().unwrap());
            self.update_byob_request_view(ec)?;
            break;
        }

        // Step 4: Return filledPullIntos.
        Ok(filled_pull_intos)
    }
}

pub(crate) fn with_readable_byte_stream_controller_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableByteStreamController, &mut dyn ExecutionContext<crate::js::Types>) -> R,
) -> Completion<R, crate::js::Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let controller = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableByteStreamController>().cloned());
    let Some(controller) = controller else {
        return Err(ec.new_type_error("object is not a ReadableByteStreamController"));
    };
    Ok(f(&controller, ec))
}

pub(crate) fn with_readable_stream_byob_request_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableStreamBYOBRequest, &mut dyn ExecutionContext<crate::js::Types>) -> R,
) -> Completion<R, crate::js::Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let request = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStreamBYOBRequest>().cloned());
    let Some(request) = request else {
        return Err(ec.new_type_error("object is not a ReadableStreamBYOBRequest"));
    };
    Ok(f(&request, ec))
}

/// <https://streams.spec.whatwg.org/#set-up-readable-byte-stream-controller-from-underlying-source>
pub(crate) fn set_up_readable_byte_stream_controller_from_underlying_source(
    stream: ReadableStream,
    underlying_source_object: Option<JsObject>,
    high_water_mark: f64,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let controller = ReadableByteStreamController::new(ec);
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
    stream: ReadableStream,
    controller: ReadableByteStreamController,
    controller_object: &JsObject,
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: f64,
    auto_allocate_chunk_size: Option<usize>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 2 (implicit): Set controller.[[stream]] to stream.
    *controller.stream.borrow_mut(ec) = Some(stream.clone());

    // Step 3 (implicit): Set stream.[[controller]] to controller.
    stream.set_controller_slot(Some(ReadableStreamController::Byte(controller.clone())), ec);
    stream.set_controller_object_slot(Some(controller_object.clone()), ec);

    controller.close_requested.set(false);
    controller.started.set(false);
    controller.pull_again.set(false);
    controller.pulling.set(false);
    controller.strategy_high_water_mark.set(high_water_mark);
    controller
        .auto_allocate_chunk_size
        .set(auto_allocate_chunk_size);
    *controller.pull_algorithm.borrow_mut(ec) = Some(pull_algorithm.clone());
    *controller.cancel_algorithm.borrow_mut(ec) = Some(cancel_algorithm.clone());
    controller.pending_pull_intos.borrow_mut(ec).clear();
    let start_result = start_algorithm.call(controller_object, ec)?;
    let start_promise = resolved_promise(start_result, ec)?;

    let name_key = ec.property_key_from_str("");
    let on_fulfilled = create_builtin_fn_with_traced_captures(
        ec,
        controller.clone(),
        setup_on_fulfilled,
        1,
        name_key.clone(),
        false,
    );
    let on_rejected = create_builtin_fn_with_traced_captures(
        ec,
        controller,
        setup_on_rejected,
        1,
        name_key,
        false,
    );
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

/// <https://tc39.es/ecma262/#sec-transferarraybuffer>
pub(crate) fn transfer_array_buffer(
    buffer: ArrayBuffer,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ArrayBuffer, crate::js::Types> {
    // Step 1: Assert: IsDetachedBuffer(arrayBuffer) is false.
    // Step 2: If IsSharedArrayBuffer(arrayBuffer) is true, throw a TypeError exception.
    // Step 4: If IsFixedLengthArrayBuffer(arrayBuffer) is false, throw a TypeError exception.
    if !ec.can_transfer_array_buffer(&buffer) {
        return Err(ec.new_type_error(
            "ArrayBuffer cannot be transferred (it may be shared, detached, or backed by WebAssembly memory)",
        ));
    }
    // Step 5: Let newBuffer be ? AllocateArrayBuffer(%ArrayBuffer%, arrayBuffer.[[ArrayBufferByteLength]]).
    // Steps 6-7: Copy the source data block into newBuffer.
    // Step 8: Perform ! DetachArrayBuffer(arrayBuffer, key).
    // Step 9: Return newBuffer.
    let byte_length = ec.array_buffer_byte_length(&buffer);
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let new_buffer =
        ec.clone_array_buffer(buffer.clone(), 0, byte_length, intrinsics.array_buffer)?;
    ec.detach_array_buffer(buffer, None)?;
    Ok(new_buffer)
}

/// Returns the backing ArrayBuffer of a typed array or DataView JS object,
/// or None if the object is neither.  Unlike `ArrayBufferViewDescriptor::from_value`
/// this does not reject detached buffers, so callers can run IsDetachedBuffer
/// themselves.
fn view_buffer_of_js_object(
    view: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<ArrayBuffer>, crate::js::Types> {
    if let Some(typed_array) = <crate::js::Types as JsTypes>::object_as_typed_array(view) {
        return Ok(Some(ec.typed_array_buffer(&typed_array)?));
    }
    if let Some(data_view) = <crate::js::Types as JsTypes>::object_as_data_view(view) {
        return Ok(Some(ec.data_view_buffer(&data_view)?));
    }
    Ok(None)
}

fn create_view_object(
    kind: &ArrayBufferViewKind,
    buffer: ArrayBuffer,
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
    buffer: ArrayBuffer,
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
    Ok(ec.value_undefined())
}

fn pull_steps_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.error_steps(
        args.first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn setup_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.started.set(true);
    captures.call_pull_if_needed(ec)?;
    Ok(ec.value_undefined())
}

fn setup_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &ReadableByteStreamController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.error_steps(
        args.first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}
