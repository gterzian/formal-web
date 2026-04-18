use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    property::Attribute,
};

use crate::streams::{
    ByteLengthQueuingStrategy, CountQueuingStrategy, byte_length_size, count_size,
    validate_and_normalize_high_water_mark,
};

impl Class for ByteLengthQueuingStrategy {
    const NAME: &'static str = "ByteLengthQueuingStrategy";
    const LENGTH: usize = 1;

    fn data_constructor(
        _this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let init = args.get_or_undefined(0).to_object(context)?;
        let high_water_mark = init.get(js_string!("highWaterMark"), context)?;
        let high_water_mark = validate_and_normalize_high_water_mark(&high_water_mark, context)?;
        Ok(ByteLengthQueuingStrategy::new(high_water_mark))
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("highWaterMark"),
                Some(
                    NativeFunction::from_fn_ptr(get_byte_length_high_water_mark)
                        .to_js_function(&realm),
                ),
                None,
                Attribute::all(),
            )
            .accessor(
                js_string!("size"),
                Some(NativeFunction::from_fn_ptr(get_byte_length_size).to_js_function(&realm)),
                None,
                Attribute::all(),
            );
        Ok(())
    }
}

impl Class for CountQueuingStrategy {
    const NAME: &'static str = "CountQueuingStrategy";
    const LENGTH: usize = 1;

    fn data_constructor(
        _this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let init = args.get_or_undefined(0).to_object(context)?;
        let high_water_mark = init.get(js_string!("highWaterMark"), context)?;
        let high_water_mark = validate_and_normalize_high_water_mark(&high_water_mark, context)?;
        Ok(CountQueuingStrategy::new(high_water_mark))
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("highWaterMark"),
                Some(NativeFunction::from_fn_ptr(get_count_high_water_mark).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .accessor(
                js_string!("size"),
                Some(NativeFunction::from_fn_ptr(get_count_size).to_js_function(&realm)),
                None,
                Attribute::all(),
            );
        Ok(())
    }
}

fn get_byte_length_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    _: &mut Context,
) -> JsResult<JsValue> {
    let strategy = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ByteLengthQueuingStrategy receiver is not an object")
    })?;
    let strategy = strategy
        .downcast_ref::<ByteLengthQueuingStrategy>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("receiver is not a ByteLengthQueuingStrategy")
        })?;
    Ok(JsValue::from(strategy.high_water_mark()))
}

fn get_count_high_water_mark(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let strategy = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("CountQueuingStrategy receiver is not an object")
    })?;
    let strategy = strategy
        .downcast_ref::<CountQueuingStrategy>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("receiver is not a CountQueuingStrategy")
        })?;
    Ok(JsValue::from(strategy.high_water_mark()))
}

fn get_byte_length_size(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let function = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(byte_length_size_function),
    )
    .name(js_string!("size"))
    .length(1)
    .build();
    Ok(JsValue::from(function))
}

fn get_count_size(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let function = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(count_size_function),
    )
    .name(js_string!("size"))
    .length(1)
    .build();
    Ok(JsValue::from(function))
}

fn byte_length_size_function(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    byte_length_size(args.get_or_undefined(0), context)
}

fn count_size_function(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(count_size(args.get_or_undefined(0)))
}
