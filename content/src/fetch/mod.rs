//
// Event-loop side implementation.
// <https://fetch.spec.whatwg.org/>
//
pub(crate) mod body;
pub(crate) mod fetch_method;
pub(crate) mod headers;
pub(crate) mod request;
pub(crate) mod response;

pub(crate) use body::{BodyInit, BodyMixin};
pub(crate) use fetch_method::{
    FetchRequestSnapshot, FetchResponseData, PendingFetch, abort_fetch, fetch, process_response,
};
pub(crate) use headers::{Headers, HeadersGuard, HeadersInit};
pub(crate) use request::{Request as FetchApiRequest, RequestInfo, RequestInit};
pub(crate) use response::{Response, ResponseInit};

use blitz_traits::net::Request;
use serde::{Deserialize, Serialize};

/// The header list a fetch carries to the net process or to the embedder.
/// <https://fetch.spec.whatwg.org/#concept-request-header-list>
pub(crate) fn request_header_list(request: &Request) -> Vec<(String, String)> {
    let mut header_list: Vec<(String, String)> = request
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    // Note: blitz carries the content type of a request body outside the
    // header list; the net process and the embedder see one list, so it is
    // appended here.
    if let Some(content_type) = &request.content_type
        && !header_list
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        header_list.push((String::from("content-type"), content_type.clone()));
    }
    header_list
}

/// <https://fetch.spec.whatwg.org/#concept-header-list>
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HeaderList {
    /// <https://fetch.spec.whatwg.org/#concept-header-list>
    headers: Vec<(String, String)>,
}

impl HeaderList {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn pairs(&self) -> &[(String, String)] {
        &self.headers
    }

    pub(crate) fn clear(&mut self) {
        self.headers.clear();
    }

