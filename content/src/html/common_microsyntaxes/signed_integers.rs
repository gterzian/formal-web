/// <https://html.spec.whatwg.org/#rules-for-parsing-integers>
pub(crate) fn rules_for_parsing_integers(input: &str) -> Option<i64> {
    // Step 1: "Let input be the string being parsed."
    // Step 2: "Let position be a pointer into input, initially pointing at the start of the string."
    // Step 3: "Let sign have the value \"positive\"."
    let mut position = input.chars().peekable();
    let mut negative = false;
    // Step 4: "Skip ASCII whitespace within input given position."
    while matches!(position.peek(), Some(' ' | '\t' | '\n' | '\u{0C}' | '\r')) {
        position.next();
    }
    // Step 5: "If position is past the end of input, return an error."
    position.peek()?;
    // Step 6: "If the character indicated by position (the first character) is a U+002D HYPHEN-MINUS character (-):"
    if position.peek() == Some(&'-') {
        // Step 6.1: "Let sign be \"negative\"."
        negative = true;
        // Step 6.2: "Advance position to the next character."
        position.next();
        // Step 6.3: "If position is past the end of input, return an error."
        position.peek()?;
    } else if position.peek() == Some(&'+') {
        // Step 6.4: "Advance position to the next character. (The \"+\" is ignored, but it is not conforming.)"
        position.next();
        // Step 6.5: "If position is past the end of input, return an error."
        position.peek()?;
    }
    // Step 7: "If the character indicated by position is not an ASCII digit, then return an error."
    if !position.peek().is_some_and(char::is_ascii_digit) {
        return None;
    }
    // Step 8: "Collect a sequence of code points that are ASCII digits from input given position, and interpret the resulting sequence as a base-ten integer. Let value be that integer."
    // Note: the collection saturates at the bounds of i64 rather than growing without limit.
    let mut value = 0i64;
    while let Some(character) = position.peek() {
        let Some(digit) = character.to_digit(10) else {
            break;
        };
        value = value.saturating_mul(10).saturating_add(i64::from(digit));
        position.next();
    }
    // Step 9: "If sign is \"positive\", return value, otherwise return the result of subtracting value from zero."
    if negative {
        Some(value.saturating_neg())
    } else {
        Some(value)
    }
}
