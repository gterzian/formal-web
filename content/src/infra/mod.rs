/// <https://infra.spec.whatwg.org/#strip-and-collapse-ascii-whitespace>
pub(crate) fn strip_and_collapse_ascii_whitespace(value: &str) -> String {
    // Step 1: "Replace any sequence of one or more consecutive code points that are ASCII whitespace in the string with a single U+0020 SPACE code point, and then remove any leading and trailing ASCII whitespace from that string."
    let mut normalized = String::with_capacity(value.len());
    let mut pending_space = false;

    for character in value.chars() {
        if matches!(
            character,
            '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' | ' '
        ) {
            pending_space = !normalized.is_empty();
            continue;
        }

        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }

        normalized.push(character);
    }

    normalized
}
