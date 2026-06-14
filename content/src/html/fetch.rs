use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsString, JsValue,
    builtins::promise::ResolvingFunctions,
    native_function::NativeFunction,
    object::{
        JsObject,
        builtins::{JsArray, JsPromise, JsUint8Array},
    },
    property::PropertyKey,
};
use boa_gc::{Finalize, Gc, GcRef, GcRefCell, Trace};
use ipc_messages::content::{
    FetchResponse as ContentFetchResponse, HeaderList as ContentHeaderList,
};

use crate::streams::{
    ReadableStream, ReadableStreamDefaultReader, acquire_readable_stream_default_reader,
    readable_stream_from_iterable, with_readable_stream_default_reader_ref,
    with_readable_stream_ref,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{mark_promise_as_handled, rejected_promise};

/// <https://fetch.spec.whatwg.org/#headers-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct Headers {
    /// <https://fetch.spec.whatwg.org/#concept-headers-header-list>
    #[unsafe_ignore_trace]
    header_list: ContentHeaderList,
}

impl Headers {
    pub(crate) fn new(header_list: ContentHeaderList) -> Self {
        Self { header_list }
    }

    pub(crate) fn header_list(&self) -> ContentHeaderList {
        self.header_list.clone()
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-get>
    fn get(&self, name: &str) -> Option<String> {
        // Step 1: "If list does not contain name, then return null."
        let values = self
            .header_list
            .headers
            .iter()
            .filter(|(header_name, _value)| header_name.eq_ignore_ascii_case(name))
            .map(|(_header_name, value)| value.as_str())
            .collect::<Vec<_>>();

        if values.is_empty() {
            None
        } else {
            // Step 2: "Return the values of all headers in list whose name is a
            // byte-case-insensitive match for name, separated from each other by 0x2C 0x20, in
            // order."
            Some(values.join(", "))
        }
    }
}

impl WebIdlInterface for Headers {
    const NAME: &'static str = "Headers";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        // Step 1: "Set this’s guard to \"none\"."
        // Note: The Headers guard is not represented yet; all exposed Headers objects behave as
        // guard "none" until Request/Response guards are modeled.
        let header_list = if args.get_or_undefined(0).is_null_or_undefined() {
            ContentHeaderList::default()
        } else {
            // Step 2: "If init is given, then fill this with init."
            header_list_from_value(args.get_or_undefined(0), context)?
        };
        Ok(Self::new(header_list))
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_operation(OperationDef {
            id: "get",
            length: 1,
            method: headers_get_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

/// <https://fetch.spec.whatwg.org/#concept-body>
#[derive(Clone, Trace, Finalize)]
struct FetchBody {
    stream: ReadableStream,
    stream_object: JsObject,
}

impl FetchBody {
    /// <https://fetch.spec.whatwg.org/#concept-body>
    fn from_bytes(bytes: Vec<u8>, context: &mut Context) -> JsResult<Self> {
        // Step: "To get a byte sequence bytes as a body, return the body of the result of safely extracting bytes."
        // Note: Formal-web currently supports byte-sequence bodies as a one-chunk Uint8Array
        // ReadableStream. The Fetch body source and length fields are deferred until the broader
        // BodyInit extraction algorithm is implemented.
        let chunk = JsUint8Array::from_iter(bytes, context)?;
        let chunks = JsArray::from_iter([JsValue::from(chunk)], context);
        let stream_object = readable_stream_from_iterable(JsValue::from(chunks), context)?;
        let stream = with_readable_stream_ref(&stream_object, |stream| stream.clone())?;

        Ok(Self {
            stream,
            stream_object,
        })
    }

    fn stream_object(&self) -> JsObject {
        self.stream_object.clone()
    }

    /// <https://fetch.spec.whatwg.org/#body-unusable>
    fn unusable(&self) -> bool {
        // Step: "An object including the Body interface mixin is said to be unusable if its body is
        // non-null and its body’s stream is disturbed or locked."
        self.stream.disturbed() || self.stream.locked()
    }

    fn disturbed(&self) -> bool {
        self.stream.disturbed()
    }
}

/// <https://fetch.spec.whatwg.org/#response-class>
#[derive(Trace, Finalize, JsData)]
pub struct Response {
    /// <https://fetch.spec.whatwg.org/#concept-response-url-list>
    #[unsafe_ignore_trace]
    url_list: Vec<String>,

    /// <https://fetch.spec.whatwg.org/#concept-response-status>
    #[unsafe_ignore_trace]
    status: u16,

    /// <https://fetch.spec.whatwg.org/#concept-response-status-message>
    #[unsafe_ignore_trace]
    status_text: String,

    /// <https://fetch.spec.whatwg.org/#dom-response-headers>
    headers: JsObject,

    /// <https://fetch.spec.whatwg.org/#concept-response-body>
    body: Option<FetchBody>,
}

impl Response {
    /// <https://fetch.spec.whatwg.org/#response-create>
    pub(crate) fn from_content_fetch_response(
        response: ContentFetchResponse,
        context: &mut Context,
    ) -> JsResult<Self> {
        // Step 1: "Let responseObject be a new Response object with realm."
        // Note: The caller performs the Web IDL object allocation after this helper converts the
        // completed content fetch response into Response backing data.

        // Step 2: "Set responseObject’s response to response."
        let final_url = response.final_url.clone();
        let url_list = if response.url_list.is_empty() {
            vec![final_url]
        } else {
            response.url_list
        };
        // Step 3: "Set responseObject’s headers to a new Headers object with realm, whose headers
        // list is response’s headers list and guard is guard."
        // Note: The Headers guard is not represented yet; fetch-created Response objects therefore
        // expose the response header list without enforcing guard "immutable".
        let headers = create_interface_instance::<Headers>(
            Headers::new(response.header_list.clone()),
            context,
        )?;
        // Note: The HTTP-network fetch steps produce a null body for null-body status responses.
        // The request method is not available in this content callback yet, so HEAD response body
        // nulling remains deferred to the request/response plumbing.
        let body = if is_null_body_status(response.status) {
            None
        } else {
            Some(FetchBody::from_bytes(response.body, context)?)
        };

        // Step 4: "Return responseObject."
        Ok(Self {
            url_list,
            status: response.status,
            status_text: response.status_text,
            headers,
            body,
        })
    }

    fn new(
        body: Option<FetchBody>,
        status: u16,
        status_text: String,
        headers: JsObject,
    ) -> Self {
        Self {
            url_list: Vec::new(),
            status,
            status_text,
            body,
            headers,
        }
    }

    fn body_used(&self) -> bool {
        self.body.as_ref().is_some_and(FetchBody::disturbed)
    }

    fn body_unusable(&self) -> bool {
        self.body.as_ref().is_some_and(FetchBody::unusable)
    }
}

impl WebIdlInterface for Response {
    const NAME: &'static str = "Response";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        // Step 1: "Set this’s response to a new response."

        // Step 3: "Let bodyWithType be null."
        let body = if args.get_or_undefined(0).is_null_or_undefined() {
            None
        } else {
            // Step 4: "If body is non-null, then set bodyWithType to the result of extracting body."
            // Note: Full BodyInit extraction is not implemented yet. The current constructor path
            // extracts non-null bodies through Web IDL string conversion and stores the resulting
            // bytes in a Fetch body stream.
            Some(FetchBody::from_bytes(
                args.get_or_undefined(0)
                    .to_string(context)?
                    .to_std_string_escaped()
                    .into_bytes(),
                context,
            )?)
        };
        // Step 2: "Set this’s headers to a new Headers object with this’s relevant realm, whose
        // header list is this’s response’s header list and guard is \"response\"."
        // Note: The Headers guard is not represented yet; the stable Headers object below exposes
        // the initialized response header list but does not enforce guard "response". Construction
        // is delayed until after ResponseInit parsing because headers are stored directly on
        // `Response` instead of through a separate internal response object.
        let init = response_init(args.get_or_undefined(1), context)?;
        // Step 5: "Perform initialize a response given this, init, and bodyWithType."
        if body.is_some() {
            // Step 6.1: "If response’s status is a null body status, then throw a TypeError."
            if is_null_body_status(init.status) {
                return Err(type_error("Response body is not allowed for this status"));
            }
            // Step 6.2: "Set response’s body to body’s body."
            // Note: The `body` field already holds the extracted Fetch body used to initialize
            // the Response below.
            // TODO: Step 6.3: "If body’s type is non-null and response’s header list does not
            // contain `Content-Type`, then append (`Content-Type`, body’s type) to response’s
            // header list."
        }
        let headers =
            create_interface_instance::<Headers>(Headers::new(init.header_list.clone()), context)?;
        Ok(Self::new(
            body,
            init.status,
            init.status_text,
            headers,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_attribute(readonly_attribute("headers", response_headers_getter));
        def.add_attribute(readonly_attribute("ok", response_ok_getter));
        def.add_attribute(readonly_attribute("status", response_status_getter));
        def.add_attribute(readonly_attribute(
            "statusText",
            response_status_text_getter,
        ));
        def.add_attribute(readonly_attribute("url", response_url_getter));
        def.add_attribute(readonly_attribute("body", response_body_getter));
        def.add_attribute(readonly_attribute("bodyUsed", response_body_used_getter));
        def.add_operation(OperationDef {
            id: "text",
            length: 0,
            method: response_text_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

struct ResponseInit {
    status: u16,
    status_text: String,
    header_list: ContentHeaderList,
}

pub(crate) fn header_list_from_value(
    value: &JsValue,
    context: &mut Context,
) -> JsResult<ContentHeaderList> {
    // Note: This implements the `fill a Headers object with a given object` hook for the
    // currently-supported HeadersInit shapes. Sequence support is incomplete; enumerable object
    // records and existing Headers objects are preserved for current callers.
    let Some(object) = value.as_object() else {
        return Err(type_error("Headers init must be an object"));
    };

    if let Some(headers) = object.downcast_ref::<Headers>() {
        return Ok(headers.header_list());
    }

    let keys = object.own_property_keys(context)?;
    let mut headers = Vec::new();
    for key in keys {
        if !object.has_own_property(key.clone(), context)? {
            continue;
        }
        let desc = object.borrow().properties().get(&key);
        let enumerable = desc.as_ref().and_then(|d| d.enumerable()).unwrap_or(false);
        if !enumerable {
            continue;
        }

        let Some(name) = header_name_from_property_key(&key) else {
            continue;
        };
        if !is_header_name(&name) {
            return Err(type_error(format!("invalid header name `{name}`")));
        }
        let value = object
            .get(key, context)?
            .to_string(context)?
            .to_std_string_escaped();
        headers.push((name.to_ascii_lowercase(), value));
    }

    Ok(ContentHeaderList { headers })
}

/// <https://fetch.spec.whatwg.org/#initialize-a-response>
fn response_init(value: &JsValue, context: &mut Context) -> JsResult<ResponseInit> {
    // Note: This parses ResponseInit for the Response constructor's `initialize a response` step.
    let mut init = ResponseInit {
        status: 200,
        status_text: String::new(),
        header_list: ContentHeaderList::default(),
    };

    if value.is_null_or_undefined() {
        return Ok(init);
    }

    let Some(object) = value.as_object() else {
        return Err(type_error("Response init must be an object"));
    };

    let status = object.get(js_string("status"), context)?;
    if !status.is_null_or_undefined() {
        let status = status.to_u32(context)?;
        // Step 1: "If init[\"status\"] is not in the range 200 to 599, inclusive, then throw a
        // RangeError."
        if !(200..=599).contains(&status) {
            return Err(range_error(
                "Response status must be in the range 200 to 599",
            ));
        }
        // Step 3: "Set response’s response’s status to init[\"status\"]."
        init.status = status as u16;
    }

    let status_text = object.get(js_string("statusText"), context)?;
    if !status_text.is_null_or_undefined() {
        let status_text = status_text.to_string(context)?.to_std_string_escaped();
        // Step 2: "If init[\"statusText\"] is not the empty string and does not match the
        // reason-phrase token production, then throw a TypeError."
        if !status_text.is_empty() && !is_reason_phrase(&status_text) {
            return Err(type_error("Response statusText must match reason-phrase"));
        }
        // Step 4: "Set response’s response’s status message to init[\"statusText\"]."
        init.status_text = status_text;
    }

    let headers = object.get(js_string("headers"), context)?;
    if !headers.is_null_or_undefined() {
        // Step 5: "If init[\"headers\"] exists, then fill response’s headers with
        // init[\"headers\"]."
        init.header_list = header_list_from_value(&headers, context)?;
    }

    Ok(init)
}

fn headers_get_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();

    // Step 1: "If name is not a header name, then throw a TypeError."
    if !is_header_name(&name) {
        return Err(type_error(format!("invalid header name `{name}`")));
    }

    with_headers_ref(this, |headers| {
        // Step 2: "Return the result of getting name from this’s header list."
        Ok(headers
            .get(&name)
            .map(|value| JsValue::from(JsString::from(value.as_str())))
            .unwrap_or_else(JsValue::null))
    })?
}

/// <https://fetch.spec.whatwg.org/#dom-response-headers>
fn response_headers_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_response_ref(this, |response| {
        // Step: "The headers getter steps are to return this’s headers."
        JsValue::from(response.headers.clone())
    })
}

/// <https://fetch.spec.whatwg.org/#dom-response-ok>
fn response_ok_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_response_ref(this, |response| {
        // Step: "The ok getter steps are to return true if this’s response’s status is an ok
        // status; otherwise false."
        JsValue::from((200..=299).contains(&response.status))
    })
}

/// <https://fetch.spec.whatwg.org/#dom-response-status>
fn response_status_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_response_ref(this, |response| {
        // Step: "The status getter steps are to return this’s response’s status."
        JsValue::from(response.status)
    })
}

/// <https://fetch.spec.whatwg.org/#dom-response-statustext>
fn response_status_text_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_response_ref(this, |response| {
        // Step: "The statusText getter steps are to return this’s response’s status message."
        JsValue::from(JsString::from(response.status_text.as_str()))
    })
}

/// <https://fetch.spec.whatwg.org/#dom-response-url>
fn response_url_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_response_ref(this, |response| {
        // Step: "The url getter steps are to return the empty string if this’s response’s URL is
        // null; otherwise this’s response’s URL, serialized with exclude fragment set to true."
        let url = response
            .url_list
            .last()
            .map(|url| serialized_url_without_fragment(url))
            .unwrap_or_default();
        JsValue::from(JsString::from(url.as_str()))
    })
}

/// <https://fetch.spec.whatwg.org/#dom-body-body>
fn response_body_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_response_ref(this, |response| {
        // Step: "The body getter steps are to return null if this’s body is null; otherwise this’s
        // body’s stream."
        response
            .body
            .as_ref()
            .map(|body| JsValue::from(body.stream_object()))
            .unwrap_or_else(JsValue::null)
    })
}

/// <https://fetch.spec.whatwg.org/#dom-body-bodyused>
fn response_body_used_getter(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    // Step: "The bodyUsed getter steps are to return true if this’s body is non-null and this’s
    // body’s stream is disturbed; otherwise false."
    with_response_ref(this, |response| JsValue::from(response.body_used()))
}

/// <https://fetch.spec.whatwg.org/#dom-body-text>
fn response_text_method(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step: "The text() method steps are to return the result of running consume body with this and UTF-8 decode."
    with_response_ref(this, |response| consume_body_as_text(&response, context))?
}

/// <https://fetch.spec.whatwg.org/#concept-body-consume-body>
fn consume_body_as_text(response: &Response, context: &mut Context) -> JsResult<JsValue> {
    // Step 1: "If object is unusable, then return a promise rejected with a TypeError."
    if response.body_unusable() {
        let reason = JsNativeError::typ()
            .with_message("body has already been consumed")
            .into_opaque(context)
            .into();
        return rejected_promise(reason, context).map(JsValue::from);
    }

    // Step 2: "Let promise be a new promise."
    let (promise, resolvers) = JsPromise::new_pending(context);
    let promise_object: JsObject = promise.into();

    // Step 3: "Let errorSteps given error be to reject promise with error."
    // Note: `BodyTextReadState::reject` stores the promise resolver used by these steps.

    // Step 4: "Let successSteps given a byte sequence data be to resolve promise with the result of
    // running convertBytesToJSValue with data."
    // Note: `BodyTextReadState::resolve_with_utf8_decode` performs the UTF-8 decode conversion for
    // Body.text().

    // Step 5: "If object’s body is null, then run successSteps with an empty byte sequence."
    let Some(body) = response.body.as_ref() else {
        let text = JsValue::from(JsString::from(""));
        resolvers
            .resolve
            .call(&JsValue::undefined(), &[text], context)?;
        return Ok(JsValue::from(promise_object));
    };

    // Step 6: "Otherwise, fully read object’s body given successSteps, errorSteps, and object’s
    // relevant global object."
    let reader_object = acquire_readable_stream_default_reader(body.stream.clone(), context)?;
    let reader =
        with_readable_stream_default_reader_ref(&reader_object, |reader| reader.clone())?;
    let state = BodyTextReadState::new(reader, resolvers);
    queue_body_text_read(state, context)?;

    // Step 7: "Return promise."
    Ok(JsValue::from(promise_object))
}

/// <https://streams.spec.whatwg.org/#readablestreamdefaultreader-read-all-bytes>
fn queue_body_text_read(state: BodyTextReadState, context: &mut Context) -> JsResult<()> {
    // Step: "To read all bytes from a ReadableStreamDefaultReader reader... read-loop given reader,
    // a new byte sequence, successSteps, and failureSteps."
    // Step 1: "Let readRequest be a new read request with the following items:"
    // Note: The local Streams API exposes the read request through `ReadableStreamDefaultReader.read()`;
    // the fulfillment and rejection callbacks below implement the read request items.
    // Step 2: "Perform ! ReadableStreamDefaultReaderRead(reader, readRequest)."
    let read_promise = state.reader.read(context)?;
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &BodyTextReadState, context| {
            handle_body_text_read_result(args.get_or_undefined(0), state.clone(), context)?;
            Ok(JsValue::undefined())
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &BodyTextReadState, context| {
            // Step error steps 1: "Call failureSteps with e."
            state.reject(args.get_or_undefined(0).clone(), context)?;
            Ok(JsValue::undefined())
        },
        state,
    )
    .to_js_function(context.realm());
    let reaction: JsObject = JsPromise::from_object(read_promise)?
        .then(Some(on_fulfilled), Some(on_rejected), context)?
        .into();
    mark_promise_as_handled(&reaction, context)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#read-loop>
fn handle_body_text_read_result(
    read_result: &JsValue,
    state: BodyTextReadState,
    context: &mut Context,
) -> JsResult<()> {
    let read_result_object = read_result.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream reader result must be an object")
    })?;
    let done = read_result_object
        .get(js_string("done"), context)?
        .to_boolean();

