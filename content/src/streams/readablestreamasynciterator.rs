use std::{cell::Cell, rc::Rc};

use boa_engine::{Context, JsArgs, JsNativeError, JsResult, JsValue, js_string, object::JsObject};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::webidl::{AsyncValueIterable, rejected_promise, resolved_promise};

use super::{ReadableStream, ReadableStreamDefaultReader, ReadableStreamGenericReader};

#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadableStreamAsyncIteratorState {
    /// <https://streams.spec.whatwg.org/#readablestream-async-iterator-reader>
    reader: Gc<GcRefCell<Option<ReadableStreamDefaultReader>>>,

    /// <https://streams.spec.whatwg.org/#readablestream-async-iterator-prevent-cancel>
    #[unsafe_ignore_trace]
    prevent_cancel: Rc<Cell<bool>>,
}

impl ReadableStreamAsyncIteratorState {
    fn new(reader: ReadableStreamDefaultReader, prevent_cancel: bool) -> Self {
        Self {
            reader: Gc::new(GcRefCell::new(Some(reader))),
            prevent_cancel: Rc::new(Cell::new(prevent_cancel)),
        }
    }

    fn reader(&self) -> Option<ReadableStreamDefaultReader> {
        self.reader.borrow().clone()
    }

    fn finish(&self, context: &mut Context) -> JsResult<()> {
        let Some(reader) = self.reader.borrow().clone() else {
            return Ok(());
        };

        if reader.stream_slot_value().is_some() {
            reader.release_lock(context)?;
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
        context: &mut Context,
    ) -> JsResult<Self::State> {
        let mut stream = self.clone();

        // Step 1: "Let reader be ? AcquireReadableStreamDefaultReader(stream)."
        let reader_object = stream.get_reader(&JsValue::undefined(), context)?;

        let reader = reader_object
            .downcast_ref::<ReadableStreamDefaultReader>()
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("ReadableStream async iterator requires a default reader")
            })?
            .clone();

        // Step 3: "Let preventCancel be args[0][\"preventCancel\"]."
        let prevent_cancel = iterator_prevent_cancel(args.get_or_undefined(0), context)?;

        // Step 4: "Set iterator's prevent cancel to preventCancel."
        Ok(ReadableStreamAsyncIteratorState::new(reader, prevent_cancel))
    }

    /// <https://streams.spec.whatwg.org/#rs-asynciterator-prototype-next>
    fn get_next_iteration_result(
        &self,
        state: &Self::State,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        let reader = state.reader().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream async iterator is missing its reader")
        })?;

        reader.read(context)
    }

    fn finish_async_iterator(
        &self,
        state: &Self::State,
        context: &mut Context,
    ) -> JsResult<()> {
        state.finish(context)
    }

    fn has_async_iterator_return() -> bool {
        true
    }

    /// <https://streams.spec.whatwg.org/#rs-asynciterator-prototype-return>
    fn return_async_iterator(
        &self,
        state: &Self::State,
        value: JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        let Some(reader) = state.reader() else {
            return resolved_promise(JsValue::undefined(), context);
        };

        if state.prevent_cancel.get() {
            state.finish(context)?;
            return resolved_promise(JsValue::undefined(), context);
        }

        let cancel_promise = reader.cancel(value, context).or_else(|error| {
            rejected_promise(error.into_opaque(context)?, context)
        })?;

        state.finish(context)?;
        Ok(cancel_promise)
    }
}

fn iterator_prevent_cancel(options: &JsValue, context: &mut Context) -> JsResult<bool> {
    if options.is_undefined() || options.is_null() {
        return Ok(false);
    }

    let options_object = options.to_object(context)?;
    Ok(options_object
        .get(js_string!("preventCancel"), context)?
        .to_boolean())
}