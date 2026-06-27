use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue, js_string, native_function::NativeFunction,
    object::FunctionObjectBuilder,
};
use std::marker::PhantomData;

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

use crate::streams::{
    ByteLengthQueuingStrategy, CountQueuingStrategy, byte_length_size, count_size,
    validate_and_normalize_high_water_mark,
};

impl WebIdlInterface<js_engine::boa::BoaTypes> for ByteLengthQueuingStrategy {
    const NAME: &'static str = "ByteLengthQueuingStrategy";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { crate::js::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
            let init = args.get_or_undefined(0).to_object(ctx)?;
            let high_water_mark = init.get(js_string!("highWaterMark"), ctx)?;
            let high_water_mark = validate_and_normalize_high_water_mark(&high_water_mark, ctx)?;
            Ok(ByteLengthQueuingStrategy::new(high_water_mark))
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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

impl WebIdlInterface<js_engine::boa::BoaTypes> for CountQueuingStrategy {
    const NAME: &'static str = "CountQueuingStrategy";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { crate::js::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
            let init = args.get_or_undefined(0).to_object(ctx)?;
            let high_water_mark = init.get(js_string!("highWaterMark"), ctx)?;
            let high_water_mark = validate_and_normalize_high_water_mark(&high_water_mark, ctx)?;
            Ok(CountQueuingStrategy::new(high_water_mark))
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let strategy = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ByteLengthQueuingStrategy receiver is not an object")
        })?;
        let strategy = strategy
            .downcast_ref::<ByteLengthQueuingStrategy>()
            .ok_or_else(|| {
                JsNativeError::typ().with_message("receiver is not a ByteLengthQueuingStrategy")
            })?;
        Ok(JsValue::from(strategy.high_water_mark()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_count_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let strategy = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("CountQueuingStrategy receiver is not an object")
        })?;
        let strategy = strategy
            .downcast_ref::<CountQueuingStrategy>()
            .ok_or_else(|| {
                JsNativeError::typ().with_message("receiver is not a CountQueuingStrategy")
            })?;
        Ok(JsValue::from(strategy.high_water_mark()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_byte_length_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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
    byte_length_size(args.get_or_undefined(0), context)
}

fn count_size_function(_: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(count_size(args.get_or_undefined(0)))
}
