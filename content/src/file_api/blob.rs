use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::encoding::{utf_8_decode, utf_8_encode};
use crate::js::Types;
use crate::streams::{ReadableStream, readable_stream_set_up_with_byte_reading_support};
use crate::webidl::{create_array_buffer, create_uint8_array, resolved_promise};

type JsObject = <Types as JsTypes>::JsObject;

/// <https://w3c.github.io/FileAPI/#typedefdef-blobpart>
pub(crate) enum BlobPart {
    BufferSource(Vec<u8>),
    Blob(Blob),
    String(String),
}

/// <https://w3c.github.io/FileAPI/#enumdef-endingtype>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndingType {
    Transparent,
    Native,
}

impl EndingType {
    /// <https://webidl.spec.whatwg.org/#js-enumeration>
    pub(crate) fn from_idl(value: &str) -> Option<Self> {
        match value {
            "transparent" => Some(Self::Transparent),
            "native" => Some(Self::Native),
            _ => None,
        }
    }
}

/// <https://w3c.github.io/FileAPI/#dfn-BlobPropertyBag>
pub(crate) struct BlobPropertyBag {
    /// <https://w3c.github.io/FileAPI/#dfn-BPtype>
    pub(crate) type_: String,
    /// <https://w3c.github.io/FileAPI/#dfn-endings>
    pub(crate) endings: EndingType,
}

impl Default for BlobPropertyBag {
    fn default() -> Self {
        Self {
            type_: String::new(),
            endings: EndingType::Transparent,
        }
    }
}

/// <https://w3c.github.io/FileAPI/#dfn-Blob>
#[gc_struct]
pub(crate) struct Blob {
    /// The byte sequence the blob refers to.
    #[ignore_trace]
    bytes: Rc<[u8]>,

    /// <https://w3c.github.io/FileAPI/#dfn-type>
    #[ignore_trace]
    type_: String,
}

impl Blob {
    /// <https://w3c.github.io/FileAPI/#constructorBlob>
    pub(crate) fn constructor(blob_parts: Option<Vec<BlobPart>>, options: BlobPropertyBag) -> Self {
        // Step 1: If invoked with zero parameters, return a new Blob object
        // consisting of 0 bytes, with size set to 0, and with type set to the
        // empty string.
        let Some(blob_parts) = blob_parts else {
            return Self {
                bytes: Rc::from([]),
                type_: String::new(),
            };
        };

        // Step 2: Let bytes be the result of processing blob parts given
        // blobParts and options.
        let bytes = process_blob_parts(blob_parts, &options);

        // Step 3: If the type member of the options argument is not the empty
        // string, run the following sub-steps:
        let mut type_ = options.type_;
        if !type_.is_empty() {
            // Step 3.1: Let t be the type dictionary member. If t contains
            // any characters outside the range U+0020 to U+007E, then set t
            // to the empty string and return from these substeps.
            if type_
                .chars()
                .any(|character| !('\u{20}'..='\u{7E}').contains(&character))
            {
                type_ = String::new();
            } else {
                // Step 3.2: Convert every character in t to ASCII lowercase.
                type_ = type_.to_ascii_lowercase();
            }
        }

        // Step 4: Return a Blob object referring to bytes as its associated
        // byte sequence, with its size set to the length of bytes, and its
        // type set to the value of t from the substeps above.
        Self {
            bytes: Rc::from(bytes),
            type_,
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A Blob that represents bytes as its raw data, with the empty string as
    /// its type.
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Rc::from(bytes),
            type_: String::new(),
        }
    }

    /// <https://w3c.github.io/FileAPI/#dfn-size>
    pub(crate) fn size(&self) -> u64 {
        // Returns the size of the byte sequence in number of bytes.
        self.bytes.len() as u64
    }

    /// <https://w3c.github.io/FileAPI/#dfn-type>
    pub(crate) fn type_(&self) -> String {
        // The ASCII-encoded string in lower case representing the media type
        // of the Blob.
        self.type_.clone()
    }

