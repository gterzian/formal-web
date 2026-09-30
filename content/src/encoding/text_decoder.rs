use std::cell::RefCell;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, gc_struct};

use crate::js::Types;

use super::get_an_encoding;

/// <https://encoding.spec.whatwg.org/#dictdef-textdecoderoptions>
#[derive(Default)]
pub(crate) struct TextDecoderOptions {
    /// <https://encoding.spec.whatwg.org/#dom-textdecoderoptions-fatal>
    pub(crate) fatal: bool,
    /// <https://encoding.spec.whatwg.org/#dom-textdecoderoptions-ignorebom>
    pub(crate) ignore_bom: bool,
}

/// The decoder's state across streaming decode() calls.
#[derive(Default)]
struct DecodeState {
    /// <https://encoding.spec.whatwg.org/#textdecoder-do-not-flush-flag>
    do_not_flush: bool,
    /// <https://encoding.spec.whatwg.org/#textdecodercommon-i/o-queue>
    io_queue: Vec<u8>,
    /// <https://encoding.spec.whatwg.org/#textdecodercommon-bom-seen-flag>
    bom_seen: bool,
}

/// <https://encoding.spec.whatwg.org/#textdecoder>
#[gc_struct]
pub(crate) struct TextDecoder {
    /// <https://encoding.spec.whatwg.org/#textdecodercommon-encoding>
    #[ignore_trace]
    encoding: &'static str,

    /// <https://encoding.spec.whatwg.org/#textdecodercommon-error-mode>
    #[ignore_trace]
    fatal: bool,

    /// <https://encoding.spec.whatwg.org/#textdecodercommon-ignore-bom-flag>
    #[ignore_trace]
    ignore_bom: bool,

    #[ignore_trace]
    state: Rc<RefCell<DecodeState>>,
}

impl TextDecoder {
    /// <https://encoding.spec.whatwg.org/#dom-textdecoder>
    pub(crate) fn constructor(
        label: String,
        options: TextDecoderOptions,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let encoding be the result of getting an encoding from
        // label.
        let encoding = get_an_encoding(&label);

        // Step 2: If encoding is failure or replacement, then throw a
        // RangeError.
        let Some(encoding) = encoding else {
            return Err(ec.new_range_error(&format!("'{label}' is not a supported encoding label")));
        };

        // Step 3: Set this's encoding to encoding.
        // Step 4: If options["fatal"] is true, then set this's error mode to
        // "fatal".
        // Step 5: Set this's ignore BOM to options["ignoreBOM"].
        Ok(Self {
            encoding,
            fatal: options.fatal,
            ignore_bom: options.ignore_bom,
            state: Rc::new(RefCell::new(DecodeState::default())),
        })
    }

    /// <https://encoding.spec.whatwg.org/#dom-textdecoder-encoding>
    pub(crate) fn encoding(&self) -> String {
        // The encoding getter steps are to return this's encoding's name,
        // ASCII lowercased.
        self.encoding.to_ascii_lowercase()
    }

    /// <https://encoding.spec.whatwg.org/#dom-textdecoder-fatal>
    pub(crate) fn fatal(&self) -> bool {
        // The fatal getter steps are to return true if this's error mode is
        // "fatal", otherwise false.
        self.fatal
    }

    /// <https://encoding.spec.whatwg.org/#dom-textdecoder-ignorebom>
    pub(crate) fn ignore_bom(&self) -> bool {
        // The ignoreBOM getter steps are to return this's ignore BOM.
        self.ignore_bom
    }

    /// <https://encoding.spec.whatwg.org/#dom-textdecoder-decode>
    pub(crate) fn decode(
        &self,
        input: Option<Vec<u8>>,
        stream: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<String, Types> {
        let mut state = self.state.borrow_mut();

        // Step 1: If this's do not flush is false, then set this's decoder to
        // a new instance of this's encoding's decoder, this's I/O queue to
        // the I/O queue of bytes « end-of-queue », and this's BOM seen to
        // false.
        if !state.do_not_flush {
            state.io_queue.clear();
            state.bom_seen = false;
        }

        // Step 2: Set this's do not flush to options["stream"].
        state.do_not_flush = stream;

        // Step 3: If input is given, then push a copy of input to this's I/O
        // queue.
        if let Some(input) = input {
            state.io_queue.extend(input);
        }

        // Step 4: Let output be the I/O queue of scalar values
        // « end-of-queue ».
        let mut output = String::new();

        // Step 5: While true:
        // Step 5.1: Let item be the result of reading from this's I/O queue.
        // Step 5.2: If item is end-of-queue and this's do not flush is true,
        // then return the result of running serialize I/O queue with this
        // and output.
        // Step 5.3: Otherwise:
        // Step 5.3.1: Let result be the result of processing an item with
        // item, this's decoder, this's I/O queue, output, and this's error
        // mode.
        // Step 5.3.2: If result is finished, then return the result of
        // running serialize I/O queue with this and output.
        // Step 5.3.3: Otherwise, if result is error, throw a TypeError.
        // Note: The UTF-8 decoder runs over the whole queue at once: each
        // maximal subpart of an ill-formed sequence is one error.
        let mut bytes = std::mem::take(&mut state.io_queue);
        let mut position = 0;
        loop {
            match std::str::from_utf8(&bytes[position..]) {
                Ok(valid) => {
                    output.push_str(valid);
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    output.push_str(
                        std::str::from_utf8(&bytes[position..position + valid_up_to])
                            .unwrap_or_default(),
                    );
                    position += valid_up_to;
                    match error.error_len() {
                        // An incomplete sequence at the end of the queue.
                        None if state.do_not_flush => {
                            bytes.drain(..position);
                            state.io_queue = bytes;
                            return Ok(self.serialize_io_queue(&mut state, output));
                        }
                        None if self.fatal => {
                            return Err(ec.new_type_error("The encoded data was not valid UTF-8"));
                        }
                        None => {
                            output.push('\u{FFFD}');
                            break;
                        }
                        Some(_) if self.fatal => {
                            return Err(ec.new_type_error("The encoded data was not valid UTF-8"));
                        }
                        Some(error_len) => {
                            output.push('\u{FFFD}');
                            position += error_len;
                        }
                    }
                }
            }
        }
        Ok(self.serialize_io_queue(&mut state, output))
    }

    /// <https://encoding.spec.whatwg.org/#concept-td-serialize>
    fn serialize_io_queue(&self, state: &mut DecodeState, io_queue: String) -> String {
        // Step 1: Let output be the empty string.
        // Step 2: While true:
        // Step 2.1: Let item be the result of reading from ioQueue.
        // Step 2.2: If item is end-of-queue, then return output.
        // Step 2.3: If encoding is UTF-8, UTF-16BE, or UTF-16LE, and ignore
        // BOM and BOM seen are false, then:
        // Step 2.3.1: Set BOM seen to true.
        // Step 2.3.2: If item is U+FEFF, then continue.
        // Step 2.4: Append item to output.
        if io_queue.is_empty() || self.ignore_bom || state.bom_seen {
            return io_queue;
        }
        state.bom_seen = true;
        match io_queue.strip_prefix('\u{FEFF}') {
            Some(rest) => rest.to_owned(),
            None => io_queue,
        }
    }
}