    if done {
        // Step close steps 1: "Call successSteps with bytes."
        state.resolve_with_utf8_decode(context)?;
        return Ok(());
    }

    let chunk = read_result_object.get(js_string("value"), context)?;
    let chunk_bytes = match bytes_from_uint8_array(&chunk, context) {
        Ok(bytes) => bytes,
        Err(error) => {
            // Step chunk steps 1: "If chunk is not a Uint8Array object, call failureSteps with a
            // TypeError and abort these steps."
            state.reject(error.into_opaque(context)?.into(), context)?;
            return Ok(());
        }
    };

    // Step chunk steps 2: "Append the bytes represented by chunk to bytes."
    state.bytes.borrow_mut().extend(chunk_bytes);

    // Step chunk steps 3: "Read-loop given reader, bytes, successSteps, and failureSteps."
    queue_body_text_read(state, context)
}

#[derive(Clone, Trace, Finalize)]
struct BodyTextReadState {
    reader: ReadableStreamDefaultReader,
    bytes: Gc<GcRefCell<Vec<u8>>>,
    resolvers: ResolvingFunctions,
}

impl BodyTextReadState {
    fn new(reader: ReadableStreamDefaultReader, resolvers: ResolvingFunctions) -> Self {
        Self {
            reader,
            bytes: Gc::new(GcRefCell::new(Vec::new())),
            resolvers,
        }
    }

