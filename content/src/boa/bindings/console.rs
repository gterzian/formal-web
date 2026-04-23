use boa_engine::{
    Context, JsResult, JsValue, js_string, native_function::NativeFunction,
    object::ObjectInitializer, property::Attribute,
};

enum ConsoleSink {
    Stdout,
    Stderr,
}

/// <https://console.spec.whatwg.org/#namespacedef-console>
pub(crate) fn install_console_namespace(context: &mut Context) -> JsResult<()> {
    let console = {
        let mut initializer = ObjectInitializer::new(context);
        initializer.function(NativeFunction::from_fn_ptr(log), js_string!("log"), 0);
        initializer.function(NativeFunction::from_fn_ptr(info), js_string!("info"), 0);
        initializer.function(NativeFunction::from_fn_ptr(debug), js_string!("debug"), 0);
        initializer.function(NativeFunction::from_fn_ptr(warn), js_string!("warn"), 0);
        initializer.function(NativeFunction::from_fn_ptr(error), js_string!("error"), 0);
        initializer.build()
    };

    context.register_global_property(js_string!("console"), console, Attribute::all())
}

/// <https://console.spec.whatwg.org/#log>
/// Note: This implements `console.log()` by running the `Logger("log", data)` algorithm inline.
fn log(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "Perform Logger(\"log\", data)."
    logger(ConsoleSink::Stdout, args)
}

/// <https://console.spec.whatwg.org/#info>
/// Note: This implements `console.info()` by running the `Logger("info", data)` algorithm inline.
fn info(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "Perform Logger(\"info\", data)."
    logger(ConsoleSink::Stdout, args)
}

/// <https://console.spec.whatwg.org/#debug>
/// Note: This implements `console.debug()` by running the `Logger("debug", data)` algorithm inline.
fn debug(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "Perform Logger(\"debug\", data)."
    logger(ConsoleSink::Stdout, args)
}

/// <https://console.spec.whatwg.org/#warn>
/// Note: This implements `console.warn()` by running the `Logger("warn", data)` algorithm inline.
fn warn(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "Perform Logger(\"warn\", data)."
    logger(ConsoleSink::Stderr, args)
}

/// <https://console.spec.whatwg.org/#error>
/// Note: This implements `console.error()` by running the `Logger("error", data)` algorithm inline.
fn error(_this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "Perform Logger(\"error\", data)."
    logger(ConsoleSink::Stderr, args)
}

/// <https://console.spec.whatwg.org/#logger>
fn logger(sink: ConsoleSink, args: &[JsValue]) -> JsResult<JsValue> {
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
        print_console_line(&sink, format_console_argument(first));
        return Ok(JsValue::undefined());
    }

    // Step 5: "Otherwise, perform Printer(logLevel, Formatter(args))."
    // Note: The content runtime does not yet model the Console Standard's separate Printer and Formatter abstractions; it joins the rendered arguments with spaces before printing.
    let rendered = args
        .iter()
        .map(format_console_argument)
        .collect::<Vec<_>>()
        .join(" ");
    print_console_line(&sink, rendered);

    // Step 6: "Return undefined."
    Ok(JsValue::undefined())
}

fn print_console_line(sink: &ConsoleSink, line: String) {
    match sink {
        ConsoleSink::Stdout => println!("{line}"),
        ConsoleSink::Stderr => eprintln!("{line}"),
    }
}

fn format_console_argument(value: &JsValue) -> String {
    if let Some(string) = value.as_string() {
        string.to_std_string_escaped()
    } else {
        value.display().to_string()
    }
}
