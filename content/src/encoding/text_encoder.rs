use js_engine::gc_struct;

use super::utf_8_encode;

/// <https://encoding.spec.whatwg.org/#textencoder>
#[gc_struct]
pub(crate) struct TextEncoder {}

/// <https://encoding.spec.whatwg.org/#dictdef-textencoderencodeintoresult>
pub(crate) struct TextEncoderEncodeIntoResult {
    /// <https://encoding.spec.whatwg.org/#dom-textencoderencodeintoresult-read>
    pub(crate) read: u64,
    /// <https://encoding.spec.whatwg.org/#dom-textencoderencodeintoresult-written>
    pub(crate) written: Vec<u8>,
}

impl TextEncoder {
    /// <https://encoding.spec.whatwg.org/#dom-textencoder>
    pub(crate) fn constructor() -> Self {
        // The new TextEncoder() constructor steps are to do nothing.
        Self {}
    }

    /// <https://encoding.spec.whatwg.org/#dom-textencoder-encoding>
    pub(crate) fn encoding(&self) -> String {
        // The encoding getter steps are to return this's encoding's name,
        // ASCII lowercased.
        String::from("utf-8")
    }

    /// <https://encoding.spec.whatwg.org/#dom-textencoder-encode>
    pub(crate) fn encode(&self, input: String) -> Vec<u8> {
        // Step 1: Convert input to an I/O queue of scalar values.
        // Step 2: Let output be the I/O queue of bytes « end-of-queue ».
        // Step 3: While true:
        // Step 3.1: Let item be the result of reading from input.
        // Step 3.2: Let result be the result of processing an item with item,
        // an instance of the UTF-8 encoder, input, output, and "fatal".
        // Step 3.3: Assert: result is not an error.
        // Step 3.4: If result is finished, then convert output into a byte
        // sequence and return a Uint8Array object wrapping an ArrayBuffer
        // containing output.
        // Note: The Uint8Array is the binding layer's.
        utf_8_encode(&input)
    }

    /// <https://encoding.spec.whatwg.org/#dom-textencoder-encodeinto>
    pub(crate) fn encode_into(
        &self,
        source: String,
        destination_byte_length: usize,
    ) -> TextEncoderEncodeIntoResult {
        // Step 1: Let read be 0.
        let mut read = 0;

        // Step 2: Let written be 0.
        let mut written = Vec::new();

        // Step 3: Let encoder be an instance of the UTF-8 encoder.
        // Step 4: Let unused be the I/O queue of scalar values
        // « end-of-queue ».
        // Step 5: Convert source to an I/O queue of scalar values.
        // Step 6: While true:
        for item in source.chars() {
            // Step 6.1: Let item be the result of reading from source.
            // Step 6.2: Let result be the result of running the UTF-8
            // encoder's handler on unused and item.
            let mut result = [0; 4];
            let result = item.encode_utf8(&mut result);

            // Step 6.3: If result is finished, then break.
            // Step 6.4: Otherwise:
            // Step 6.4.1: If destination's byte length − written is greater
            // than or equal to the number of bytes in result, then:
            if destination_byte_length - written.len() >= result.len() {
                // Step 6.4.1.1: If item is greater than U+FFFF, then
                // increment read by 2.
                // Step 6.4.1.2: Otherwise, increment read by 1.
                read += item.len_utf16() as u64;

                // Step 6.4.1.3: Write the bytes in result into destination,
                // with startingOffset set to written.
                // Note: The binding layer writes the bytes into the
                // destination view.
                // Step 6.4.1.4: Increment written by the number of bytes in
                // result.
                written.extend_from_slice(result.as_bytes());
            } else {
                // Step 6.4.2: Otherwise, break.
                break;
            }
        }

        // Step 7: Return «[ "read" → read, "written" → written ]».
        TextEncoderEncodeIntoResult { read, written }
    }
}