    fn resolve_with_utf8_decode(&self, context: &mut Context) -> JsResult<()> {
        let bytes = self.bytes.borrow();
        let decoded_text = String::from_utf8_lossy(&bytes).into_owned();
        // Note: Fetch's Body.text() conversion uses UTF-8 decode, which strips one leading BOM.
        let text = decoded_text
            .strip_prefix('\u{feff}')
            .unwrap_or(decoded_text.as_str());
        self.resolvers.resolve.call(
            &JsValue::undefined(),
            &[JsValue::from(JsString::from(text))],
            context,
        )?;
        Ok(())
    }

    fn reject(&self, reason: JsValue, context: &mut Context) -> JsResult<()> {
        self.resolvers
            .reject
            .call(&JsValue::undefined(), &[reason], context)?;
        Ok(())
    }
}

fn readonly_attribute(
    id: &'static str,
    getter: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
) -> AttributeDef {
    AttributeDef {
        id,
        getter,
        setter: None,
        static_: false,
        unforgeable: false,
        promise_type: false,
        legacy_lenient_this: false,
        replaceable: false,
        put_forwards: None,
        legacy_lenient_setter: false,
    }
}

fn serialized_url_without_fragment(url: &str) -> String {
    let Ok(mut parsed_url) = url::Url::parse(url) else {
        return url.to_owned();
    };
    parsed_url.set_fragment(None);
    parsed_url.to_string()
}

fn bytes_from_uint8_array(value: &JsValue, context: &mut Context) -> JsResult<Vec<u8>> {
    let object = value
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("chunk is not a Uint8Array object"))?;
    let array = JsUint8Array::from_object(object.clone())?;
    array.to_vec(context)
}

