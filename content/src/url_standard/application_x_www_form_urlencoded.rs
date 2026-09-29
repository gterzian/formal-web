use form_urlencoded::{Serializer, parse};

/// <https://url.spec.whatwg.org/#concept-urlencoded-parser>
pub(crate) fn application_x_www_form_urlencoded_parser(input: &[u8]) -> Vec<(String, String)> {
    // Step 1: Let sequences be the result of splitting input on 0x26 (&).
    // Step 2: Let output be an initially empty list of name-value tuples
    // where both name and value hold a string.
    // Step 3: For each byte sequence bytes in sequences:
    // Step 3.1: If bytes is the empty byte sequence, then continue.
    // Step 3.2: If bytes contains a 0x3D (=), then let name be the bytes
    // from the start of bytes up to but excluding its first 0x3D (=), and
    // let value be the bytes, if any, after the first 0x3D (=) up to the
    // end of bytes. If 0x3D (=) is the first byte, then name will be the
    // empty byte sequence. If it is the last, then value will be the empty
    // byte sequence.
    // Step 3.3: Otherwise, let name have the value of bytes and let value
    // be the empty byte sequence.
    // Step 3.4: Replace any 0x2B (+) in name and value with 0x20 (SP).
    // Step 3.5: Let nameString and valueString be the result of running
    // UTF-8 decode without BOM on the percent-decoding of name and value,
    // respectively.
    // Step 3.6: Append (nameString, valueString) to output.
    // Step 4: Return output.
    parse(input)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// <https://url.spec.whatwg.org/#concept-urlencoded-serializer>
pub(crate) fn application_x_www_form_urlencoded_serializer(tuples: &[(String, String)]) -> String {
    // Step 1: Set encoding to the result of getting an output encoding from
    // encoding.
    // Step 2: Let output be the empty string.
    let mut serializer = Serializer::new(String::new());

    // Step 3: For each tuple of tuples:
    for (name, value) in tuples {
        // Step 3.1: Assert: tuple's name and tuple's value are scalar value
        // strings.
        // Step 3.2: Let name be the result of running percent-encode after
        // encoding with encoding, tuple's name, the
        // application/x-www-form-urlencoded percent-encode set, and true.
        // Step 3.3: Let value be the result of running percent-encode after
        // encoding with encoding, tuple's value, the
        // application/x-www-form-urlencoded percent-encode set, and true.
        // Step 3.4: If output is not the empty string, then append U+0026
        // (&) to output.
        // Step 3.5: Append name, followed by U+003D (=), followed by value,
        // to output.
        serializer.append_pair(name, value);
    }

    // Step 4: Return output.
    serializer.finish()
}

#[cfg(test)]
mod tests {
    use super::{
        application_x_www_form_urlencoded_parser as parse,
        application_x_www_form_urlencoded_serializer as serialize,
    };

    #[test]
    fn pairs_round_trip_through_the_parser_and_serializer() {
        let pairs = parse(b"a=1&b=&c&=d&e=a+b%20c");
        assert_eq!(
            pairs,
            vec![
                (String::from("a"), String::from("1")),
                (String::from("b"), String::new()),
                (String::from("c"), String::new()),
                (String::new(), String::from("d")),
                (String::from("e"), String::from("a b c")),
            ]
        );
        assert_eq!(serialize(&pairs), "a=1&b=&c=&=d&e=a+b+c");
        assert_eq!(serialize(&[]), "");
    }
}
