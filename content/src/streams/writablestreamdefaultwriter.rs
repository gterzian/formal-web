use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    class::Class,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::webidl::{mark_promise_as_handled, rejected_promise, resolved_promise};

use super::{
    WritableStream, WritableStreamState, WritableStreamWriter, rejected_type_error_promise,
    type_error_value, with_writable_stream_ref,
    writable_stream_default_controller_get_chunk_size,
    writable_stream_default_controller_get_desired_size,
    writable_stream_default_controller_write,
};

/// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct WritableStreamDefaultWriter {
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-stream>
    stream: Gc<GcRefCell<Option<WritableStream>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-readypromise>
    ready_promise: Gc<GcRefCell<Option<JsObject>>>,
    ready_resolvers: Gc<GcRefCell<Option<ResolvingFunctions>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-closedpromise>
    closed_promise: Gc<GcRefCell<Option<JsObject>>>,
    closed_resolvers: Gc<GcRefCell<Option<ResolvingFunctions>>>,
}

impl WritableStreamDefaultWriter {
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            stream: Gc::new(GcRefCell::new(None)),
            ready_promise: Gc::new(GcRefCell::new(None)),
            ready_resolvers: Gc::new(GcRefCell::new(None)),
            closed_promise: Gc::new(GcRefCell::new(None)),
            closed_resolvers: Gc::new(GcRefCell::new(None)),
        }
    }
    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }
    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultWriter is missing its JavaScript object")
                .into()
        })
    }
    pub(crate) fn stream_slot_value(&self) -> Option<WritableStream> {
        self.stream.borrow().clone()
    }
    pub(crate) fn set_stream_slot_value(&self, stream: Option<WritableStream>) {
        *self.stream.borrow_mut() = stream;
    }
    pub(crate) fn ready_promise_value(&self) -> Option<JsObject> {
        self.ready_promise.borrow().clone()
    }
    pub(crate) fn set_ready_promise_value(&self, promise: Option<JsObject>) {
        *self.ready_promise.borrow_mut() = promise;
    }
    pub(crate) fn ready_resolvers_value(&self) -> Option<ResolvingFunctions> {
        self.ready_resolvers.borrow().clone()
    }
    pub(crate) fn set_ready_resolvers_value(&self, resolvers: Option<ResolvingFunctions>) {
        *self.ready_resolvers.borrow_mut() = resolvers;
    }
    pub(crate) fn closed_promise_value(&self) -> Option<JsObject> {
        self.closed_promise.borrow().clone()
    }
    pub(crate) fn set_closed_promise_value(&self, promise: Option<JsObject>) {
        *self.closed_promise.borrow_mut() = promise;
    }
    pub(crate) fn closed_resolvers_value(&self) -> Option<ResolvingFunctions> {
        self.closed_resolvers.borrow().clone()
    }
    pub(crate) fn set_closed_resolvers_value(&self, resolvers: Option<ResolvingFunctions>) {
        *self.closed_resolvers.borrow_mut() = resolvers;
    }

    /// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-writer>
    pub(crate) fn set_up_writable_stream_default_writer(
        &self,
        stream: WritableStream,
        context: &mut Context,
    ) -> JsResult<()> {
        if stream.is_writable_stream_locked() {
            return Err(JsNativeError::typ()
                .with_message("Cannot create a writer for a stream that already has a writer")
                .into());
        }

        self.set_stream_slot_value(Some(stream.clone()));
        stream.set_writer_slot(Some(WritableStreamWriter::Default(self.clone())));

        match stream.state() {
            WritableStreamState::Writable => {
                if !stream.close_queued_or_in_flight() && stream.backpressure() {
                    self.reset_ready_promise(context)?;
                } else {
                    self.resolve_ready_promise(context)?;
                }
                self.reset_closed_promise(context);
            }
            WritableStreamState::Erroring => {
                self.reject_ready_promise(stream.stored_error(), context)?;
                self.reset_closed_promise(context);
            }
            WritableStreamState::Closed => {
                self.resolve_ready_promise(context)?;
                self.resolve_closed_promise(context)?;
            }
            WritableStreamState::Errored => {
                let stored_error = stream.stored_error();
                self.reject_ready_promise(stored_error.clone(), context)?;
                self.reject_closed_promise(stored_error, context)?;
            }
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#default-writer-closed>
    pub(crate) fn closed(&self) -> JsResult<JsObject> {
        self.closed_promise_value().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultWriter is missing its closed promise")
                .into()
        })
    }

    /// <https://streams.spec.whatwg.org/#default-writer-desired-size>
    pub(crate) fn desired_size(&self) -> JsResult<Option<f64>> {
        let stream = self.stream_slot_value().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStreamDefaultWriter has been released")
        })?;
        self.get_desired_size_from_stream(stream)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-ready>
    pub(crate) fn ready(&self) -> JsResult<JsObject> {
        self.ready_promise_value().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultWriter is missing its ready promise")
                .into()
        })
    }

    /// <https://streams.spec.whatwg.org/#default-writer-abort>
    pub(crate) fn abort(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        let Some(stream) = self.stream_slot_value() else {
            return rejected_type_error_promise(
                "Cannot abort using a released WritableStreamDefaultWriter",
                context,
            );
        };

        stream.abort_stream(reason, context)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-close>
    pub(crate) fn close(&self, context: &mut Context) -> JsResult<JsObject> {
        let Some(stream) = self.stream_slot_value() else {
            return rejected_type_error_promise(
                "Cannot close using a released WritableStreamDefaultWriter",
                context,
            );
        };

        if stream.close_queued_or_in_flight() {
            return rejected_type_error_promise(
                "Cannot close a WritableStream that is already closing",
                context,
            );
        }

        stream.close_stream(context)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-release-lock>
    pub(crate) fn release_lock(&self, context: &mut Context) -> JsResult<()> {
        let Some(_) = self.stream_slot_value() else {
            return Ok(());
        };

        self.release(context)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-write>
    pub(crate) fn write(&self, chunk: JsValue, context: &mut Context) -> JsResult<JsObject> {
        let Some(stream) = self.stream_slot_value() else {
            return rejected_type_error_promise(
                "Cannot write using a released WritableStreamDefaultWriter",
                context,
            );
        };

        self.write_with_stream(stream, chunk, context)
    }
    pub(crate) fn reset_ready_promise(&self, context: &mut Context) -> JsResult<()> {
        let (promise, resolvers) = JsPromise::new_pending(context);
        self.set_ready_promise_value(Some(promise.into()));
        self.set_ready_resolvers_value(Some(resolvers));
        Ok(())
    }
    pub(crate) fn resolve_ready_promise(&self, context: &mut Context) -> JsResult<()> {
        if let Some(resolvers) = self.ready_resolvers_value() {
            resolvers
                .resolve
                .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
            self.set_ready_resolvers_value(None);
            return Ok(());
        }

        self.set_ready_promise_value(Some(resolved_promise(JsValue::undefined(), context)?));
        Ok(())
    }
    pub(crate) fn reject_ready_promise(
        &self,
        error: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        if let Some(resolvers) = self.ready_resolvers_value() {
            resolvers
                .reject
                .call(&JsValue::undefined(), &[error.clone()], context)?;
            self.set_ready_resolvers_value(None);
        } else {
            self.set_ready_promise_value(Some(rejected_promise(error, context)?));
        }

        if let Some(ready_promise) = self.ready_promise_value() {
            mark_promise_as_handled(&ready_promise, context)?;
        }
        Ok(())
    }
    pub(crate) fn reset_closed_promise(&self, context: &mut Context) {
        let (promise, resolvers) = JsPromise::new_pending(context);
        self.set_closed_promise_value(Some(promise.into()));
        self.set_closed_resolvers_value(Some(resolvers));
    }
    pub(crate) fn resolve_closed_promise(&self, context: &mut Context) -> JsResult<()> {
        if let Some(resolvers) = self.closed_resolvers_value() {
            resolvers
                .resolve
                .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
            self.set_closed_resolvers_value(None);
            return Ok(());
        }

        self.set_closed_promise_value(Some(resolved_promise(JsValue::undefined(), context)?));
        Ok(())
    }
    pub(crate) fn reject_closed_promise(
        &self,
        error: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        if let Some(resolvers) = self.closed_resolvers_value() {
            resolvers
                .reject
                .call(&JsValue::undefined(), &[error.clone()], context)?;
            self.set_closed_resolvers_value(None);
        } else {
            self.set_closed_promise_value(Some(rejected_promise(error, context)?));
        }

        if let Some(closed_promise) = self.closed_promise_value() {
            mark_promise_as_handled(&closed_promise, context)?;
        }
        Ok(())
    }

    pub(crate) fn ensure_closed_promise_rejected(
        &self,
        error: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        self.reject_closed_promise(error, context)
    }

    pub(crate) fn ensure_ready_promise_rejected(
        &self,
        error: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        self.reject_ready_promise(error, context)
    }

    fn get_desired_size_from_stream(&self, stream: WritableStream) -> JsResult<Option<f64>> {
        match stream.state() {
            WritableStreamState::Errored | WritableStreamState::Erroring => Ok(None),
            WritableStreamState::Closed => Ok(Some(0.0)),
            WritableStreamState::Writable => {
                let controller = stream.controller_slot().ok_or_else(|| {
                    JsNativeError::typ().with_message("WritableStream is missing its controller")
                })?;
                Ok(Some(writable_stream_default_controller_get_desired_size(
                    controller.as_default_controller(),
                )?))
            }
        }
    }

    fn release(&self, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot_value().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStreamDefaultWriter has been released")
        })?;
        debug_assert!(stream.writer_slot().is_some());

        let released_error = type_error_value("Writer was released", context)?;
        self.ensure_ready_promise_rejected(released_error.clone(), context)?;
        self.ensure_closed_promise_rejected(released_error, context)?;
        stream.set_writer_slot(None);
        self.set_stream_slot_value(None);
        Ok(())
    }

    fn write_with_stream(
        &self,
        stream: WritableStream,
        chunk: JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        let controller = stream.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its controller")
        })?;
        let chunk_size = writable_stream_default_controller_get_chunk_size(
            controller.as_default_controller(),
            &chunk,
            context,
        )?;

        if let Some(current_stream) = self.stream_slot_value() {
            if !JsObject::equals(&current_stream.object()?, &stream.object()?) {
                return rejected_type_error_promise(
                    "Cannot write using a released WritableStreamDefaultWriter",
                    context,
                );
            }
        } else {
            return rejected_type_error_promise(
                "Cannot write using a released WritableStreamDefaultWriter",
                context,
            );
        }

        match stream.state() {
            WritableStreamState::Errored => return rejected_promise(stream.stored_error(), context),
            WritableStreamState::Closed => {
                return rejected_type_error_promise(
                    "Cannot write to a WritableStream that is closing or closed",
                    context,
                );
            }
            WritableStreamState::Erroring => {
                return rejected_promise(stream.stored_error(), context);
            }
            WritableStreamState::Writable => {}
        }

        if stream.close_queued_or_in_flight() {
            return rejected_type_error_promise(
                "Cannot write to a WritableStream that is closing or closed",
                context,
            );
        }

        let promise = stream.add_write_request(context)?;
        writable_stream_default_controller_write(
            controller.as_default_controller(),
            chunk,
            chunk_size,
            context,
        )?;
        Ok(promise)
    }
}
pub(crate) fn construct_writable_stream_default_writer(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<WritableStreamDefaultWriter> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;
    let stream_object = args.get_or_undefined(0).as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter requires a WritableStream")
    })?;
    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let writer = WritableStreamDefaultWriter::new(Some(writer_object.clone()));
    writer.set_up_writable_stream_default_writer(stream, context)?;
    Ok(writer)
}

/// <https://streams.spec.whatwg.org/#acquire-writable-stream-default-writer>
pub(crate) fn acquire_writable_stream_default_writer(
    stream: WritableStream,
    context: &mut Context,
) -> JsResult<JsObject> {
    let writer = create_writable_stream_default_writer(context)?;
    writer.set_up_writable_stream_default_writer(stream, context)?;
    writer.object()
}
fn create_writable_stream_default_writer(
    context: &mut Context,
) -> JsResult<WritableStreamDefaultWriter> {
    let writer = WritableStreamDefaultWriter::new(None);
    let writer_object = WritableStreamDefaultWriter::from_data(writer.clone(), context)?;
    writer.set_reflector(writer_object);
    Ok(writer)
}
pub(crate) fn with_writable_stream_default_writer_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&WritableStreamDefaultWriter) -> R,
) -> JsResult<R> {
    let writer = object
        .downcast_ref::<WritableStreamDefaultWriter>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not a WritableStreamDefaultWriter"))?;
    Ok(f(&writer))
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-writer-release>
pub(crate) fn writable_stream_default_writer_release(
    writer: WritableStreamDefaultWriter,
    context: &mut Context,
) -> JsResult<()> {
    writer.release(context)
}