fn with_headers_ref<R>(this: &JsValue, f: impl FnOnce(GcRef<'_, Headers>) -> R) -> JsResult<R> {
    let Some(object) = this.as_object() else {
        return Err(type_error("receiver is not a Headers"));
    };
    let Some(headers) = object.downcast_ref::<Headers>() else {
        return Err(type_error("receiver is not a Headers"));
    };
    Ok(f(headers))
}

fn with_response_ref<R>(this: &JsValue, f: impl FnOnce(GcRef<'_, Response>) -> R) -> JsResult<R> {
    let Some(object) = this.as_object() else {
        return Err(type_error("receiver is not a Response"));
    };
    let Some(response) = object.downcast_ref::<Response>() else {
        return Err(type_error("receiver is not a Response"));
    };
    Ok(f(response))
}

fn header_name_from_property_key(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::String(name) => Some(name.to_std_string_escaped()),
        PropertyKey::Index(index) => Some(index.get().to_string()),
        PropertyKey::Symbol(_) => None,
    }
}

fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            matches!(
                byte,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
                    | b'0'..=b'9'
                    | b'A'..=b'Z'
                    | b'a'..=b'z'
            )
        })
}

fn is_null_body_status(status: u16) -> bool {
    matches!(status, 101 | 103 | 204 | 205 | 304)
}

