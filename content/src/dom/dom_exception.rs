use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

/// <https://webidl.spec.whatwg.org/#idl-DOMException>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct DOMException {
    /// <https://webidl.spec.whatwg.org/#dom-domexception-message>
    #[unsafe_ignore_trace]
    message: String,

    /// <https://webidl.spec.whatwg.org/#dom-domexception-name>
    #[unsafe_ignore_trace]
    name: String,
}

impl DOMException {
    pub(crate) fn new(message: String, name: String) -> Self {
        Self { message, name }
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn abort_error() -> Self {
        Self::new(String::new(), String::from("AbortError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn timeout_error() -> Self {
        Self::new(String::new(), String::from("TimeoutError"))
    }

    /// <https://webidl.spec.whatwg.org/#dom-domexception-message>
    pub(crate) fn message_value(&self) -> &str {
        &self.message
    }

    /// <https://webidl.spec.whatwg.org/#dom-domexception-name>
    pub(crate) fn name_value(&self) -> &str {
        &self.name
    }

    /// <https://webidl.spec.whatwg.org/#dom-domexception-code>
    pub(crate) fn code_value(&self) -> u16 {
        match self.name.as_str() {
            "AbortError" => 20,
            "TimeoutError" => 23,
            _ => 0,
        }
    }
}