    /// <https://fetch.spec.whatwg.org/#header-list-contains>
    pub(crate) fn contains(&self, name: &str) -> bool {
        // A header list list contains a header name name if list contains a
        // header whose name is a byte-case-insensitive match for name.
        self.headers
            .iter()
            .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-get>
    pub(crate) fn get(&self, name: &str) -> Option<String> {
        // Step 1: If list does not contain name, then return null.
        if !self.contains(name) {
            return None;
        }

        // Step 2: Return the values of all headers in list whose name is a
        // byte-case-insensitive match for name, separated from each other by
        // 0x2C 0x20, in order.
        Some(
            self.headers
                .iter()
                .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-delete>
    pub(crate) fn delete(&mut self, name: &str) {
        // To delete a header name name from a header list list, remove all
        // headers whose name is a byte-case-insensitive match for name from
        // list.
        self.headers
            .retain(|(header_name, _)| !header_name.eq_ignore_ascii_case(name));
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-set>
    pub(crate) fn set(&mut self, name: &str, value: &str) {
        // Step 1: If list contains name, then set the value of the first such
        // header to value and remove the others.
        if let Some(first) = self
            .headers
            .iter()
            .position(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        {
            self.headers[first].1 = value.to_owned();
            let mut index = 0;
            self.headers.retain(|(header_name, _)| {
                let keep = index == first || !header_name.eq_ignore_ascii_case(name);
                index += 1;
                keep
            });
            return;
        }

        // Step 2: Otherwise, append (name, value) to list.
        self.headers.push((name.to_owned(), value.to_owned()));
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-append>
    pub(crate) fn append(&mut self, name: &str, value: &str) {
        // Step 1: If list contains name, then set name to the first such
        // header's name.
        let name = self
            .headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(header_name, _)| header_name.clone())
            .unwrap_or_else(|| name.to_owned());

        // Step 2: Append (name, value) to list.
        self.headers.push((name, value.to_owned()));
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-sort-and-combine>
    pub(crate) fn sort_and_combine(&self) -> Vec<(String, String)> {
        // Step 1: Let headers be an empty list of headers with the key being
        // the name and value the value.
        let mut headers = Vec::new();

        // Step 2: Let names be the result of convert header names to a
        // sorted-lowercase set given list's names.
        let names = convert_header_names_to_a_sorted_lowercase_set(
            self.headers.iter().map(|(name, _)| name.as_str()),
        );

        // Step 3: For each name of names:
        for name in names {
            // Step 3.1: If name is `set-cookie`, then:
            if name == "set-cookie" {
                // Step 3.1.1: Let values be a list of all values of headers
                // in list whose name is a byte-case-insensitive match for
                // name, in order.
                // Step 3.1.2: For each value of values: Append (name, value)
                // to headers.
                for (_, value) in self
                    .headers
                    .iter()
                    .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(&name))
                {
                    headers.push((name.clone(), value.clone()));
                }
                continue;
            }

            // Step 3.2: Otherwise:
            // Step 3.2.1: Let value be the result of getting name from list.
            // Step 3.2.2: Assert: value is non-null.
            // Step 3.2.3: Append (name, value) to headers.
            if let Some(value) = self.get(&name) {
                headers.push((name, value));
            }
        }

        // Step 4: Return headers.
        headers
    }
}

/// <https://fetch.spec.whatwg.org/#convert-header-names-to-a-sorted-lowercase-set>
fn convert_header_names_to_a_sorted_lowercase_set<'a>(
    header_names: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    // Step 1: Let headerNamesSet be a new ordered set.
    let mut header_names_set: Vec<String> = Vec::new();

    // Step 2: For each name of headerNames, append the result of
    // byte-lowercasing name to headerNamesSet.
    for name in header_names {
        let lowercased = name.to_ascii_lowercase();
        if !header_names_set.contains(&lowercased) {
            header_names_set.push(lowercased);
        }
    }

    // Step 3: Return the result of sorting headerNamesSet in ascending order
    // with byte less than.
    header_names_set.sort();
    header_names_set
}

/// <https://fetch.spec.whatwg.org/#header-name>
pub(crate) fn is_header_name(name: &str) -> bool {
    // A header name is a byte sequence that matches the field-name token
    // production.
    !name.is_empty() && name.bytes().all(is_token_byte)
}

/// <https://fetch.spec.whatwg.org/#header-value>
pub(crate) fn is_header_value(value: &str) -> bool {
    // A header value is a byte sequence that matches the following
    // conditions: Has no leading or trailing HTTP tab or space bytes.
    // Contains no 0x00 (NUL) or HTTP newline bytes.
    let bytes = value.as_bytes();
    let untrimmed_ends = bytes
        .first()
        .is_some_and(|byte| matches!(byte, b'\t' | b' '))
        || bytes
            .last()
            .is_some_and(|byte| matches!(byte, b'\t' | b' '));
    !untrimmed_ends && !bytes.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
}

/// <https://fetch.spec.whatwg.org/#concept-header-value-normalize>
pub(crate) fn normalize_header_value(value: &str) -> String {
    // To normalize a byte sequence potentialValue, remove any leading and
    // trailing HTTP whitespace bytes from potentialValue.
    value
        .trim_matches(|character: char| matches!(character, '\t' | '\n' | '\r' | ' '))
        .to_owned()
}

/// <https://fetch.spec.whatwg.org/#forbidden-request-header>
pub(crate) fn is_forbidden_request_header(name: &str, value: &str) -> bool {
    // Step 1: If name is a byte-case-insensitive match for one of: ... then
    // return true.
    const FORBIDDEN_NAMES: [&str; 21] = [
        "accept-charset",
        "accept-encoding",
        "access-control-request-headers",
        "access-control-request-method",
        "connection",
        "content-length",
        "cookie",
        "cookie2",
        "date",
        "dnt",
        "expect",
        "host",
        "keep-alive",
        "origin",
        "referer",
        "set-cookie",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "via",
    ];
    if FORBIDDEN_NAMES
        .iter()
        .any(|forbidden| forbidden.eq_ignore_ascii_case(name))
    {
        return true;
    }

    // Step 2: If name when byte-lowercased starts with `proxy-` or `sec-`,
    // then return true.
    let lowercased = name.to_ascii_lowercase();
    if lowercased.starts_with("proxy-") || lowercased.starts_with("sec-") {
        return true;
    }

    // Step 3: If name is a byte-case-insensitive match for one of:
    // `X-HTTP-Method`, `X-HTTP-Method-Override`, `X-Method-Override`, then:
    if [
        "x-http-method",
        "x-http-method-override",
        "x-method-override",
    ]
    .iter()
    .any(|override_name| override_name.eq_ignore_ascii_case(name))
    {
        // Step 3.1: Let parsedValues be the result of getting, decoding, and
        // splitting value.
        // Step 3.2: For each method of parsedValues: if the isomorphic encoding
        // of method is a forbidden method, then return true.
        if get_decode_and_split_value(value)
            .iter()
            .any(|method| is_forbidden_method(method))
        {
            return true;
        }
    }

    // Step 4: Return false.
    false
}

/// <https://fetch.spec.whatwg.org/#header-value-get-decode-and-split>
fn get_decode_and_split_value(value: &str) -> Vec<String> {
    // Step 1: Let input be the result of isomorphic decoding value.
    // Step 2: Let position be a position variable for input, initially
    // pointing at the start of input.
    // Step 3: Let values be a list of strings, initially « ».
    // Step 4: Let temporaryValue be the empty string.
    // Step 5: While true:
    // Step 5.1: Append the result of collecting a sequence of code points
    // that are not U+0022 (") or U+002C (,) from input, given position, to
    // temporaryValue.
    // Step 5.2: If position is not past the end of input and the code point
    // at position within input is U+0022 ("): append the result of collecting
    // an HTTP quoted string from input, given position, to temporaryValue;
    // if position is not past the end of input, then continue.
    // Step 5.3: Remove all HTTP tab or space from the start and end of
    // temporaryValue.
    // Step 5.4: Append temporaryValue to values.
    // Step 5.5: Set temporaryValue to the empty string.
    // Step 5.6: If position is past the end of input, then return values.
    // Step 5.7: Assert: the code point at position within input is U+002C
    // (,).
    // Step 5.8: Advance position by 1.
    let mut values = Vec::new();
    let mut temporary_value = String::new();
    let mut characters = value.chars().peekable();
    loop {
        while let Some(&character) = characters.peek() {
            if character == '"' || character == ',' {
                break;
            }
            temporary_value.push(character);
            characters.next();
        }
        if characters.peek() == Some(&'"') {
            temporary_value.push('"');
            characters.next();
            while let Some(character) = characters.next() {
                temporary_value.push(character);
                if character == '\\' {
                    if let Some(escaped) = characters.next() {
                        temporary_value.push(escaped);
                    }
                    continue;
                }
                if character == '"' {
                    break;
                }
            }
            if characters.peek().is_some() {
                continue;
            }
        }
        values.push(
            temporary_value
                .trim_matches(|character: char| character == '\t' || character == ' ')
                .to_owned(),
        );
        temporary_value.clear();
        if characters.peek().is_none() {
            return values;
        }
        characters.next();
    }
}

/// <https://fetch.spec.whatwg.org/#no-cors-safelisted-request-header-name>
pub(crate) fn is_no_cors_safelisted_request_header_name(name: &str) -> bool {
    // A no-CORS-safelisted request-header name is a header name that is a
    // byte-case-insensitive match for one of: `Accept`, `Accept-Language`,
    // `Content-Language`, `Content-Type`.
    [
        "accept",
        "accept-language",
        "content-language",
        "content-type",
    ]
    .iter()
    .any(|safelisted| safelisted.eq_ignore_ascii_case(name))
}

/// <https://fetch.spec.whatwg.org/#no-cors-safelisted-request-header>
pub(crate) fn is_no_cors_safelisted_request_header(name: &str, value: &str) -> bool {
    // Step 1: If name is not a no-CORS-safelisted request-header name, then
    // return false.
    if !is_no_cors_safelisted_request_header_name(name) {
        return false;
    }

    // Step 2: Return whether (name, value) is a CORS-safelisted
    // request-header.
    is_cors_safelisted_request_header(name, value)
}

/// <https://fetch.spec.whatwg.org/#cors-safelisted-request-header>
pub(crate) fn is_cors_safelisted_request_header(name: &str, value: &str) -> bool {
    // Step 1: If value's length is greater than 128, then return false.
    if value.len() > 128 {
        return false;
    }

    // Step 2: Byte-lowercase name and switch on the result:
    let lowercased = name.to_ascii_lowercase();
    let is_cors_unsafe_request_header_byte = |byte: u8| {
        byte < 0x20 && byte != 0x09
            || matches!(
                byte,
                b'"' | b'('
                    | b')'
                    | b':'
                    | b'<'
                    | b'>'
                    | b'?'
                    | b'@'
                    | b'['
                    | b'\\'
                    | b']'
                    | b'{'
                    | b'}'
                    | 0x7F
            )
    };
    match lowercased.as_str() {
        // `accept`: If value contains a CORS-unsafe request-header byte, then
        // return false.
        "accept" => !value.bytes().any(is_cors_unsafe_request_header_byte),
        // `accept-language`, `content-language`: If value contains a byte
        // that is not in the range 0x30 (0) to 0x39 (9), inclusive, is not in
        // the range 0x41 (A) to 0x5A (Z), inclusive, is not in the range 0x61
        // (a) to 0x7A (z), inclusive, and is not 0x20 (SP), 0x2A (*), 0x2C
        // (,), 0x2D (-), 0x2E (.), 0x3B (;), or 0x3D (=), then return false.
        "accept-language" | "content-language" => value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b' ' | b'*' | b',' | b'-' | b'.' | b';' | b'=')
        }),
        // `content-type`: If value contains a CORS-unsafe request-header
        // byte, then return false. Let mimeType be the result of parsing the
        // result of isomorphic decoding value. If mimeType is failure, then
        // return false. If mimeType's essence is not
        // "application/x-www-form-urlencoded", "multipart/form-data", or
        // "text/plain", then return false.
        "content-type" => {
            if value.bytes().any(is_cors_unsafe_request_header_byte) {
                return false;
            }
            let essence = value
                .split(';')
                .next()
                .unwrap_or("")
                .trim_matches(|character: char| character == '\t' || character == ' ')
                .to_ascii_lowercase();
            matches!(
                essence.as_str(),
                "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
            )
        }
        // `range`: ... (not a no-CORS-safelisted name; the range check is
        // not needed here.)
        // Otherwise: Return false.
        _ => false,
    }
}

/// <https://fetch.spec.whatwg.org/#privileged-no-cors-request-header-name>
pub(crate) fn is_privileged_no_cors_request_header_name(name: &str) -> bool {
    // A privileged no-CORS request-header name is a header name that is a
    // byte-case-insensitive match for one of: `Range`.
    name.eq_ignore_ascii_case("range")
}

/// <https://fetch.spec.whatwg.org/#forbidden-response-header-name>
pub(crate) fn is_forbidden_response_header_name(name: &str) -> bool {
    // A forbidden response-header name is a header name that is a
    // byte-case-insensitive match for one of: `Set-Cookie`, `Set-Cookie2`.
    name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("set-cookie2")
}

/// <https://fetch.spec.whatwg.org/#concept-method>
pub(crate) fn is_method(method: &str) -> bool {
    // A method is a byte sequence that matches the method token production.
    !method.is_empty() && method.bytes().all(is_token_byte)
}

/// <https://fetch.spec.whatwg.org/#forbidden-method>
pub(crate) fn is_forbidden_method(method: &str) -> bool {
    // A forbidden method is a method that is a byte-case-insensitive match
    // for `CONNECT`, `TRACE`, or `TRACK`.
    ["CONNECT", "TRACE", "TRACK"]
        .iter()
        .any(|forbidden| forbidden.eq_ignore_ascii_case(method))
}

/// <https://fetch.spec.whatwg.org/#concept-method-normalize>
pub(crate) fn normalize_method(method: &str) -> String {
    // To normalize a method, if it is a byte-case-insensitive match for
    // `DELETE`, `GET`, `HEAD`, `OPTIONS`, `POST`, or `PUT`, byte-uppercase
    // it.
    if ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"]
        .iter()
        .any(|known| known.eq_ignore_ascii_case(method))
    {
        method.to_ascii_uppercase()
    } else {
        method.to_owned()
    }
}

/// <https://fetch.spec.whatwg.org/#null-body-status>
pub(crate) fn is_null_body_status(status: u16) -> bool {
    // A null body status is a status that is 101, 103, 204, 205, or 304.
    matches!(status, 101 | 103 | 204 | 205 | 304)
}

/// <https://fetch.spec.whatwg.org/#redirect-status>
pub(crate) fn is_redirect_status(status: u16) -> bool {
    // A redirect status is a status that is 301, 302, 303, 307, or 308.
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// <https://httpwg.org/specs/rfc9110.html#tokens>
fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}
