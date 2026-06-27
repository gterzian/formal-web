use boa_engine::{
    Context, JsData, JsNativeError, JsResult, JsString, JsValue,
    object::{JsObject, builtins::JsFunction},
};
use boa_gc::{Finalize, Trace};

use js_engine::boa::BoaTypes;

use crate::webidl::{Callback, EcmascriptHost, ExceptionBehavior, invoke_callback_function};

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
    /// <https://streams.spec.whatwg.org/#size-algorithm>
    pub(crate) fn size(&self, chunk: &JsValue, context: &mut Context) -> JsResult<f64> {
        match self {
            Self::ReturnOne => Ok(1.0),
            Self::Callback { callback } => {
                // Note: Wraps &mut Context in a temporary EcmascriptHost adapter.
                // This adapter will be eliminated in Phase 4 when the call chain
                // threads Engine instead of Context.  The NativeFunction barrier
                // (P3) currently prevents passing Engine through JS binding callbacks.
                struct CtxHost<'a>(&'a mut Context);
                impl EcmascriptHost<BoaTypes> for CtxHost<'_> {
                    fn get(
                        &mut self,
                        object: &JsObject,
                        property: &str,
                    ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes>
                    {
                        object
                            .get(JsString::from(property), self.0)
                            .map_err(|e| e.into_opaque(self.0).unwrap_or(JsValue::undefined()))
                    }
                    fn is_callable(&self, value: &JsValue) -> bool {
                        value.as_object().is_some_and(|o| o.is_callable())
                    }
                    fn call(
                        &mut self,
                        callable: &JsObject,
                        this_arg: &JsValue,
                        args: &[JsValue],
                    ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes>
                    {
                        let function =
                            JsFunction::from_object(callable.clone()).ok_or_else(|| {
                                JsValue::from(
                                    JsNativeError::typ()
                                        .with_message("callback is not callable")
                                        .into_opaque(self.0),
                                )
                            })?;
                        function
                            .call(this_arg, args, self.0)
                            .map_err(|e| e.into_opaque(self.0).unwrap_or(JsValue::undefined()))
                    }
                    fn perform_a_microtask_checkpoint(
                        &mut self,
                    ) -> js_engine::Completion<(), js_engine::boa::BoaTypes> {
                        let _ = self.0.run_jobs();
                        Ok(())
                    }
                    fn report_exception(&mut self, error: JsValue) {
                        log::error!("uncaught callback error: {error:?}");
                    }
                    fn value_undefined(&mut self) -> JsValue {
                        JsValue::undefined()
                    }
                    fn value_null(&mut self) -> JsValue {
                        JsValue::null()
                    }
                    fn value_from_bool(&mut self, b: bool) -> JsValue {
                        JsValue::from(b)
                    }
                    fn value_from_number(&mut self, n: f64) -> JsValue {
                        JsValue::from(n)
                    }
                    fn value_from_string(&mut self, s: boa_engine::JsString) -> JsValue {
                        JsValue::from(s)
                    }
                    fn js_string_from_str(&self, s: &str) -> boa_engine::JsString {
                        boa_engine::js_string!(s)
                    }
                }
                let value = {
                    let mut host = CtxHost(context);
                    invoke_callback_function(
                        &mut host,
                        callback,
                        &[chunk.clone()],
                        ExceptionBehavior::Rethrow,
                        None,
                    )?
                };
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
        callback: Callback::from_object(size.clone()),
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