    /// <https://w3c.github.io/FileAPI/#dfn-slice>
    pub(crate) fn slice(
        &self,
        start: Option<i64>,
        end: Option<i64>,
        content_type: Option<String>,
    ) -> Blob {
        // Step 1: Let sliceStart, sliceEnd, and sliceContentType be null.
        // Step 2: If start is given, set sliceStart to start.
        // Step 3: If end is given, set sliceEnd to end.
        // Step 4: If contentType is given, set sliceContentType to
        // contentType.
        // Step 5: Return the result of slice blob given this, sliceStart,
        // sliceEnd, and sliceContentType.
        slice_blob(self, start, end, content_type)
    }

    /// <https://w3c.github.io/FileAPI/#dfn-stream>
    pub(crate) fn stream(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(ReadableStream, JsObject), Types> {
        // The stream() method, when invoked, must return the result of
        // calling get stream on this.
        self.get_stream(ec)
    }

    /// <https://w3c.github.io/FileAPI/#blob-get-stream>
    fn get_stream(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(ReadableStream, JsObject), Types> {
        // Step 1: Let stream be a new ReadableStream created in blob's
        // relevant Realm.
        // Step 2: Set up stream with byte reading support.
        let (stream, stream_object, controller) =
            readable_stream_set_up_with_byte_reading_support(ec)?;

        // Step 3: Run the following steps in parallel:
        // Step 3.1: While not all bytes of blob have been read:
        // Step 3.1.1: Let bytes be the byte sequence that results from
        // reading a chunk from blob, or failure if a chunk cannot be read.
        // Step 3.1.2: Queue a global task on the file reading task source
        // given blob's relevant global object to perform the following
        // steps:
        // Step 3.1.2.1: If bytes is failure, then error stream with a failure
        // reason and abort these steps.
        // Step 3.1.2.2: Let chunk be a new Uint8Array wrapping an ArrayBuffer
        // containing bytes. If creating the ArrayBuffer throws an exception,
        // then error stream with that exception and abort these steps.
        // Step 3.1.2.3: Enqueue chunk in stream.
        // Step 3.2: Once all bytes have been read, close stream.
        // Note: The bytes are in memory: the one chunk is enqueued and the
        // stream closed here, before the stream is returned, in place of the
        // parallel steps and the tasks they queue.
        if !self.bytes.is_empty() {
            let chunk = create_uint8_array(&self.bytes, ec)?;
            controller.enqueue(Types::value_from_object(chunk), ec)?;
        }
        controller.close(ec)?;

        // Step 4: Return stream.
        Ok((stream, stream_object))
    }

    /// <https://w3c.github.io/FileAPI/#dfn-text>
    pub(crate) fn text(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
        // Step 1: Let stream be the result of calling get stream on this.
        // Step 2: Let reader be the result of getting a reader from stream.
        // If that threw an exception, return a new promise rejected with that
        // exception.
        // Step 3: Let promise be the result of reading all bytes from stream
        // with reader.
        // Step 4: Return the result of transforming promise by a fulfillment
        // handler that returns the result of running UTF-8 decode on its
        // first argument.
        // Note: The bytes are in memory: the promise is resolved with the
        // decoded bytes, without the stream read.
        let text = utf_8_decode(&self.bytes);
        let text = ec.js_string_from_str(&text);
        let text = ec.value_from_string(text);
        resolved_promise(text, ec)
    }

    /// <https://w3c.github.io/FileAPI/#dfn-arrayBuffer>
    pub(crate) fn array_buffer(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let stream be the result of calling get stream on this.
        // Step 2: Let reader be the result of getting a reader from stream.
        // If that threw an exception, return a new promise rejected with that
        // exception.
        // Step 3: Let promise be the result of reading all bytes from stream
        // with reader.
        // Step 4: Return the result of transforming promise by a fulfillment
        // handler that returns a new ArrayBuffer whose contents are its first
        // argument.
        // Note: The bytes are in memory: the promise is resolved with the
        // ArrayBuffer, without the stream read.
        let buffer = create_array_buffer(&self.bytes, ec)?;
        resolved_promise(Types::value_from_object(buffer), ec)
    }

    /// <https://w3c.github.io/FileAPI/#dfn-bytes>
    pub(crate) fn bytes_method(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let stream be the result of calling get stream on this.
        // Step 2: Let reader be the result of getting a reader from stream.
        // If that threw an exception, return a new promise rejected with that
        // exception.
        // Step 3: Let promise be the result of reading all bytes from stream
        // with reader.
        // Step 4: Return the result of transforming promise by a fulfillment
        // handler that returns a new Uint8Array wrapping an ArrayBuffer
        // containing its first argument.
        // Note: The bytes are in memory: the promise is resolved with the
        // Uint8Array, without the stream read.
        let array = create_uint8_array(&self.bytes, ec)?;
        resolved_promise(Types::value_from_object(array), ec)
    }
}

/// <https://w3c.github.io/FileAPI/#process-blob-parts>
fn process_blob_parts(parts: Vec<BlobPart>, options: &BlobPropertyBag) -> Vec<u8> {
    // Step 1: Let bytes be an empty sequence of bytes.
    let mut bytes = Vec::new();

    // Step 2: For each element in parts:
    for element in parts {
        match element {
            // Step 2.1: If element is a USVString, run the following
            // substeps:
            BlobPart::String(element) => {
                // Step 2.1.1: Let s be element.
                let mut string = element;

                // Step 2.1.2: If the endings member of options is "native",
                // set s to the result of converting line endings to native of
                // element.
                if options.endings == EndingType::Native {
                    string = convert_line_endings_to_native(&string);
                }

                // Step 2.1.3: Append the result of UTF-8 encoding s to bytes.
                bytes.extend(utf_8_encode(&string));
            }

            // Step 2.2: If element is a BufferSource, get a copy of the bytes
            // held by the buffer source, and append those bytes to bytes.
            BlobPart::BufferSource(copy) => bytes.extend(copy),

            // Step 2.3: If element is a Blob, append the bytes it represents
            // to bytes.
            BlobPart::Blob(blob) => bytes.extend_from_slice(blob.bytes()),
        }
    }

    // Step 3: Return bytes.
    bytes
}

/// <https://w3c.github.io/FileAPI/#convert-line-endings-to-native>
fn convert_line_endings_to_native(string: &str) -> String {
    // Step 1: Let native line ending be be the code point U+000A LF.
    // Step 2: If the underlying platform's conventions are to represent
    // newlines as a carriage return and line feed sequence, set native line
    // ending to the code point U+000D CR followed by the code point U+000A
    // LF.
    let native_line_ending = if cfg!(windows) { "\r\n" } else { "\n" };

    // Step 3: Set result to the empty string.
    let mut result = String::with_capacity(string.len());

    // Step 4: Let position be a position variable for s, initially pointing
    // at the start of s.
    let mut rest = string;

    // Step 5: Let token be the result of collecting a sequence of code points
    // that are not equal to U+000A LF or U+000D CR from s given position.
    let token_end = rest.find(['\n', '\r']).unwrap_or(rest.len());

    // Step 6: Append token to result.
    result.push_str(&rest[..token_end]);
    rest = &rest[token_end..];

    // Step 7: While position is not past the end of s:
    while !rest.is_empty() {
        // Step 7.1: If the code point at position within s equals U+000D CR:
        if let Some(after_cr) = rest.strip_prefix('\r') {
            // Step 7.1.1: Append native line ending to result.
            result.push_str(native_line_ending);

            // Step 7.1.2: Advance position by 1.
            rest = after_cr;

            // Step 7.1.3: If position is not past the end of s and the code
            // point at position within s equals U+000A LF advance position by
            // 1.
            if let Some(after_lf) = rest.strip_prefix('\n') {
                rest = after_lf;
            }
        } else if let Some(after_lf) = rest.strip_prefix('\n') {
            // Step 7.2: Otherwise if the code point at position within s
            // equals U+000A LF, advance position by 1 and append native line
            // ending to result.
            rest = after_lf;
            result.push_str(native_line_ending);
        }

        // Step 7.3: Let token be the result of collecting a sequence of code
        // points that are not equal to U+000A LF or U+000D CR from s given
        // position.
        let token_end = rest.find(['\n', '\r']).unwrap_or(rest.len());

        // Step 7.4: Append token to result.
        result.push_str(&rest[..token_end]);
        rest = &rest[token_end..];
    }

    // Step 8: Return result.
    result
}

/// <https://w3c.github.io/FileAPI/#slice-blob>
fn slice_blob(
    blob: &Blob,
    start: Option<i64>,
    end: Option<i64>,
    content_type: Option<String>,
) -> Blob {
    // Step 1: Let originalSize be blob's size.
    let original_size = blob.size() as i64;

    // Step 2: The start parameter, if non-null, is a value for the start
    // point of a slice blob call, and must be treated as a byte-order
    // position, with the zeroth position representing the first byte. User
    // agents must normalize start according to the following:
    let relative_start = match start {
        // Step 2.1: If start is null, let relativeStart be 0.
        None => 0,
        // Step 2.2: If start is negative, let relativeStart be
        // max((originalSize + start), 0).
        Some(start) if start < 0 => (original_size + start).max(0),
        // Step 2.3: Otherwise, let relativeStart be min(start, originalSize).
        Some(start) => start.min(original_size),
    };

    // Step 3: The end parameter, if non-null. User agents must normalize end
    // according to the following:
    let relative_end = match end {
        // Step 3.1: If end is null, let relativeEnd be originalSize.
        None => original_size,
        // Step 3.2: If end is negative, let relativeEnd be
        // max((originalSize + end), 0).
        Some(end) if end < 0 => (original_size + end).max(0),
        // Step 3.3: Otherwise, let relativeEnd be min(end, originalSize).
        Some(end) => end.min(original_size),
    };

    // Step 4: The contentType parameter, if non-null, is used to set the
    // ASCII-encoded string in lower case representing the media type of the
    // Blob. User agents must normalize contentType according to the
    // following:
    let relative_content_type = match content_type {
        // Step 4.1: If contentType is null, let relativeContentType be set to
        // the empty string.
        None => String::new(),
        // Step 4.2: Otherwise, let relativeContentType be set to contentType
        // and run the substeps in § 3.1 Constructors, which may set
        // relativeContentType to the empty string.
        Some(content_type) => {
            if content_type
                .chars()
                .any(|character| !('\u{20}'..='\u{7E}').contains(&character))
            {
                String::new()
            } else {
                content_type.to_ascii_lowercase()
            }
        }
    };

    // Step 5: Let span be max((relativeEnd - relativeStart), 0).
    let span = (relative_end - relative_start).max(0) as usize;

    // Step 6: Return a new Blob object S with the following characteristics:
    // Step 6.1: S refers to span consecutive bytes from blob's associated
    // byte sequence, beginning with the byte at byte-order position
    // relativeStart.
    // Step 6.2: S.size = span.
    // Step 6.3: S.type = relativeContentType.
    let start = relative_start as usize;
    Blob {
        bytes: Rc::from(&blob.bytes[start..start + span]),
        type_: relative_content_type,
    }
}

#[cfg(test)]
mod tests {
    use super::{Blob, BlobPart, BlobPropertyBag, EndingType, convert_line_endings_to_native};

    fn text_blob(text: &str, type_: &str) -> Blob {
        Blob::constructor(
            Some(vec![BlobPart::String(String::from(text))]),
            BlobPropertyBag {
                type_: String::from(type_),
                endings: EndingType::Transparent,
            },
        )
    }

    #[test]
    fn line_endings_convert_to_the_platform_ending() {
        let native = if cfg!(windows) { "\r\n" } else { "\n" };
        assert_eq!(
            convert_line_endings_to_native("a\r\nb\rc\nd"),
            format!("a{native}b{native}c{native}d")
        );
        assert_eq!(convert_line_endings_to_native("plain"), "plain");
        assert_eq!(convert_line_endings_to_native("\r"), native);
    }

    #[test]
    fn the_type_is_lowercased_or_dropped() {
        assert_eq!(text_blob("x", "Text/Plain").type_(), "text/plain");
        assert_eq!(text_blob("x", "a\u{e9}").type_(), "");
        assert_eq!(
            Blob::constructor(None, BlobPropertyBag::default()).size(),
            0
        );
    }

    #[test]
    fn slices_are_normalized_to_the_blob_bounds() {
        let blob = text_blob("abcd", "");
        assert_eq!(blob.slice(Some(1), Some(-1), None).bytes(), b"bc");
        assert_eq!(blob.slice(Some(-1), None, None).bytes(), b"d");
        assert_eq!(blob.slice(Some(10), Some(2), None).size(), 0);
        assert_eq!(
            blob.slice(None, None, Some(String::from("X/Y"))).type_(),
            "x/y"
        );
    }
}
