use boa_engine::JsValue;
use boa_gc::{Finalize, Trace};

use js_engine::{Completion, ExecutionContext};

use crate::webidl::{Callback, ExceptionBehavior, invoke_callback_function};

js_engine::impl_gc_traits! {
    /// <https://streams.spec.whatwg.org/#blqs-class>
    #[derive(Clone)]
    pub struct ByteLengthQueuingStrategy {
        /// <https://streams.spec.whatwg.org/#bytelengthqueuingstrategy-highwatermark>
        #[unsafe_ignore_trace]
        high_water_mark: f64,
    }
}

impl ByteLengthQueuingStrategy {
    /// <https://streams.spec.whatwg.org/#blqs-constructor>
    pub(crate) fn new(high_water_mark: f64) -> Self {
        Self { high_water_mark }
    }

    /// <https://streams.spec.whatwg.org/#blqs-high-water-mark>
    pub(crate) fn high_water_mark(&self) -> f64 {
        self.high_water_mark
    }
}

js_engine::impl_gc_traits! {
    /// <https://streams.spec.whatwg.org/#cqs-class>
    #[derive(Clone)]
    pub struct CountQueuingStrategy {
        /// <https://streams.spec.whatwg.org/#countqueuingstrategy-highwatermark>
        #[unsafe_ignore_trace]
        high_water_mark: f64,
    }
}

impl CountQueuingStrategy {
    /// <https://streams.spec.whatwg.org/#cqs-constructor>
    pub(crate) fn new(high_water_mark: f64) -> Self {
        Self { high_water_mark }
    }

    /// <https://streams.spec.whatwg.org/#cqs-high-water-mark>
    pub(crate) fn high_water_mark(&self) -> f64 {
        self.high_water_mark
    }
}

/// <https://streams.spec.whatwg.org/#size-algorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum SizeAlgorithm {
    ReturnOne,
    Callback { callback: Callback },
}

impl SizeAlgorithm {
    /// <https://streams.spec.whatwg.org/#make-size-algorithm-from-size-function>
    pub(crate) fn size(
        &self,
        chunk: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<f64, crate::js::Types> {
        match self {
            Self::ReturnOne => Ok(1.0),
            Self::Callback { callback } => {
                // "Return the result of invoking strategy[\"size\"] with argument
                // list « chunk »."
                let result = invoke_callback_function(
                    ec,
                    callback,
                    &[chunk.clone()],
                    ExceptionBehavior::Rethrow,
                    None,
                );
                let value = match result {
                    Ok(value) => value,
                    Err(js_error) => {
                        // SAFETY: ec is backed by BoaContext repr(transparent) over Context
                        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
                        let opaque = js_error.into_opaque(ctx);
                        return Err(opaque.unwrap_or(JsValue::undefined()));
                    }
                };
                to_non_negative_number(&value, ec)
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#validate-and-normalize-high-water-mark>
pub(crate) fn validate_and_normalize_high_water_mark(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<f64, crate::js::Types> {
    // Step 1 (implicit): "Let highWaterMark be ? ToNumber(highWaterMark)."
    let number = ec.to_number(value.clone())?;
    // Step 2: "If highWaterMark is NaN or highWaterMark < 0, throw a RangeError exception."
    if number.is_nan() || number < 0.0 {
        return Err(ec.new_range_error("highWaterMark must be a non-negative number"));
    }
    // Step 3: "Return highWaterMark."
    Ok(number)
}

/// <https://streams.spec.whatwg.org/#validate-and-normalize-high-water-mark>
pub(crate) fn extract_high_water_mark(
    strategy: &JsValue,
    default_high_water_mark: f64,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<f64, crate::js::Types> {
    // Step 1: "If strategy[\"highWaterMark\"] does not exist, return defaultHWM."
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(default_high_water_mark);
    }

    let strategy = ec.to_object(strategy.clone())?;

    // Step 2: "Let highWaterMark be strategy[\"highWaterMark\"]."
    let high_water_mark =
        ExecutionContext::get(ec, strategy, ec.property_key_from_str("highWaterMark"))?;

    // Step 3: "If highWaterMark is undefined, return defaultHWM."
    let undefined_value = ec.value_undefined();
    if ec.same_value(&high_water_mark, &undefined_value) {
        return Ok(default_high_water_mark);
    }

    // Step 4: "Return ? ValidateAndNormalizeHighWaterMark(highWaterMark)."
    validate_and_normalize_high_water_mark(&high_water_mark, ec)
}

/// <https://streams.spec.whatwg.org/#make-size-algorithm-from-size-function>
pub(crate) fn extract_size_algorithm(
    strategy: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<SizeAlgorithm, crate::js::Types> {
    // Step 1: "If strategy[\"size\"] does not exist, return an algorithm that returns 1."
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(SizeAlgorithm::ReturnOne);
    }

    let strategy = ec.to_object(strategy.clone())?;
    let size = ExecutionContext::get(ec, strategy, ec.property_key_from_str("size"))?;
    let undefined_value = ec.value_undefined();
    if ec.same_value(&size, &undefined_value) {
        return Ok(SizeAlgorithm::ReturnOne);
    }

    // Step 2: "Return an algorithm that performs the following steps, taking a chunk argument:
    // Return the result of invoking strategy[\"size\"] with argument list « chunk »."
    // Note: IsCallable is checked here rather than deferring to the Web IDL invoke
    // algorithm so that the TypeError is thrown at construction time rather than at
    // first enqueue.
    let size = size
        .as_object()
        .ok_or_else(|| ec.new_type_error("strategy.size must be callable"))?;
    let size_value = JsValue::from(size.clone());
    if !ec.is_callable(&size_value) {
        return Err(ec.new_type_error("strategy.size must be callable"));
    }

    Ok(SizeAlgorithm::Callback {
        callback: Callback::from_object(size.clone()),
    })
}

/// <https://streams.spec.whatwg.org/#byte-length-queuing-strategy-size-function>
pub(crate) fn byte_length_size(
    chunk: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // "Return ? GetV(chunk, \"byteLength\")."
    let chunk = ec.to_object(chunk.clone())?;
    ExecutionContext::get(ec, chunk, ec.property_key_from_str("byteLength"))
}

/// <https://streams.spec.whatwg.org/#count-queuing-strategy-size-function>
pub(crate) fn count_size(_: &JsValue) -> JsValue {
    // "Return 1."
    JsValue::from(1)
}

fn to_non_negative_number(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<f64, crate::js::Types> {
    let number = ec.to_number(value.clone())?;
    if !number.is_finite() || number < 0.0 {
        return Err(ec.new_range_error("queue strategy size must be a finite, non-negative number"));
    }
    Ok(number)
}
