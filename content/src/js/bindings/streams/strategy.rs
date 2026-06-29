use boa_engine::{
    Context, JsArgs, JsError, JsResult, JsValue, js_string, native_function::NativeFunction,
    object::FunctionObjectBuilder,
};
use std::marker::PhantomData;

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext};

use crate::streams::{
    ByteLengthQueuingStrategy, CountQueuingStrategy, byte_length_size, count_size,
    validate_and_normalize_high_water_mark,
};

impl WebIdlInterface<crate::js::Types> for ByteLengthQueuingStrategy {
    const NAME: &'static str = "ByteLengthQueuingStrategy";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let init_value = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());
        let init = ec.to_object(init_value)?;
        let high_water_mark =
            ExecutionContext::get(ec, init, ec.property_key_from_str("highWaterMark"))?;
        let high_water_mark = validate_and_normalize_high_water_mark(&high_water_mark, ec)?;
        Ok(ByteLengthQueuingStrategy::new(high_water_mark))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "highWaterMark",
            getter: get_byte_length_high_water_mark,
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

            id: "size",
            getter: get_byte_length_size,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for CountQueuingStrategy {
    const NAME: &'static str = "CountQueuingStrategy";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let init_value = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());
        let init = ec.to_object(init_value)?;
        let high_water_mark =
            ExecutionContext::get(ec, init, ec.property_key_from_str("highWaterMark"))?;
        let high_water_mark = validate_and_normalize_high_water_mark(&high_water_mark, ec)?;
        Ok(CountQueuingStrategy::new(high_water_mark))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "highWaterMark",
            getter: get_count_high_water_mark,
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

            id: "size",
            getter: get_count_size,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
    }
}

fn get_byte_length_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let strategy = this
        .as_object()
        .ok_or_else(|| ec.new_type_error("ByteLengthQueuingStrategy receiver is not an object"))?;
    let strategy = strategy
        .downcast_ref::<ByteLengthQueuingStrategy>()
        .ok_or_else(|| ec.new_type_error("receiver is not a ByteLengthQueuingStrategy"))?;
    Ok(JsValue::from(strategy.high_water_mark()))
}

fn get_count_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let strategy = this
        .as_object()
        .ok_or_else(|| ec.new_type_error("CountQueuingStrategy receiver is not an object"))?;
    let strategy = strategy
        .downcast_ref::<CountQueuingStrategy>()
        .ok_or_else(|| ec.new_type_error("receiver is not a CountQueuingStrategy"))?;
    Ok(JsValue::from(strategy.high_water_mark()))
}

fn get_byte_length_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let function = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_fn_ptr(byte_length_size_function),
        )
        .name(js_string!("size"))
        .length(1)
        .build();
        Ok(JsValue::from(function))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_count_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let function = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_fn_ptr(count_size_function),
        )
        .name(js_string!("size"))
        .length(1)
        .build();
        Ok(JsValue::from(function))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn byte_length_size_function(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    byte_length_size(
        args.get_or_undefined(0),
        js_engine::boa::context_as_ec(context),
    )
    .map_err(JsError::from_opaque)
}

fn count_size_function(_: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(count_size(args.get_or_undefined(0)))
}
