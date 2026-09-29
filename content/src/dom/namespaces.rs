use super::DOMException;

/// <https://infra.spec.whatwg.org/#xml-namespace>
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// <https://infra.spec.whatwg.org/#xmlns-namespace>
pub(crate) const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// <https://dom.spec.whatwg.org/#validate-and-extract>
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidateAndExtractContext {
    Attribute,
    Element,
}

fn contains_ascii_whitespace_null_slash_or_greater_than(name: &str) -> bool {
    name.chars().any(|code_point| {
        code_point.is_ascii_whitespace() || matches!(code_point, '\0' | '/' | '>')
    })
}

/// <https://dom.spec.whatwg.org/#valid-namespace-prefix>
pub(crate) fn is_a_valid_namespace_prefix(prefix: &str) -> bool {
    // "A string is a valid namespace prefix if its length is at least 1 and it does not contain ASCII whitespace, U+0000 NULL, U+002F (/), or U+003E (>)."
    !prefix.is_empty() && !contains_ascii_whitespace_null_slash_or_greater_than(prefix)
}

/// <https://dom.spec.whatwg.org/#valid-attribute-local-name>
pub(crate) fn is_a_valid_attribute_local_name(local_name: &str) -> bool {
    // "A string is a valid attribute local name if its length is at least 1 and it does not contain ASCII whitespace, U+0000 NULL, U+002F (/), U+003D (=), or U+003E (>)."
    !local_name.is_empty()
        && !contains_ascii_whitespace_null_slash_or_greater_than(local_name)
        && !local_name.contains('=')
}

/// <https://dom.spec.whatwg.org/#valid-element-local-name>
pub(crate) fn is_a_valid_element_local_name(name: &str) -> bool {
    // Step 1: "If name’s length is 0, then return false."
    let mut code_points = name.chars();
    let Some(first) = code_points.next() else {
        return false;
    };

    // Step 2: "If name’s 0th code point is an ASCII alpha, then:"
    if first.is_ascii_alphabetic() {
        // Step 2.1: "If name contains ASCII whitespace, U+0000 NULL, U+002F (/), or U+003E (>), then return false."
        if contains_ascii_whitespace_null_slash_or_greater_than(name) {
            return false;
        }

        // Step 2.2: "Return true."
        return true;
    }

    // Step 3: "If name’s 0th code point is not U+003A (:), U+005F (_), or in the range U+0080 to U+10FFFF, inclusive, then return false."
    if !(first == ':' || first == '_' || first as u32 >= 0x80) {
        return false;
    }

    // Step 4: "If name’s subsequent code points, if any, are not ASCII alphas, ASCII digits, U+002D (-), U+002E (.), U+003A (:), U+005F (_), or in the range U+0080 to U+10FFFF, inclusive, then return false."
    if code_points.any(|code_point| {
        !(code_point.is_ascii_alphanumeric()
            || matches!(code_point, ':' | '_' | '-' | '.')
            || code_point as u32 >= 0x80)
    }) {
        return false;
    }

    // Step 5: "Return true."
    true
}

/// <https://dom.spec.whatwg.org/#validate-and-extract>
pub(crate) fn validate_and_extract(
    namespace: Option<&str>,
    qualified_name: &str,
    context: ValidateAndExtractContext,
) -> Result<(Option<String>, Option<String>, String), DOMException> {
    // Step 1: "If namespace is the empty string, then set it to null."
    let namespace = namespace.filter(|namespace| !namespace.is_empty());

    // Step 2: "Let prefix be null."
    let mut prefix: Option<&str> = None;

    // Step 3: "Let localName be qualifiedName."
    let mut local_name = qualified_name;

    // Step 4: "If qualifiedName contains a U+003A (:):"
    if let Some((before, after)) = qualified_name.split_once(':') {
        // Step 4.1: "Set prefix to the part of qualifiedName before the first U+003A (:)."
        prefix = Some(before);

        // Step 4.2: "Set localName to the part of qualifiedName after the first U+003A (:)."
        local_name = after;

        // Step 4.3: "If prefix is not a valid namespace prefix, then throw an "InvalidCharacterError" DOMException."
        if !is_a_valid_namespace_prefix(before) {
            return Err(DOMException::invalid_character_error());
        }
    }

    // Step 5: "Assert: prefix is either null or a valid namespace prefix."

    // Step 6: "If context is "attribute" and localName is not a valid attribute local name, then throw an "InvalidCharacterError" DOMException."
    if context == ValidateAndExtractContext::Attribute
        && !is_a_valid_attribute_local_name(local_name)
    {
        return Err(DOMException::invalid_character_error());
    }

    // Step 7: "If context is "element" and localName is not a valid element local name, then throw an "InvalidCharacterError" DOMException."
    if context == ValidateAndExtractContext::Element && !is_a_valid_element_local_name(local_name) {
        return Err(DOMException::invalid_character_error());
    }

    // Step 8: "If prefix is non-null and namespace is null, then throw a "NamespaceError" DOMException."
    if prefix.is_some() && namespace.is_none() {
        return Err(DOMException::namespace_error());
    }

    // Step 9: "If prefix is "xml" and namespace is not the XML namespace, then throw a "NamespaceError" DOMException."
    if prefix == Some("xml") && namespace != Some(XML_NAMESPACE) {
        return Err(DOMException::namespace_error());
    }

    // Step 10: "If either qualifiedName or prefix is "xmlns" and namespace is not the XMLNS namespace, then throw a "NamespaceError" DOMException."
    if (qualified_name == "xmlns" || prefix == Some("xmlns")) && namespace != Some(XMLNS_NAMESPACE)
    {
        return Err(DOMException::namespace_error());
    }

    // Step 11: "If namespace is the XMLNS namespace and neither qualifiedName nor prefix is "xmlns", then throw a "NamespaceError" DOMException."
    if namespace == Some(XMLNS_NAMESPACE) && qualified_name != "xmlns" && prefix != Some("xmlns") {
        return Err(DOMException::namespace_error());
    }

    // Step 12: "Return (namespace, prefix, localName)."
    Ok((
        namespace.map(str::to_owned),
        prefix.map(str::to_owned),
        local_name.to_owned(),
    ))
}
