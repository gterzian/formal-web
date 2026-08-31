//! <https://webidl.spec.whatwg.org/#dfn-throw>
//!
//! DOMException values are created here so domain code (html, streams, …)
//! never constructs DOMException platform objects directly.

use crate::dom::DOMException;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use js_engine::{ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

/// <https://webidl.spec.whatwg.org/#dfn-DOMException>
fn dom_exception_value(
    message: String,
    name: String,
    ec: &mut dyn ExecutionContext<Types>,
) -> JsValue {
    let obj =
        create_interface_instance::<Types, DOMException>(DOMException::new(message, name), ec)
            .expect("DOMException construction should not fail");
    Types::value_from_object(obj)
}

/// <https://webidl.spec.whatwg.org/#syntaxerror>
pub(crate) fn syntax_error_value(ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    dom_exception_value(String::new(), String::from("SyntaxError"), ec)
}

/// <https://webidl.spec.whatwg.org/#securityerror>
pub(crate) fn security_error_value(ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    dom_exception_value(String::new(), String::from("SecurityError"), ec)
}

/// <https://webidl.spec.whatwg.org/#datacloneerror>
pub(crate) fn data_clone_error_value(ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    dom_exception_value(
        String::from("The object could not be cloned."),
        String::from("DataCloneError"),
        ec,
    )
}
