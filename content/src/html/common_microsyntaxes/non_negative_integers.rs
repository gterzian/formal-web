use super::signed_integers::rules_for_parsing_integers;

/// <https://html.spec.whatwg.org/#rules-for-parsing-non-negative-integers>
pub(crate) fn rules_for_parsing_non_negative_integers(input: &str) -> Option<u64> {
    // Step 1: "Let input be the string being parsed."
    // Step 2: "Let value be the result of parsing input using the rules for parsing integers."
    let value = rules_for_parsing_integers(input)?;
    // Step 3: "If value is an error, return an error."
    // Step 4: "If value is less than zero, return an error."
    if value < 0 {
        return None;
    }
    // Step 5: "Return value."
    Some(value as u64)
}
