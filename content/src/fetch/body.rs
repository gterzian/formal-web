use std::cell::RefCell;
use std::rc::Rc;

use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::encoding::{utf_8_decode, utf_8_encode};
use crate::file_api::Blob;
use crate::infra::parse_json_bytes_to_a_javascript_value;
use crate::js::Types;
use crate::streams::{
    ReadableStream, acquire_readable_stream_default_reader,
    readable_stream_set_up_with_byte_reading_support,
};
use crate::url_standard::URLSearchParams;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{create_array_buffer, create_uint8_array, rejected_promise, resolved_promise};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://fetch.spec.whatwg.org/#concept-body>
#[derive(Debug, Clone)]
pub(crate) struct Body {
    /// <https://fetch.spec.whatwg.org/#concept-body-source>
    pub(crate) source: Vec<u8>,
    /// <https://fetch.spec.whatwg.org/#concept-body-disturbed>
    pub(crate) disturbed: bool,
}

/// <https://fetch.spec.whatwg.org/#body-with-type>
pub(crate) struct BodyWithType {
    /// <https://fetch.spec.whatwg.org/#body-with-type-body>
    pub(crate) body: Body,
    /// <https://fetch.spec.whatwg.org/#body-with-type-type>
    pub(crate) type_: Option<String>,
}

/// <https://fetch.spec.whatwg.org/#typedefdef-bodyinit>
pub(crate) enum BodyInit {
    Blob(Blob),
    BufferSource(Vec<u8>),
    URLSearchParams(URLSearchParams),
    String(String),
}

/// <https://fetch.spec.whatwg.org/#concept-bodyinit-extract>
pub(crate) fn extract(object: BodyInit) -> BodyWithType {
    // Step 1: Let stream be null.
    // Step 2: If object is a ReadableStream object, then set stream to
    // object.
    // Step 3: Otherwise, if object is a Blob object, set stream to the result
    // of running object's get stream.
    // Step 4: Otherwise, set stream to a new ReadableStream object, and set
    // up stream with byte reading support.
    // Note: The stream is created when the body is read; the source bytes
    // are what the body carries.
    // Step 5: Assert: stream is a ReadableStream object.
    // Step 6: Let action be null.
    // Step 7: Let source be null.
    // Step 8: Let length be null.
    // Step 9: Let type be null.
    // Step 10: Switch on object:
    let (source, type_) = match object {
        // Blob: Set source to object. Set length to object's size. If
        // object's type attribute is not the empty byte sequence, set type to
        // its value.
        BodyInit::Blob(blob) => {
            let type_ = blob.type_();
            (blob.bytes().to_vec(), (!type_.is_empty()).then_some(type_))
        }
        // byte sequence: Set source to object.
        // BufferSource: Set source to a copy of the bytes held by object.
        BodyInit::BufferSource(bytes) => (bytes, None),
        // FormData: ... (not implemented)
        // URLSearchParams: Set source to the result of running the
        // application/x-www-form-urlencoded serializer with object's list.
        // Set type to `application/x-www-form-urlencoded;charset=UTF-8`.
        BodyInit::URLSearchParams(params) => (
            utf_8_encode(&params.stringification_behavior()),
            Some(String::from(
                "application/x-www-form-urlencoded;charset=UTF-8",
            )),
        ),
        // scalar value string: Set source to the UTF-8 encoding of object.
        // Set type to `text/plain;charset=UTF-8`.
        BodyInit::String(string) => (
            utf_8_encode(&string),
            Some(String::from("text/plain;charset=UTF-8")),
        ),
        // ReadableStream: ... (not implemented)
    };

    // Step 11: If source is a byte sequence, then set action to a step that
    // returns source and length to source's length.
    // Step 12: If action is non-null, then run these steps in parallel: ...
    // Step 13: Let body be a body whose stream is stream, source is source,
    // and length is length.
    // Step 14: Return (body, type).
    BodyWithType {
        body: Body {
            source,
            disturbed: false,
        },
        type_,
    }
}

