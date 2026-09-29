use js_engine::gc_struct;
/// <https://webidl.spec.whatwg.org/#idl-DOMException>
#[gc_struct]
pub struct DOMException {
    /// <https://webidl.spec.whatwg.org/#dom-domexception-message>
    #[ignore_trace]
    message: String,

    /// <https://webidl.spec.whatwg.org/#dom-domexception-name>
    #[ignore_trace]
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
    pub(crate) fn invalid_character_error() -> Self {
        Self::new(String::new(), String::from("InvalidCharacterError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn in_use_attribute_error() -> Self {
        Self::new(String::new(), String::from("InUseAttributeError"))
    }

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn namespace_error() -> Self {
        Self::new(String::new(), String::from("NamespaceError"))
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

    /// <https://webidl.spec.whatwg.org/#dfn-error-names-table>
    pub(crate) fn no_modification_allowed_error() -> Self {
        Self::new(String::new(), String::from("NoModificationAllowedError"))
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
        // The code getter steps are to return the legacy code indicated in
        // the error names table for this's name, or 0 if no such entry
        // exists in the table.
        // <https://webidl.spec.whatwg.org/#dfn-error-names-table>
        match self.name.as_str() {
            "IndexSizeError" => 1,
            "HierarchyRequestError" => 3,
            "WrongDocumentError" => 4,
            "InvalidCharacterError" => 5,
            "NoModificationAllowedError" => 7,
            "NotFoundError" => 8,
            "NotSupportedError" => 9,
            "InUseAttributeError" => 10,
            "InvalidStateError" => 11,
            "SyntaxError" => 12,
            "InvalidModificationError" => 13,
            "NamespaceError" => 14,
            "InvalidAccessError" => 15,
            "TypeMismatchError" => 17,
            "SecurityError" => 18,
            "NetworkError" => 19,
            "AbortError" => 20,
            "URLMismatchError" => 21,
            "TimeoutError" => 23,
            "InvalidNodeTypeError" => 24,
            "DataCloneError" => 25,
            _ => 0,
        }
    }
}
