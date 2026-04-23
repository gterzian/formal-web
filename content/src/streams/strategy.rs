use boa_engine::{Context, JsData, JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use boa_gc::{Finalize, Trace};

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
pub enum SizeAlgorithm {
    ReturnOne,
    Callback { callback: JsObject },
}

impl SizeAlgorithm {
    /// <https://streams.spec.whatwg.org/#size-algorithm>
    pub(crate) fn size(&self, chunk: &JsValue, context: &mut Context) -> JsResult<f64> {
        match self {
            Self::ReturnOne => Ok(1.0),
            Self::Callback { callback } => {
                let callback =
                    boa_engine::object::builtins::JsFunction::from_object(callback.clone())
                        .ok_or_else(|| {
                            JsError::from(
                                JsNativeError::typ().with_message("size algorithm is not callable"),
                            )
                        })?;

                let value = callback.call(&JsValue::undefined(), &[chunk.clone()], context)?;
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

/// <https://streams.spec.whatwg.org/#extract-high-water-mark>
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

/// <https://streams.spec.whatwg.org/#extract-size-algorithm>
pub(crate) fn extract_size_algorithm(
    strategy: &JsValue,
    context: &mut Context,
) -> JsResult<SizeAlgorithm> {
    // Step 1: "If strategy[\"size\"] does not exist, return an algorithm that returns 1."
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(SizeAlgorithm::ReturnOne);
    }

    let strategy = strategy.to_object(context)?;

    // Step 2: "Let size be strategy[\"size\"]."
    let size = strategy.get(boa_engine::js_string!("size"), context)?;

    // Step 3: "If size is undefined, return an algorithm that returns 1."
    if size.is_undefined() {
        return Ok(SizeAlgorithm::ReturnOne);
    }

    // Step 4: "If IsCallable(size) is false, throw a TypeError exception."
    let size = size
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("strategy.size must be callable"))?;
    if !size.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("strategy.size must be callable")
            .into());
    }

    // Step 5: "Return an algorithm that performs ? Call(size, strategy, « chunk »)."
    Ok(SizeAlgorithm::Callback {
        callback: size.clone(),
    })
}

/// <https://streams.spec.whatwg.org/#blqs-size>
pub(crate) fn byte_length_size(chunk: &JsValue, context: &mut Context) -> JsResult<JsValue> {
    let chunk = chunk.to_object(context)?;
    chunk.get(boa_engine::js_string!("byteLength"), context)
}

/// <https://streams.spec.whatwg.org/#cqs-size>
pub(crate) fn count_size(_: &JsValue) -> JsValue {
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
