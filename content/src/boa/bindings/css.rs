/// Boa bindings for the `CSS` namespace.
///
/// https://drafts.csswg.org/css-conditional-3/#the-css-namespace

use boa_engine::{
    Context, JsResult, JsValue, js_string, native_function::NativeFunction,
    object::ObjectInitializer, property::Attribute,
};

use crate::css::CSS;

/// Install the `CSS` namespace object on the global scope.
///
/// https://drafts.csswg.org/css-conditional-3/#the-css-namespace
pub(crate) fn install_css_namespace(context: &mut Context) -> JsResult<()> {
    let css_object = {
        let mut initializer = ObjectInitializer::new(context);
        initializer.function(
            NativeFunction::from_fn_ptr(supports),
            js_string!("supports"),
            2,
        );
        initializer.build()
    };

    context.register_global_property(js_string!("CSS"), css_object, Attribute::all())
}

/// `CSS.supports(property, value)` / `CSS.supports(conditionText)`
///
/// https://drafts.csswg.org/css-conditional-3/#dom-css-supports-conditiontext-conditiontext
fn supports(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let result = if args.len() >= 2 {
        let property = args
            .first()
            .and_then(|value| value.as_string())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let value = args
            .get(1)
            .and_then(|value| value.as_string())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        CSS::supports(&property, &value)
    } else if let Some(condition_text) = args
        .first()
        .and_then(|value| value.as_string())
        .map(|s| s.to_std_string_escaped())
    {
        CSS::supports_condition(&condition_text)
    } else {
        false
    };

    Ok(JsValue::from(result))
}
