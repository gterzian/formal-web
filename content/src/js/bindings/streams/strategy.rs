use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use crate::streams::{
    ByteLengthQueuingStrategy, CountQueuingStrategy, byte_length_size, count_size,
};

impl WebIdlInterface<Types> for ByteLengthQueuingStrategy {
    const NAME: &'static str = "ByteLengthQueuingStrategy";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Set this.[[highWaterMark]] to init["highWaterMark"].
        let init_value = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());
        let init = ec.to_object(init_value)?;
        let high_water_mark =
            ExecutionContext::get(ec, init, ec.property_key_from_str("highWaterMark"))?;
        // QueuingStrategyInit.highWaterMark is a required unrestricted double:
        // a missing member throws a TypeError and no non-negativity validation
        // applies here (the stream constructors validate later).
        let undefined_value = ec.value_undefined();
        if ec.same_value(&high_water_mark, &undefined_value) {
            return Err(
                ec.new_type_error("ByteLengthQueuingStrategy requires a highWaterMark member")
            );
        }
        let high_water_mark = ec.to_number(high_water_mark)?;
        Ok(ByteLengthQueuingStrategy::new(high_water_mark))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
    }
}

impl WebIdlInterface<Types> for CountQueuingStrategy {
    const NAME: &'static str = "CountQueuingStrategy";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let init_value = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());
        let init = ec.to_object(init_value)?;
        let high_water_mark =
            ExecutionContext::get(ec, init, ec.property_key_from_str("highWaterMark"))?;
        // QueuingStrategyInit.highWaterMark is a required unrestricted double:
        // a missing member throws a TypeError and no non-negativity validation
        // applies here (the stream constructors validate later).
        let undefined_value = ec.value_undefined();
        if ec.same_value(&high_water_mark, &undefined_value) {
            return Err(ec.new_type_error("CountQueuingStrategy requires a highWaterMark member"));
        }
        let high_water_mark = ec.to_number(high_water_mark)?;
        Ok(CountQueuingStrategy::new(high_water_mark))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
    }
}

fn get_byte_length_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ByteLengthQueuingStrategy receiver is not an object"))?;
    if let Some(strategy) = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<ByteLengthQueuingStrategy>())
    {
        return Ok(ec.value_from_number(strategy.high_water_mark()));
    }
    Err(ec.new_type_error("receiver is not a ByteLengthQueuingStrategy"))
}

fn get_count_high_water_mark(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("CountQueuingStrategy receiver is not an object"))?;
    if let Some(strategy) = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<CountQueuingStrategy>())
    {
        return Ok(ec.value_from_number(strategy.high_water_mark()));
    }
    Err(ec.new_type_error("receiver is not a CountQueuingStrategy"))
}

fn byte_length_size_fn(
    args: &[JsValue],
    _this: JsValue,
    inner_ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    byte_length_size(
        &args.first().cloned().unwrap_or(inner_ec.value_undefined()),
        inner_ec,
    )
}

fn get_byte_length_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Return this's relevant global object's byte length queuing
    //         strategy size function.
    global_size_function(
        ec,
        byte_length_size_fn,
        1,
        "\u{0}byte-length-queuing-strategy-size",
    )
}

fn get_count_size(
    _: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Return this's relevant global object's count queuing strategy
    //         size function.
    global_size_function(ec, count_size_fn, 0, "\u{0}count-queuing-strategy-size")
}

/// <https://streams.spec.whatwg.org/#blqs-size>
///
/// Returns the per-global size function, creating it on first access and
/// caching it on the global object so every access returns the same function.
/// The cache key is an internal null-prefixed property that user code cannot
/// realistically collide with.
type SizeSteps =
    fn(&[JsValue], JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>;

fn global_size_function(
    ec: &mut dyn ExecutionContext<Types>,
    steps: SizeSteps,
    length: u32,
    cache_key: &'static str,
) -> Completion<JsValue, Types> {
    let global = ec.realm_global_object();
    let key = ec.property_key_from_str(cache_key);
    let existing = ExecutionContext::get(ec, global.clone(), key.clone())?;
    let undefined_value = ec.value_undefined();
    if !ec.same_value(&existing, &undefined_value) {
        return Ok(existing);
    }
    let function =
        ec.create_builtin_fn_static(steps, length, ec.property_key_from_str("size"), false);
    let value = Types::value_from_object(Types::object_from_function(function));
    ec.define_property_or_throw(
        global,
        key,
        js_engine::PropertyDescriptor {
            value: Some(value.clone()),
            writable: Some(false),
            enumerable: Some(false),
            configurable: Some(true),
            get: None,
            set: None,
        },
    )?;
    Ok(value)
}

fn count_size_fn(
    args: &[JsValue],
    _this: JsValue,
    inner_ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    count_size(
        &args.first().cloned().unwrap_or(inner_ec.value_undefined()),
        inner_ec,
    )
}
