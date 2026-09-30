//! Encoding Standard (<https://encoding.spec.whatwg.org/>): UTF-8 encoding
//! and decoding, and the TextEncoder and TextDecoder interfaces.

pub(crate) mod text_decoder;
pub(crate) mod text_encoder;

pub(crate) use text_decoder::TextDecoder;
pub(crate) use text_encoder::TextEncoder;

/// <https://encoding.spec.whatwg.org/#utf-8-encode>
pub(crate) fn utf_8_encode(string: &str) -> Vec<u8> {
    // Note: A Rust string is UTF-8: its bytes are the encoder's output.
    string.as_bytes().to_vec()
}

/// <https://encoding.spec.whatwg.org/#utf-8-decode>
pub(crate) fn utf_8_decode(bytes: &[u8]) -> String {
    // Step 1: Let buffer be the result of peeking three bytes from ioQueue,
    // converted to a byte sequence.
    // Step 2: If buffer is 0xEF 0xBB 0xBF, then read three bytes from
    // ioQueue. (Do nothing with those bytes.)
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    // Step 3: Process a queue with an instance of UTF-8's decoder, ioQueue,
    // output, and "replacement".
    // Step 4: Return output.
    String::from_utf8_lossy(bytes).into_owned()
}

/// <https://encoding.spec.whatwg.org/#concept-encoding-get>
pub(crate) fn get_an_encoding(label: &str) -> Option<&'static str> {
    // Step 1: Remove any leading and trailing ASCII whitespace from label.
    let label = label
        .trim_matches(|character: char| matches!(character, '\t' | '\n' | '\u{C}' | '\r' | ' '));

    // Step 2: If label is an ASCII case-insensitive match for any of the
    // labels listed in the table below, then return the corresponding
    // encoding; otherwise return failure.
    // Note: UTF-8 is the only encoding implemented; every other label is
    // failure.
    const UTF_8_LABELS: [&str; 6] = [
        "unicode-1-1-utf-8",
        "unicode11utf8",
        "unicode20utf8",
        "utf-8",
        "utf8",
        "x-unicode20utf8",
    ];
    UTF_8_LABELS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(label))
        .then_some("UTF-8")
}

#[cfg(test)]
mod tests {
    use super::{get_an_encoding, utf_8_decode, utf_8_encode};

    #[test]
    fn utf_8_labels_are_matched_case_insensitively_and_trimmed() {
        assert_eq!(get_an_encoding("utf-8"), Some("UTF-8"));
        assert_eq!(get_an_encoding(" UTF8\t"), Some("UTF-8"));
        assert_eq!(get_an_encoding("unicode-1-1-utf-8"), Some("UTF-8"));
        assert_eq!(get_an_encoding("utf-16le"), None);
        assert_eq!(get_an_encoding("\u{a0}utf-8"), None);
    }

    #[test]
    fn utf_8_decode_strips_the_byte_order_mark_and_replaces_errors() {
        assert_eq!(utf_8_decode(&[0xEF, 0xBB, 0xBF, b'a']), "a");
        assert_eq!(utf_8_decode(&[0xEF, 0xBB]), "\u{FFFD}");
        assert_eq!(utf_8_decode(&[0xC0, 0xC1]), "\u{FFFD}\u{FFFD}");
        assert_eq!(utf_8_encode("a\u{30A}"), vec![0x61, 0xCC, 0x8A]);
    }
}
