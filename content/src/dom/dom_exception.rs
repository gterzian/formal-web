use js_engine::gc_struct;
/// <https://webidl.spec.whatwg.org/#idl-DOMException>
#[gc_struct]
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

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn hierarchy_request_error() -> Self {
        Self::new(String::new(), String::from("HierarchyRequestError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn not_found_error() -> Self {
        Self::new(String::new(), String::from("NotFoundError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn not_supported_error() -> Self {
        Self::new(String::new(), String::from("NotSupportedError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn syntax_error() -> Self {
        Self::new(String::new(), String::from("SyntaxError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn security_error() -> Self {
        Self::new(String::new(), String::from("SecurityError"))
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
            "HierarchyRequestError" => 3,
            "InvalidCharacterError" => 5,
            "NotFoundError" => 8,
            "NotSupportedError" => 9,
            "SyntaxError" => 12,
            "SecurityError" => 18,
            "AbortError" => 20,
            "TimeoutError" => 23,
            _ => 0,
        }
    }
}
