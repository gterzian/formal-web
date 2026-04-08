use boa_engine::{
    Context, JsResult, JsValue, js_string, native_function::NativeFunction,
    object::ObjectInitializer, property::Attribute,
};

/// <https://console.spec.whatwg.org/#namespacedef-console>
pub(crate) fn install_console_namespace(context: &mut Context) -> JsResult<()> {
    let console = {
        let mut initializer = ObjectInitializer::new(context);
        initializer.function(NativeFunction::from_fn_ptr(log), js_string!("log"), 0);
        initializer.build()
    };

    context.register_global_property(js_string!("console"), console, Attribute::all())
}

/// <https://console.spec.whatwg.org/#log>
/// Note: This implements `console.log()` by running the `Logger("log", data)` algorithm inline.
fn log(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "Perform Logger(\"log\", data)."
    logger(args)
}

/// <https://console.spec.whatwg.org/#logger>
fn logger(args: &[JsValue]) -> JsResult<JsValue> {
    // Step 1: "If args is empty, return."
    if args.is_empty() {
        return Ok(JsValue::undefined());
    }

    // Step 2: "Let first be args[0]."
    let first = &args[0];

    // Step 3: "Let rest be all elements following first in args."
    let rest = &args[1..];

    // Step 4: "If rest is empty, perform Printer(logLevel, « first ») and return."
    // Note: The content runtime uses `println!` as the implementation-defined console side effect.
    if rest.is_empty() {
        println!("{}", format_console_argument(first));
        return Ok(JsValue::undefined());
    }

    // Step 5: "Otherwise, perform Printer(logLevel, Formatter(args))."
    // Note: The content runtime does not yet model the Console Standard's separate Printer and Formatter abstractions; it joins the rendered arguments with spaces before printing.
    let rendered = args
        .iter()
        .map(format_console_argument)
        .collect::<Vec<_>>()
        .join(" ");
    println!("{rendered}");

    // Step 6: "Return undefined."
    Ok(JsValue::undefined())
}

fn format_console_argument(value: &JsValue) -> String {
    if let Some(string) = value.as_string() {
        string.to_std_string_escaped()
    } else {
        value.display().to_string()
    }
}