fn is_reason_phrase(value: &str) -> bool {
    value
        .chars()
        .all(|character| matches!(character, '\t' | ' '..='~' | '\u{80}'..='\u{ff}'))
}

fn js_string(value: &'static str) -> JsString {
    JsString::from(value)
}

fn range_error(message: impl Into<String>) -> boa_engine::JsError {
    JsNativeError::range().with_message(message.into()).into()
}

fn type_error(message: impl Into<String>) -> boa_engine::JsError {
    JsNativeError::typ().with_message(message.into()).into()
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use blitz_dom::{BaseDocument, DocumentConfig};
    use url::Url;

    use crate::html::EnvironmentSettingsObject;

    fn environment_settings_object() -> EnvironmentSettingsObject {
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        EnvironmentSettingsObject::new(
            document,
            Url::parse("https://example.test/").expect("test URL must parse"),
            None,
            None,
            None,
        )
        .expect("environment settings object must initialize")
    }

    #[test]
    fn headers_get_combines_case_insensitive_values() {
        let mut settings = environment_settings_object();
        let value = settings
            .evaluate_script_to_json(
                r#"
                const headers = new Headers({ "X-Test": "one", "x-test": "two" });
                ({
                    exact: headers.get("x-test"),
                    missing: headers.get("missing") === null,
                });
                "#,
            )
            .expect("headers script must evaluate");

        assert_eq!(
            value,
            serde_json::json!({
                "exact": "one, two",
                "missing": true,
            })
        );
    }

    #[test]
    fn response_text_consumes_body_once() {
        let mut settings = environment_settings_object();
        settings
            .evaluate_script(
                r#"
                globalThis.fetchTestResult = null;
                const response = new Response("hello", {
                    status: 201,
                    statusText: "Created",
                    headers: { "Content-Type": "text/plain" },
                });
                const before = response.bodyUsed;
                response.text().then(text => {
                    const afterFirst = response.bodyUsed;
                    response.text().then(
                        () => { globalThis.fetchTestResult = { unexpected: true }; },
                        error => {
                            globalThis.fetchTestResult = {
                                text,
                                before,
                                afterFirst,
                                afterSecond: response.bodyUsed,
                                ok: response.ok,
                                status: response.status,
                                statusText: response.statusText,
                                contentType: response.headers.get("content-type"),
                                errorName: error.name,
                            };
                        },
                    );
                });
                "#,
            )
            .expect("response script must evaluate");

        let value = settings
            .evaluate_script_to_json("globalThis.fetchTestResult")
            .expect("result must serialize");

        assert_eq!(
            value,
            serde_json::json!({
                "text": "hello",
                "before": false,
                "afterFirst": true,
                "afterSecond": true,
                "ok": true,
                "status": 201,
                "statusText": "Created",
                "contentType": "text/plain",
                "errorName": "TypeError",
            })
        );
    }
}
