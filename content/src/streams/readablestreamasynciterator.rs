use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use std::{cell::Cell, rc::Rc};

use crate::webidl::{AsyncValueIterable, rejected_promise, resolved_promise};

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

use super::{ReadableStream, ReadableStreamDefaultReader, ReadableStreamGenericReader};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

#[gc_struct]
pub(crate) struct ReadableStreamAsyncIteratorState {
    /// <https://streams.spec.whatwg.org/#readablestream-async-iterator-reader>
    reader: GcCell<Option<ReadableStreamDefaultReader>>,

    /// <https://streams.spec.whatwg.org/#readablestream-async-iterator-prevent-cancel>
    #[ignore_trace]
    prevent_cancel: Rc<Cell<bool>>,
}

impl ReadableStreamAsyncIteratorState {
    fn new(reader: ReadableStreamDefaultReader, prevent_cancel: bool) -> Self {
        Self {
            reader: gc_cell_new(Some(reader)),
            prevent_cancel: Rc::new(Cell::new(prevent_cancel)),
        }
    }

    fn reader(&self) -> Option<ReadableStreamDefaultReader> {
        self.reader.borrow().clone()
    }

    fn finish(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let Some(reader) = self.reader.borrow().clone() else {
            return Ok(());
        };

        if reader.stream_slot_value().is_some() {
            reader.release_lock(ec)?;
        }

        *self.reader.borrow_mut() = None;
        Ok(())
    }
}

impl AsyncValueIterable for ReadableStream {
    type State = ReadableStreamAsyncIteratorState;

    /// <https://streams.spec.whatwg.org/#rs-get-iterator>
    fn create_async_iterator_state(
        &self,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self::State, Types> {
        let mut stream = self.clone();

        // Step 1: "Let reader be ? AcquireReadableStreamDefaultReader(stream)."
        let reader_object = stream.get_reader(&ec.value_undefined(), ec)?;

        // Note: get_reader returns the generic JsObject type. For Boa,
        // this IS boa_engine::JsObject, so we can transmute to access
        // the domain-specific downcast.
        let reader = {
            // SAFETY: <Types as JsTypes>::JsObject is boa_engine::object::JsObject
            // for the Boa backend. This cast lets us access downcast_ref.
            let boa_obj: &JsObject = unsafe {
                &*(&reader_object as *const <Types as js_engine::JsTypes>::JsObject
                    as *const JsObject)
            };
            boa_obj
                .downcast_ref::<ReadableStreamDefaultReader>()
                .ok_or_else(|| {
                    ec.new_type_error("ReadableStream async iterator requires a default reader")
                })?
                .clone()
        };

        // Step 3: "Let preventCancel be args[0][\"preventCancel\"]."
        let value = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());
        let prevent_cancel = iterator_prevent_cancel(&value, ec)?;

        // Step 4: "Set iterator's prevent cancel to preventCancel."
        Ok(ReadableStreamAsyncIteratorState::new(
            reader,
            prevent_cancel,
        ))
    }

    /// <https://streams.spec.whatwg.org/#rs-asynciterator-prototype-next>
    fn get_next_iteration_result(
        &self,
        state: &Self::State,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as js_engine::JsTypes>::JsObject, Types> {
        let reader = state.reader().ok_or_else(|| {
            ec.new_type_error("ReadableStream async iterator is missing its reader")
        })?;

        reader.read(ec)
    }

    fn finish_async_iterator(
        &self,
        state: &Self::State,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        state.finish(ec)
    }

    fn has_async_iterator_return() -> bool {
        true
    }

    /// <https://streams.spec.whatwg.org/#rs-asynciterator-prototype-return>
    fn return_async_iterator(
        &self,
        state: &Self::State,
        value: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as js_engine::JsTypes>::JsObject, Types> {
        let Some(reader) = state.reader() else {
            return resolved_promise(ec.value_undefined(), ec);
        };

        if state.prevent_cancel.get() {
            state.finish(ec)?;
            return resolved_promise(ec.value_undefined(), ec);
        }

        let cancel_promise = reader
            .cancel(value, ec)
            .or_else(|error_value| rejected_promise(error_value, ec))?;

        state.finish(ec)?;
        Ok(cancel_promise)
    }
}

fn iterator_prevent_cancel(
    options: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types> {
    if JsValue::is_undefined(options) || options.is_null() {
        return Ok(false);
    }

    let options_obj = ec.to_object(options.clone())?;
    let prevent_val = js_engine::EcmascriptHost::get(ec, &options_obj, "preventCancel")?;
    Ok(ec.to_boolean(&prevent_val))
}
