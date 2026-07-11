use js_engine::{Completion, ExecutionContext};
use js_engine::gc_struct;

use crate::js::Types;

/// <https://console.spec.whatwg.org/#namespacedef-console>
///
/// Installs the `console` namespace on the global object using only the
/// generic [`ExecutionContext`] trait — no engine-specific APIs.
pub(crate) fn install_console_namespace(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: Create the console namespace object.
    let console_obj = ec.create_plain_object(None);

    // Step 2: Install each console method.
    install_console_method(ec, &console_obj, "log", stdout_sink)?;
    install_console_method(ec, &console_obj, "info", stdout_sink)?;
    install_console_method(ec, &console_obj, "debug", stdout_sink)?;
    install_console_method(ec, &console_obj, "warn", stderr_sink)?;
    install_console_method(ec, &console_obj, "error", stderr_sink)?;

    // Step 3: Register on global.
    let global = ec.realm_global_object();
    ec.set(
        global,
        ec.property_key_from_str("console"),
        <Types as js_engine::JsTypes>::value_from_object(console_obj),
        false,
    )
}

#[gc_struct]
struct ConsoleCapture {
    #[ignore_trace]
    sink: fn(&str),
}

fn console_fn(
    args: &[<Types as js_engine::JsTypes>::JsValue],
    _this: <Types as js_engine::JsTypes>::JsValue,
    captures: &ConsoleCapture,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as js_engine::JsTypes>::JsValue, Types> {
    // <https://console.spec.whatwg.org/#logger>
    //
    // Step 1: "If args is empty, return."
    if args.is_empty() {
        return Ok(ec.value_undefined());
    }

    // Step 2-3: "Let first be args[0]. Let rest be all elements
    // following first in args."
    let rest = &args[1..];

    // Step 4: "If rest is empty, perform Printer(logLevel, « first »)
    // and return."
    if rest.is_empty() {
        let rendered = ec.to_rust_string(args[0].clone())?;
        (captures.sink)(&rendered);
        return Ok(ec.value_undefined());
    }

    // Step 5: "Otherwise, perform Printer(logLevel, Formatter(args))."
    let mut rendered = String::new();
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            rendered.push(' ');
        }
        rendered.push_str(&ec.to_rust_string(arg.clone())?);
    }
    (captures.sink)(&rendered);

    // Step 6: "Return undefined."
    Ok(ec.value_undefined())
}

/// Install a single console method (log/info/debug/warn/error) on the console object.
fn install_console_method(
    ec: &mut dyn ExecutionContext<Types>,
    console_obj: &<Types as js_engine::JsTypes>::JsObject,
    method_name: &str,
    sink: fn(&str),
) -> Completion<(), Types> {
    let set_key = ec.property_key_from_str(method_name);
    let name_key = set_key.clone();
    let fn_obj = {
        let capture = ConsoleCapture { sink };
        crate::js::create_builtin_fn_with_traced_captures(
            ec,
            capture,
            console_fn,
            0,
            name_key,
            false,
        )
    };

    ec.set(
        console_obj.clone(),
        set_key.clone(),
        <Types as js_engine::JsTypes>::value_from_object(
            <Types as js_engine::JsTypes>::object_from_function(fn_obj),
        ),
        false,
    )
}

use std::io::Write;

fn stdout_sink(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

fn stderr_sink(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}
