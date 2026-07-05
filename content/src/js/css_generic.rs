use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::css::CSS;
use crate::js::Types;

/// <https://drafts.csswg.org/css-conditional-3/#the-css-namespace>
///
/// Installs the `CSS` namespace on the global object using only the
/// generic [`ExecutionContext`] trait — no engine-specific APIs.
///
/// Uses `create_builtin_function` (closure path) instead of
/// `create_builtin_function_from_behaviour` (Behaviour trait object path)
/// because the Behaviour trait object path causes a SIGSEGV on the JSC
/// backend.  Since CSS.supports() carries no captures, the closure path
/// is equivalent and works on all backends.
/// <https://drafts.csswg.org/css-conditional-3/#dom-css-supports-conditiontext-conditiontext>
pub(crate) fn install_css_namespace(ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
    // Create the CSS namespace object.
    let css_obj = ec.create_plain_object(None);

    // Install the `supports` method.
    let fn_obj = ec.create_builtin_function(
        Box::new(|args, _this, ec| {
            let result = if args.len() >= 2 {
                // Invoked as supports(property, value) — 2 required arguments.
                let property = ec.to_rust_string(args[0].clone()).unwrap_or_default();
                let value = ec.to_rust_string(args[1].clone()).unwrap_or_default();
                CSS::supports(&property, &value)
            } else if args.len() >= 1 {
                // Invoked as supports(conditionText) — 1 required argument.
                let condition_text = ec.to_rust_string(args[0].clone()).unwrap_or_default();
                CSS::supports_condition(&condition_text)
            } else {
                false
            };

            Ok(ec.value_from_bool(result))
        }),
        2,
        ec.property_key_from_str("supports"),
    );

    ec.set(
        css_obj.clone(),
        ec.property_key_from_str("supports"),
        <Types as JsTypes>::value_from_object(<Types as JsTypes>::object_from_function(fn_obj)),
        false,
    )?;

    // Register on global.
    let global = ec.realm_global_object();
    ec.set(
        global,
        ec.property_key_from_str("CSS"),
        <Types as JsTypes>::value_from_object(css_obj),
        false,
    )
}
