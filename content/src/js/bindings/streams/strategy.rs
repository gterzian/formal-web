use std::marker::PhantomData;

use boa_engine::JsValue;

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

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
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ByteLengthQueuingStrategy receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(strategy) = data.downcast_ref::<ByteLengthQueuingStrategy>() {
            return Ok(ec.value_from_number(strategy.high_water_mark()));
        }
    }
    Err(ec.new_type_error("receiver is not a ByteLengthQueuingStrategy"))
}

fn get_count_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("CountQueuingStrategy receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(strategy) = data.downcast_ref::<CountQueuingStrategy>() {
            return Ok(ec.value_from_number(strategy.high_water_mark()));
        }
    }
    Err(ec.new_type_error("receiver is not a CountQueuingStrategy"))
}

fn get_byte_length_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let function = ec.create_builtin_function(
        Box::new(move |args, _this, inner_ec| {
            byte_length_size(
                &args.first().cloned().unwrap_or(inner_ec.value_undefined()),
                inner_ec,
            )
        }),
        1,
        ec.property_key_from_str("size"),
    );
    Ok(crate::js::Types::value_from_object(
        crate::js::Types::object_from_function(function),
    ))
}

fn get_count_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let function = ec.create_builtin_function(
        Box::new(move |args, _this, inner_ec| {
            Ok(count_size(
                &args.first().cloned().unwrap_or(inner_ec.value_undefined()),
            ))
        }),
        1,
        ec.property_key_from_str("size"),
    );
    Ok(crate::js::Types::value_from_object(
        crate::js::Types::object_from_function(function),
    ))
}
