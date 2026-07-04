use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{mark_promise_as_handled, rejected_promise, resolved_promise};
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes, PromiseResolvers};

use crate::js::Types;

use super::{
    WritableStream, WritableStreamState, WritableStreamWriter, rejected_type_error_promise,
    type_error_value, with_writable_stream_ref, writable_stream_default_controller_get_chunk_size,
    writable_stream_default_controller_get_desired_size, writable_stream_default_controller_write,
};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter>
#[gc_struct]
pub struct WritableStreamDefaultWriter {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-stream>
    stream: GcCell<Option<WritableStream>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-readypromise>
    ready_promise: GcCell<Option<JsObject>>,
    ready_resolvers: GcCell<Option<PromiseResolvers<Types>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-closedpromise>
    closed_promise: GcCell<Option<JsObject>>,
    closed_resolvers: GcCell<Option<PromiseResolvers<Types>>>,
}

impl WritableStreamDefaultWriter {
    pub(crate) fn new() -> Self {
        Self {
            stream: gc_cell_new(None),
            ready_promise: gc_cell_new(None),
            ready_resolvers: gc_cell_new(None),
            closed_promise: gc_cell_new(None),
            closed_resolvers: gc_cell_new(None),
        }
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
    pub(crate) fn ready_resolvers_value(&self) -> Option<PromiseResolvers<Types>> {
        self.ready_resolvers.borrow().clone()
    }
    pub(crate) fn set_ready_resolvers_value(&self, resolvers: Option<PromiseResolvers<Types>>) {
        *self.ready_resolvers.borrow_mut() = resolvers;
    }
    pub(crate) fn closed_promise_value(&self) -> Option<JsObject> {
        self.closed_promise.borrow().clone()
    }
    pub(crate) fn set_closed_promise_value(&self, promise: Option<JsObject>) {
        *self.closed_promise.borrow_mut() = promise;
    }
    pub(crate) fn closed_resolvers_value(&self) -> Option<PromiseResolvers<Types>> {
        self.closed_resolvers.borrow().clone()
    }
    pub(crate) fn set_closed_resolvers_value(&self, resolvers: Option<PromiseResolvers<Types>>) {
        *self.closed_resolvers.borrow_mut() = resolvers;
    }

    /// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-writer>
    pub(crate) fn set_up_writable_stream_default_writer(
        &self,
        stream: WritableStream,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if stream.is_writable_stream_locked() {
            return Err(
                ec.new_type_error("Cannot create a writer for a stream that already has a writer")
            );
        }

        self.set_stream_slot_value(Some(stream.clone()));
        stream.set_writer_slot(Some(WritableStreamWriter::Default(self.clone())));

        match stream.state() {
            WritableStreamState::Writable => {
                if !stream.close_queued_or_in_flight() && stream.backpressure() {
                    self.reset_ready_promise(ec)?;
                } else {
                    self.resolve_ready_promise(ec)?;
                }
                self.reset_closed_promise(ec)?;
            }
            WritableStreamState::Erroring => {
                self.reject_ready_promise(stream.stored_error(), ec)?;
                self.reset_closed_promise(ec)?;
            }
            WritableStreamState::Closed => {
                self.resolve_ready_promise(ec)?;
                self.resolve_closed_promise(ec)?;
            }
            WritableStreamState::Errored => {
                let stored_error = stream.stored_error();
                self.reject_ready_promise(stored_error.clone(), ec)?;
                self.reject_closed_promise(stored_error, ec)?;
            }
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#default-writer-closed>
    pub(crate) fn closed(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let err = ec.new_type_error("WritableStreamDefaultWriter is missing its closed promise");
        self.closed_promise_value().ok_or(err)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-desired-size>
    pub(crate) fn desired_size(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<f64>, Types> {
        let stream = self
            .stream_slot_value()
            .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter has been released"))?;
        self.get_desired_size_from_stream(stream, ec)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-ready>
    pub(crate) fn ready(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let err = ec.new_type_error("WritableStreamDefaultWriter is missing its ready promise");
        self.ready_promise_value().ok_or(err)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-abort>
    pub(crate) fn abort(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let Some(stream) = self.stream_slot_value() else {
            return rejected_type_error_promise(
                "Cannot abort using a released WritableStreamDefaultWriter",
                ec,
            );
        };

        stream.abort_stream(reason, ec)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-close>
    pub(crate) fn close(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let Some(stream) = self.stream_slot_value() else {
            return rejected_type_error_promise(
                "Cannot close using a released WritableStreamDefaultWriter",
                ec,
            );
        };

        if stream.close_queued_or_in_flight() {
            return rejected_type_error_promise(
                "Cannot close a WritableStream that is already closing",
                ec,
            );
        }

        stream.close_stream(ec)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-release-lock>
    pub(crate) fn release_lock(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let Some(_) = self.stream_slot_value() else {
            return Ok(());
        };

        self.release(ec)
    }

    /// <https://streams.spec.whatwg.org/#default-writer-write>
    pub(crate) fn write(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let Some(stream) = self.stream_slot_value() else {
            return rejected_type_error_promise(
                "Cannot write using a released WritableStreamDefaultWriter",
                ec,
            );
        };

        self.write_with_stream(stream, chunk, ec)
    }

    pub(crate) fn reset_ready_promise(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let (promise, resolvers) = ec.new_promise_pending()?;
        let promise_obj = promise
            .as_object()
            .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
        self.set_ready_promise_value(Some(promise_obj));
        self.set_ready_resolvers_value(Some(resolvers));
        Ok(())
    }

    pub(crate) fn resolve_ready_promise(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if let Some(resolvers) = self.ready_resolvers_value() {
            let undefined = ec.value_undefined();
            let args = [undefined];
            ec.call(&resolvers.resolve, &args[0], &args)?;
            self.set_ready_resolvers_value(None);
            return Ok(());
        }

        let promise = resolved_promise(ec.value_undefined(), ec)?;
        self.set_ready_promise_value(Some(promise));
        Ok(())
    }

    pub(crate) fn reject_ready_promise(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if let Some(resolvers) = self.ready_resolvers_value() {
            let undefined = ec.value_undefined();
            ec.call(&resolvers.reject, &undefined, &[error])?;
            self.set_ready_resolvers_value(None);
        } else {
            self.set_ready_promise_value(Some(rejected_promise(error, ec)?));
        }

        if let Some(ready_promise) = self.ready_promise_value() {
            mark_promise_as_handled(&ready_promise, ec)?;
        }
        Ok(())
    }

    pub(crate) fn reset_closed_promise(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let (promise, resolvers) = ec.new_promise_pending()?;
        let promise_obj = promise
            .as_object()
            .ok_or_else(|| ec.new_type_error("new_promise_pending did not return an object"))?;
        self.set_closed_promise_value(Some(promise_obj));
        self.set_closed_resolvers_value(Some(resolvers));
        Ok(())
    }

    pub(crate) fn resolve_closed_promise(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if let Some(resolvers) = self.closed_resolvers_value() {
            let undefined = ec.value_undefined();
            let args = [undefined];
            ec.call(&resolvers.resolve, &args[0], &args)?;
            self.set_closed_resolvers_value(None);
            return Ok(());
        }

        let promise = resolved_promise(ec.value_undefined(), ec)?;
        self.set_closed_promise_value(Some(promise));
        Ok(())
    }

    pub(crate) fn reject_closed_promise(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if let Some(resolvers) = self.closed_resolvers_value() {
            let undefined = ec.value_undefined();
            ec.call(&resolvers.reject, &undefined, &[error])?;
            self.set_closed_resolvers_value(None);
        } else {
            self.set_closed_promise_value(Some(rejected_promise(error, ec)?));
        }

        if let Some(closed_promise) = self.closed_promise_value() {
            mark_promise_as_handled(&closed_promise, ec)?;
        }
        Ok(())
    }

    pub(crate) fn ensure_closed_promise_rejected(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        self.reject_closed_promise(error, ec)
    }

    pub(crate) fn ensure_ready_promise_rejected(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        self.reject_ready_promise(error, ec)
    }

    fn get_desired_size_from_stream(
        &self,
        stream: WritableStream,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<f64>, Types> {
        match stream.state() {
            WritableStreamState::Errored | WritableStreamState::Erroring => Ok(None),
            WritableStreamState::Closed => Ok(Some(0.0)),
            WritableStreamState::Writable => {
                let controller = stream
                    .controller_slot()
                    .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
                Ok(Some(writable_stream_default_controller_get_desired_size(
                    controller.as_default_controller(),
                    ec,
                )?))
            }
        }
    }

    fn release(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let released = ec.new_type_error("WritableStreamDefaultWriter has been released");
        let stream = self.stream_slot_value().ok_or_else(|| released)?;
        debug_assert!(stream.writer_slot().is_some());

        let released_error = type_error_value("Writer was released", ec)?;
        self.ensure_ready_promise_rejected(released_error.clone(), ec)?;
        self.ensure_closed_promise_rejected(released_error, ec)?;
        stream.set_writer_slot(None);
        self.set_stream_slot_value(None);
        Ok(())
    }

    fn write_with_stream(
        &self,
        stream: WritableStream,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let no_ctrl = ec.new_type_error("WritableStream is missing its controller");
        let controller = stream.controller_slot().ok_or_else(|| no_ctrl)?;
        let chunk_size = writable_stream_default_controller_get_chunk_size(
            controller.as_default_controller(),
            &chunk,
            ec,
        )?;

        if let Some(current_stream) = self.stream_slot_value() {
            if !current_stream.same_instance(&stream) {
                return rejected_type_error_promise(
                    "Cannot write using a released WritableStreamDefaultWriter",
                    ec,
                );
            }
        } else {
            return rejected_type_error_promise(
                "Cannot write using a released WritableStreamDefaultWriter",
                ec,
            );
        }

        match stream.state() {
            WritableStreamState::Errored => {
                return rejected_promise(stream.stored_error(), ec);
            }
            WritableStreamState::Closed => {
                return rejected_type_error_promise(
                    "Cannot write to a WritableStream that is closing or closed",
                    ec,
                );
            }
            WritableStreamState::Erroring => {
                return rejected_promise(stream.stored_error(), ec);
            }
            WritableStreamState::Writable => {}
        }

        if stream.close_queued_or_in_flight() {
            return rejected_type_error_promise(
                "Cannot write to a WritableStream that is closing or closed",
                ec,
            );
        }

        let promise = stream.add_write_request(ec)?;
        writable_stream_default_controller_write(
            controller.as_default_controller(),
            chunk,
            chunk_size,
            ec,
        )?;
        Ok(promise)
    }
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultwriter-constructor>
pub(crate) fn construct_writable_stream_default_writer(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<WritableStreamDefaultWriter, Types> {
    let stream_object = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined())
        .as_object()
        .ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultWriter requires a WritableStream")
        })?;
    let stream = with_writable_stream_ref(&stream_object, ec, |stream| stream.clone())?;
    let writer = WritableStreamDefaultWriter::new();
    writer.set_up_writable_stream_default_writer(stream, ec)?;
    Ok(writer)
}

/// <https://streams.spec.whatwg.org/#acquire-writable-stream-default-writer>
pub(crate) fn acquire_writable_stream_default_writer(
    stream: WritableStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let writer_object = create_writable_stream_default_writer(ec)?;
    let writer =
        with_writable_stream_default_writer_ref(&writer_object, ec, |writer| writer.clone())?;
    writer.set_up_writable_stream_default_writer(stream, ec)?;
    Ok(writer_object)
}

fn create_writable_stream_default_writer(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let writer = WritableStreamDefaultWriter::new();
    let writer_object =
        create_interface_instance::<Types, WritableStreamDefaultWriter>(writer, ec)?;
    Ok(writer_object)
}

pub(crate) fn with_writable_stream_default_writer_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&WritableStreamDefaultWriter) -> R,
) -> Completion<R, Types> {
    let writer_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<WritableStreamDefaultWriter>());
    let writer = match writer_ref {
        Some(w) => w,
        None => return Err(ec.new_type_error("object is not a WritableStreamDefaultWriter")),
    };
    Ok(f(writer))
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-writer-release>
pub(crate) fn writable_stream_default_writer_release(
    writer: WritableStreamDefaultWriter,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    writer.release(ec)
}
