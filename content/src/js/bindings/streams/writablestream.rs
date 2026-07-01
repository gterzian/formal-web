use boa_engine::{JsArgs, JsNativeError, JsValue};
use std::marker::PhantomData;

use crate::streams::{
    WritableStream, WritableStreamDefaultController, WritableStreamDefaultWriter,
    construct_writable_stream, construct_writable_stream_default_writer,
    with_writable_stream_default_controller_ref, with_writable_stream_default_controller_ref_ec,
    with_writable_stream_default_writer_ref, with_writable_stream_default_writer_ref_ec,
    with_writable_stream_ref_ec,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for WritableStream {
    const NAME: &'static str = "WritableStream";

    fn create_platform_object(
        new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        construct_writable_stream(new_target, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "locked",
            getter: get_locked,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "abort",
            length: 1,
            method: abort_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "close",
            length: 0,
            method: close_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "getWriter",
            length: 0,
            method: get_writer_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for WritableStreamDefaultController {
    const NAME: &'static str = "WritableStreamDefaultController";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "signal",
            getter: get_signal,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "error",
            length: 1,
            method: error_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for WritableStreamDefaultWriter {
    const NAME: &'static str = "WritableStreamDefaultWriter";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        construct_writable_stream_default_writer(this, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "closed",
            getter: get_closed,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "desiredSize",
            getter: get_desired_size,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "ready",
            getter: get_ready,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "abort",
            length: 1,
            method: abort_writer_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "close",
            length: 0,
            method: close_writer_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "releaseLock",
            length: 0,
            method: release_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "write",
            length: 1,
            method: write_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ── Member getters/setters/methods ──

fn get_locked(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStream receiver is not an object"))?;
    let locked = with_writable_stream_ref_ec(&stream_object, ec, |stream| stream.locked())?;
    Ok(JsValue::from(locked))
}

fn abort_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStream receiver is not an object"))?;
    let stream = with_writable_stream_ref_ec(&stream_object, ec, |s| s.clone())?;
    let promise = stream.abort(args.get_or_undefined(0).clone(), ec)?;
    Ok(crate::js::Types::value_from_object(promise))
}

fn close_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStream receiver is not an object"))?;
    let stream = with_writable_stream_ref_ec(&stream_object, ec, |s| s.clone())?;
    let promise = stream.close(ec)?;
    Ok(crate::js::Types::value_from_object(promise))
}

fn get_writer_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStream receiver is not an object"))?;
    let stream = with_writable_stream_ref_ec(&stream_object, ec, |s| s.clone())?;
    let writer = stream.get_writer(ec)?;
    Ok(crate::js::Types::value_from_object(writer))
}

fn get_signal(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultController receiver is not an object"))?;
    let controller = with_writable_stream_default_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    let signal = controller.signal_value_ec(ec)?;
    Ok(JsValue::from(signal))
}

fn error_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultController receiver is not an object"))?;
    let controller = with_writable_stream_default_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.error(args.get_or_undefined(0).clone(), ec)?;
    Ok(ec.value_undefined())
}

fn get_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    let closed = writer.closed_ec(ec)?;
    Ok(JsValue::from(closed))
}

fn get_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    let size = writer.desired_size_ec(ec)?;
    Ok(match size {
        Some(s) => JsValue::from(s),
        None => JsValue::null(),
    })
}

fn get_ready(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    let ready = writer.ready_ec(ec)?;
    Ok(JsValue::from(ready))
}

fn abort_writer_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let reason = args.get_or_undefined(0).clone();
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    let promise = writer.abort(reason, ec)?;
    Ok(crate::js::Types::value_from_object(promise))
}

fn close_writer_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    let promise = writer.close(ec)?;
    Ok(crate::js::Types::value_from_object(promise))
}

fn release_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    writer.release_lock(ec)?;
    Ok(ec.value_undefined())
}

fn write_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let writer_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WritableStreamDefaultWriter receiver is not an object"))?;
    let chunk = args.get_or_undefined(0).clone();
    let writer = with_writable_stream_default_writer_ref_ec(&writer_object, ec, |w| w.clone())?;
    let promise = writer.write(chunk, ec)?;
    Ok(crate::js::Types::value_from_object(promise))
}
