use boa_engine::{Context, JsData, JsNativeError, JsResult, JsValue};
use boa_gc::{Finalize, Trace};

use crate::webidl::{Callback, ExceptionBehavior, invoke_callback_function};

/// <https://streams.spec.whatwg.org/#blqs-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ByteLengthQueuingStrategy {
    /// <https://streams.spec.whatwg.org/#bytelengthqueuingstrategy-highwatermark>
    #[unsafe_ignore_trace]
    high_water_mark: f64,
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

/// <https://streams.spec.whatwg.org/#cqs-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct CountQueuingStrategy {
    /// <https://streams.spec.whatwg.org/#countqueuingstrategy-highwatermark>
    #[unsafe_ignore_trace]
    high_water_mark: f64,
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
    pub(crate) fn size(&self, chunk: &JsValue, context: &mut Context) -> JsResult<f64> {
        match self {
            Self::ReturnOne => Ok(1.0),
            Self::Callback { callback } => {
                // "Return the result of invoking strategy[\"size\"] with argument
                // list « chunk »."
                let ec = crate::js::context_as_ec(context);
                let value = invoke_callback_function(
                    ec,
                    callback,
                    &[chunk.clone()],
                    ExceptionBehavior::Rethrow,
                    None,
                )?;
                to_non_negative_number(&value, context)
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#validate-and-normalize-high-water-mark>
pub(crate) fn validate_and_normalize_high_water_mark(
    value: &JsValue,
    context: &mut Context,
) -> JsResult<f64> {
    let number = value.to_number(context)?;
    if number.is_nan() || number < 0.0 {
        return Err(JsNativeError::range()
            .with_message("highWaterMark must be a non-negative number")
            .into());
    }
    Ok(number)
}

/// <https://streams.spec.whatwg.org/#validate-and-normalize-high-water-mark>
pub(crate) fn extract_high_water_mark(
    strategy: &JsValue,
    default_high_water_mark: f64,
    context: &mut Context,
) -> JsResult<f64> {
    // Step 1: "If strategy[\"highWaterMark\"] does not exist, return defaultHWM."
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(default_high_water_mark);
    }

    let strategy = strategy.to_object(context)?;

    // Step 2: "Let highWaterMark be strategy[\"highWaterMark\"]."
    let high_water_mark = strategy.get(boa_engine::js_string!("highWaterMark"), context)?;

    // Step 3: "If highWaterMark is undefined, return defaultHWM."
    if high_water_mark.is_undefined() {
        return Ok(default_high_water_mark);
    }

    // Step 4: "Return ? ValidateAndNormalizeHighWaterMark(highWaterMark)."
    validate_and_normalize_high_water_mark(&high_water_mark, context)
}

/// <https://streams.spec.whatwg.org/#make-size-algorithm-from-size-function>
pub(crate) fn extract_size_algorithm(
    strategy: &JsValue,
    context: &mut Context,
) -> JsResult<SizeAlgorithm> {
    // Step 1: "If strategy[\"size\"] does not exist, return an algorithm that returns 1."
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(SizeAlgorithm::ReturnOne);
    }

    let strategy = strategy.to_object(context)?;
    let size = strategy.get(boa_engine::js_string!("size"), context)?;
    if size.is_undefined() {
        return Ok(SizeAlgorithm::ReturnOne);
    }

    // Step 2: "Return an algorithm that performs the following steps, taking a chunk argument:
    // Return the result of invoking strategy[\"size\"] with argument list « chunk »."
    // Note: IsCallable is checked here rather than deferring to the Web IDL invoke
    // algorithm so that the TypeError is thrown at construction time (Step 2 of the
    // streams constructor algorithm) rather than at first enqueue.
    let size = size
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("strategy.size must be callable"))?;
    if !size.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("strategy.size must be callable")
            .into());
    }

    Ok(SizeAlgorithm::Callback {
        callback: Callback::from_object(size.clone()),
    })
}

/// <https://streams.spec.whatwg.org/#byte-length-queuing-strategy-size-function>
pub(crate) fn byte_length_size(chunk: &JsValue, context: &mut Context) -> JsResult<JsValue> {
    // "Return ? GetV(chunk, \"byteLength\")."
    let chunk = chunk.to_object(context)?;
    chunk.get(boa_engine::js_string!("byteLength"), context)
}

/// <https://streams.spec.whatwg.org/#count-queuing-strategy-size-function>
pub(crate) fn count_size(_: &JsValue) -> JsValue {
    // "Return 1."
    JsValue::from(1)
}

fn to_non_negative_number(value: &JsValue, context: &mut Context) -> JsResult<f64> {
    let number = value.to_number(context)?;
    if !number.is_finite() || number < 0.0 {
        return Err(JsNativeError::range()
            .with_message("queue strategy size must be a finite, non-negative number")
            .into());
    }
    Ok(number)
}
