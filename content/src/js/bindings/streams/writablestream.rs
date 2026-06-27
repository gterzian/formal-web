use std::marker::PhantomData;
use boa_engine::{Context, JsArgs, JsNativeError, JsResult, JsValue};

use crate::streams::{
    construct_writable_stream, construct_writable_stream_default_writer,
    with_writable_stream_default_controller_ref, with_writable_stream_default_writer_ref,
    with_writable_stream_ref, WritableStream, WritableStreamDefaultController,
    WritableStreamDefaultWriter,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

impl WebIdlInterface<js_engine::boa::BoaTypes> for WritableStream {
    const NAME: &'static str = "WritableStream";

    fn create_platform_object(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        construct_writable_stream(new_target, args, context)
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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

impl WebIdlInterface<js_engine::boa::BoaTypes> for WritableStreamDefaultController {
    const NAME: &'static str = "WritableStreamDefaultController";

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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

impl WebIdlInterface<js_engine::boa::BoaTypes> for WritableStreamDefaultWriter {
    const NAME: &'static str = "WritableStreamDefaultWriter";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        construct_writable_stream_default_writer(this, args, context)
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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

fn get_locked(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    with_writable_stream_ref(&stream_object, |stream| JsValue::from(stream.locked()))
}

fn abort_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let promise = stream.abort(args.get_or_undefined(0).clone(), context)?;
    Ok(JsValue::from(promise))
}

fn close_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let promise = stream.close(context)?;
    Ok(JsValue::from(promise))
}

fn get_writer_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let writer = stream.get_writer(context)?;
    Ok(JsValue::from(writer))
}

fn get_signal(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WritableStreamDefaultController receiver is not an object")
    })?;

    let signal = with_writable_stream_default_controller_ref(&controller_object, |controller| {
        controller.signal_value()
    })??;
    Ok(JsValue::from(signal))
}

fn error_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WritableStreamDefaultController receiver is not an object")
    })?;

    let controller =
        with_writable_stream_default_controller_ref(&controller_object, |controller| {
            controller.clone()
        })?;
    controller.error(args.get_or_undefined(0).clone(), context)?;
    Ok(JsValue::undefined())
}

fn get_closed(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise =
        with_writable_stream_default_writer_ref(&writer_object, |writer| writer.closed())??;
    Ok(JsValue::from(promise))
}

fn get_desired_size(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    match with_writable_stream_default_writer_ref(&writer_object, |writer| writer.desired_size())??
    {
        Some(size) => Ok(JsValue::from(size)),
        None => Ok(JsValue::null()),
    }
}

fn get_ready(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise =
        with_writable_stream_default_writer_ref(&writer_object, |writer| writer.ready())??;
    Ok(JsValue::from(promise))
}

fn abort_writer_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.abort(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::from(promise))
}

fn close_writer_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise =
        with_writable_stream_default_writer_ref(&writer_object, |writer| writer.close(context))??;
    Ok(JsValue::from(promise))
}

fn release_lock_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.release_lock(context)
    })??;
    Ok(JsValue::undefined())
}

fn write_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.write(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::from(promise))
}