/// The Body mixin's state: the object's body and the stream the `body`
/// attribute hands out.
#[gc_struct]
pub(crate) struct BodyMixin {
    #[ignore_trace]
    body: Rc<RefCell<Option<Body>>>,

    /// <https://fetch.spec.whatwg.org/#concept-body-stream>
    stream: GcCell<Option<ReadableStream>>,
    stream_object: GcCell<Option<JsObject>>,
}

/// <https://fetch.spec.whatwg.org/#body-mixin>
impl BodyMixin {
    pub(crate) fn new(body: Option<Body>, ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            body: Rc::new(RefCell::new(body)),
            stream: gc_cell_new(None, ec),
            stream_object: gc_cell_new(None, ec),
        }
    }

    pub(crate) fn set_body(&self, body: Body) {
        *self.body.borrow_mut() = Some(body);
    }

    /// The body as it is: its source and whether it is disturbed.
    pub(crate) fn body_state(&self) -> Option<Body> {
        self.body.borrow().clone()
    }

    /// <https://fetch.spec.whatwg.org/#concept-body-disturbed>
    pub(crate) fn mark_disturbed(&self, ec: &mut dyn ExecutionContext<Types>) {
        if let Some(body) = self.body.borrow_mut().as_mut() {
            body.disturbed = true;
        }
        // A stream handed out through the body attribute is read by the
        // consumer: it is disturbed and locked.
        if let Some(stream) = self.stream.borrow(ec).clone() {
            stream.set_disturbed(true);
            if !stream.is_readable_stream_locked(ec)
                && let Err(error) = acquire_readable_stream_default_reader(stream, ec)
            {
                log::error!("failed to lock a consumed body stream: {}", error.display());
            }
        }
    }

    /// <https://fetch.spec.whatwg.org/#concept-body-clone>
    pub(crate) fn clone_body(&self) -> Option<Body> {
        // Step 1: Let « out1, out2 » be the result of teeing body's stream.
        // Step 2: Set body's stream to out1.
        // Step 3: Return a body whose stream is out2 and other members are
        // copied from body.
        // Note: The source bytes are copied; each body creates its own stream
        // from them.
        self.body.borrow().as_ref().map(|body| Body {
            source: body.source.clone(),
            disturbed: false,
        })
    }

    pub(crate) fn body_bytes(&self) -> Option<Vec<u8>> {
        self.body.borrow().as_ref().map(|body| body.source.clone())
    }

    /// <https://fetch.spec.whatwg.org/#body-unusable>
    pub(crate) fn unusable(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        // An object including the Body interface mixin is said to be unusable
        // if its body is non-null and its body's stream is disturbed or
        // locked.
        let Some(body_disturbed) = self.body.borrow().as_ref().map(|body| body.disturbed) else {
            return false;
        };
        match self.stream.borrow(ec).clone() {
            Some(stream) => stream.disturbed() || stream.is_readable_stream_locked(ec),
            None => body_disturbed,
        }
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-body>
    pub(crate) fn stream(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<JsObject>, Types> {
        // The body getter steps are to return null if this's body is null;
        // otherwise this's body's stream.
        // Note: The stream is created on first access from the body's source
        // and marks the body disturbed, so the source is read once.
        if let Some(stream_object) = self.stream_object.borrow(ec).clone() {
            return Ok(Some(stream_object));
        }
        let Some(source) = self.body.borrow().as_ref().map(|body| body.source.clone()) else {
            return Ok(None);
        };
        let (stream, stream_object, controller) =
            readable_stream_set_up_with_byte_reading_support(ec)?;
        if !source.is_empty() {
            let chunk = create_uint8_array(&source, ec)?;
            controller.enqueue(Types::value_from_object(chunk), ec)?;
        }
        controller.close(ec)?;
        *self.stream.borrow_mut(ec) = Some(stream);
        *self.stream_object.borrow_mut(ec) = Some(stream_object.clone());
        Ok(Some(stream_object))
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-bodyused>
    pub(crate) fn body_used(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        // The bodyUsed getter steps are to return true if this's body is
        // non-null and this's body's stream is disturbed; otherwise false.
        let Some(body_disturbed) = self.body.borrow().as_ref().map(|body| body.disturbed) else {
            return false;
        };
        match self.stream.borrow(ec).clone() {
            Some(stream) => stream.disturbed(),
            None => body_disturbed,
        }
    }

    /// <https://fetch.spec.whatwg.org/#concept-body-consume-body>
    fn consume_body(
        &self,
        convert_bytes_to_js_value: impl FnOnce(
            Vec<u8>,
            &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: If object is unusable, then return a promise rejected with a
        // TypeError.
        if self.unusable(ec) {
            let error = ec.new_type_error("body stream already read");
            return rejected_promise(error, ec);
        }

        // Step 2: Let promise be a new promise.
        // Step 3: Let errorSteps given error be to reject promise with error.
        // Step 4: Let successSteps given a byte sequence data be to resolve
        // promise with the result of running convertBytesToJSValue with data.
        // If that threw an exception, then run errorSteps with that
        // exception.
        // Step 5: If object's body is null, then run successSteps with an
        // empty byte sequence.
        // Step 6: Otherwise, fully read object's body given successSteps,
        // errorSteps, and object's relevant global object.
        // Note: The body's source is in memory: reading it fully is
        // immediate, so the promise is settled here.
        let data = self
            .body
            .borrow()
            .as_ref()
            .map(|body| body.source.clone())
            .unwrap_or_default();
        self.mark_disturbed(ec);
        let promise = match convert_bytes_to_js_value(data, ec) {
            Ok(value) => resolved_promise(value, ec)?,
            Err(error) => rejected_promise(error, ec)?,
        };

        // Step 7: Return promise.
        Ok(promise)
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-arraybuffer>
    pub(crate) fn array_buffer(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // The arrayBuffer() method steps are to return the result of running
        // consume body with this and the following step given a byte sequence
        // bytes: return a new ArrayBuffer whose contents are bytes.
        self.consume_body(
            |bytes, ec| Ok(Types::value_from_object(create_array_buffer(&bytes, ec)?)),
            ec,
        )
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-blob>
    pub(crate) fn blob(
        &self,
        mime_type: Option<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // The blob() method steps are to return the result of running consume
        // body with this and the following step given a byte sequence bytes:
        // return a Blob whose contents are bytes and whose type attribute is
        // the result of get the MIME type with this.
        self.consume_body(
            move |bytes, ec| {
                let blob = Blob::with_type(bytes, mime_type.unwrap_or_default());
                let object = create_interface_instance::<Types, Blob>(blob, ec)?;
                Ok(Types::value_from_object(object))
            },
            ec,
        )
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-bytes>
    pub(crate) fn bytes(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // The bytes() method steps are to return the result of running
        // consume body with this and the following step given a byte sequence
        // bytes: return the result of creating a Uint8Array from bytes in
        // this's relevant realm.
        self.consume_body(
            |bytes, ec| Ok(Types::value_from_object(create_uint8_array(&bytes, ec)?)),
            ec,
        )
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-json>
    pub(crate) fn json(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
        // The json() method steps are to return the result of running consume
        // body with this and parse JSON from bytes.
        self.consume_body(
            |bytes, ec| parse_json_bytes_to_a_javascript_value(&bytes, ec),
            ec,
        )
    }

    /// <https://fetch.spec.whatwg.org/#dom-body-text>
    pub(crate) fn text(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
        // The text() method steps are to return the result of running consume
        // body with this and UTF-8 decode.
        self.consume_body(
            |bytes, ec| {
                let text = utf_8_decode(&bytes);
                let string = ec.js_string_from_str(&text);
                Ok(ec.value_from_string(string))
            },
            ec,
        )
    }
}
