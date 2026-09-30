use std::cell::RefCell;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::infra::serialize_a_javascript_value_to_json_bytes;
use crate::js::Types;
use crate::url_standard::api_url_parser;
use crate::webidl::bindings::create_interface_instance;

use super::body::{Body, BodyInit, BodyMixin, extract};
use super::{
    HeaderList, Headers, HeadersGuard, HeadersInit, is_null_body_status, is_redirect_status,
};

type JsValue = <Types as JsTypes>::JsValue;

/// <https://fetch.spec.whatwg.org/#concept-response-type>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseType {
    Basic,
    Default,
    Error,
}

impl ResponseType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Default => "default",
            Self::Error => "error",
        }
    }
}

/// <https://fetch.spec.whatwg.org/#concept-response>
#[derive(Debug, Clone)]
pub(crate) struct InternalResponse {
    /// <https://fetch.spec.whatwg.org/#concept-response-type>
    pub(crate) type_: ResponseType,
    /// <https://fetch.spec.whatwg.org/#concept-response-url-list>
    pub(crate) url_list: Vec<String>,
    /// <https://fetch.spec.whatwg.org/#concept-response-status>
    pub(crate) status: u16,
    /// <https://fetch.spec.whatwg.org/#concept-response-status-message>
    pub(crate) status_message: String,
    /// <https://fetch.spec.whatwg.org/#concept-response-header-list>
    pub(crate) header_list: Rc<RefCell<HeaderList>>,
    /// The response's URL list has more than one URL: a redirect was
    /// followed.
    /// <https://fetch.spec.whatwg.org/#concept-response-url-list>
    pub(crate) redirected: bool,
}

impl InternalResponse {
    /// <https://fetch.spec.whatwg.org/#concept-response>
    pub(crate) fn new() -> Self {
        Self {
            type_: ResponseType::Default,
            url_list: Vec::new(),
            status: 200,
            status_message: String::new(),
            header_list: Rc::new(RefCell::new(HeaderList::new())),
            redirected: false,
        }
    }

    /// <https://fetch.spec.whatwg.org/#concept-network-error>
    pub(crate) fn network_error() -> Self {
        // A network error is a response whose type is "error", status is 0,
        // status message is the empty byte sequence, header list is « »,
        // body is null, and body info is a new response body info.
        Self {
            type_: ResponseType::Error,
            url_list: Vec::new(),
            status: 0,
            status_message: String::new(),
            header_list: Rc::new(RefCell::new(HeaderList::new())),
            redirected: false,
        }
    }
}

/// <https://fetch.spec.whatwg.org/#responseinit>
#[derive(Default)]
pub(crate) struct ResponseInit {
    pub(crate) status: Option<u16>,
    pub(crate) status_text: Option<String>,
    pub(crate) headers: Option<HeadersInit>,
}

/// <https://fetch.spec.whatwg.org/#response-class>
#[gc_struct]
pub(crate) struct Response {
    /// <https://fetch.spec.whatwg.org/#concept-response-response>
    #[ignore_trace]
    response: Rc<RefCell<InternalResponse>>,

    /// <https://fetch.spec.whatwg.org/#response-headers>
    headers: Headers,

    /// <https://fetch.spec.whatwg.org/#concept-body-body>
    body: BodyMixin,
}

/// <https://fetch.spec.whatwg.org/#reason-phrase-token>
fn is_reason_phrase(value: &str) -> bool {
    // A reason-phrase token is a byte sequence that matches the
    // reason-phrase token production: HTTP tab or space, VCHAR and
    // obs-text.
    value
        .bytes()
        .all(|byte| matches!(byte, b'\t' | b' ' | 0x21..=0x7E | 0x80..=0xFF))
}

impl Response {
    /// <https://fetch.spec.whatwg.org/#response-create>
    pub(crate) fn create(
        response: InternalResponse,
        body: Option<Body>,
        guard: HeadersGuard,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let responseObject be a new Response object with realm.
        // Step 2: Set responseObject's response to response.
        // Step 3: Set responseObject's headers to a new Headers object with
        // realm, whose headers list is response's header list and guard is
        // guard.
        let header_list = Rc::clone(&response.header_list);
        let headers = Headers::new(header_list, guard);
        let headers_object = create_interface_instance::<Types, Headers>(headers, ec)?;
        let headers = ec
            .with_object_any(&headers_object)
            .and_then(|data| data.downcast_ref::<Headers>().cloned())
            .ok_or_else(|| ec.new_type_error("Headers object has no platform data"))?;

        // Step 4: Return responseObject.
        Ok(Self {
            response: Rc::new(RefCell::new(response)),
            headers,
            body: BodyMixin::new(body, ec),
        })
    }

    /// <https://fetch.spec.whatwg.org/#initialize-a-response>
    fn initialize(
        &self,
        init: ResponseInit,
        body: Option<super::body::BodyWithType>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If init["status"] is not in the range 200 to 599,
        // inclusive, then throw a RangeError.
        let status = init.status.unwrap_or(200);
        if !(200..=599).contains(&status) {
            return Err(ec.new_range_error(&format!(
                "The status provided ({status}) is outside the range [200, 599]"
            )));
        }

        // Step 2: If init["statusText"] is not the empty string and does not
        // match the reason-phrase token production, then throw a TypeError.
        let status_text = init.status_text.unwrap_or_default();
        if !status_text.is_empty() && !is_reason_phrase(&status_text) {
            return Err(ec.new_type_error("Invalid statusText"));
        }

        // Step 3: Set response's response's status to init["status"].
        // Step 4: Set response's response's status message to
        // init["statusText"].
        {
            let mut response = self.response.borrow_mut();
            response.status = status;
            response.status_message = status_text;
        }

        // Step 5: If init["headers"] exists, then fill response's headers
        // with init["headers"].
        if let Some(headers) = init.headers {
            self.headers.fill(headers, ec)?;
        }

        // Step 6: If body is non-null, then:
        if let Some(body) = body {
            // Step 6.1: If response's status is a null body status, then
            // throw a TypeError.
            if is_null_body_status(status) {
                return Err(ec.new_type_error("Response with null body status cannot have body"));
            }

            // Step 6.2: Set response's body to body's body.
            self.body.set_body(body.body);

            // Step 6.3: If body's type is non-null and response's header list
            // does not contain `Content-Type`, then append (`Content-Type`,
            // body's type) to response's header list.
            if let Some(type_) = body.type_
                && !self.headers.header_list().borrow().contains("Content-Type")
            {
                self.headers
                    .header_list()
                    .borrow_mut()
                    .append("Content-Type", &type_);
            }
        }
        Ok(())
    }

    /// <https://fetch.spec.whatwg.org/#dom-response>
    pub(crate) fn constructor(
        body: Option<BodyInit>,
        init: ResponseInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Set this's response to a new response.
        // Step 2: Set this's headers to a new Headers object with this's
        // relevant realm, whose header list is this's response's header list
        // and guard is "response".
        let response = Self::create(InternalResponse::new(), None, HeadersGuard::Response, ec)?;

        // Step 3: Let bodyWithType be null.
        // Step 4: If body is non-null, then set bodyWithType to the result of
        // extracting body.
        let body_with_type = body.map(extract);

        // Step 5: Perform initialize a response given this, init, and
        // bodyWithType.
        response.initialize(init, body_with_type, ec)?;
        Ok(response)
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-error>
    pub(crate) fn error(ec: &mut dyn ExecutionContext<Types>) -> Completion<Self, Types> {
        // The static error() method steps are to return the result of
        // creating a Response object, given a new network error, "immutable",
        // and the current realm.
        Self::create(
            InternalResponse::network_error(),
            None,
            HeadersGuard::Immutable,
            ec,
        )
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-redirect>
    pub(crate) fn redirect(
        url: String,
        status: u16,
        base_url: Option<&str>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let parsedURL be the result of parsing url with the current
        // settings object's API base URL.
        // Step 2: If parsedURL is failure, then throw a TypeError.
        let Some(parsed_url) = api_url_parser(&url, base_url) else {
            return Err(ec.new_type_error(&format!("Failed to parse URL from {url}")));
        };

        // Step 3: If status is not a redirect status, then throw a
        // RangeError.
        if !is_redirect_status(status) {
            return Err(ec.new_range_error("Invalid status code"));
        }

        // Step 4: Let responseObject be the result of creating a Response
        // object, given a new response, "immutable", and the current realm.
        let response_object =
            Self::create(InternalResponse::new(), None, HeadersGuard::Immutable, ec)?;

        // Step 5: Set responseObject's response's status to status.
        response_object.response.borrow_mut().status = status;

        // Step 6: Let value be parsedURL, serialized and isomorphic encoded.
        // Step 7: Append (`Location`, value) to responseObject's response's
        // header list.
        response_object
            .response
            .borrow()
            .header_list
            .borrow_mut()
            .append("Location", parsed_url.as_str());

        // Step 8: Return responseObject.
        Ok(response_object)
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-json>
    pub(crate) fn json(
        data: JsValue,
        init: ResponseInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let bytes the result of running serialize a JavaScript value
        // to JSON bytes on data.
        let bytes = serialize_a_javascript_value_to_json_bytes(data, ec)?;

        // Step 2: Let body be the result of extracting bytes.
        let mut body = extract(BodyInit::BufferSource(bytes));

        // Step 3: Let responseObject be the result of creating a Response
        // object, given a new response, "response", and the current realm.
        let response_object =
            Self::create(InternalResponse::new(), None, HeadersGuard::Response, ec)?;

        // Step 4: Perform initialize a response given responseObject, init,
        // and (body, "application/json").
        body.type_ = Some(String::from("application/json"));
        response_object.initialize(init, Some(body), ec)?;

        // Step 5: Return responseObject.
        Ok(response_object)
    }

    pub(crate) fn body_mixin(&self) -> &BodyMixin {
        &self.body
    }

    pub(crate) fn headers(&self) -> &Headers {
        &self.headers
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-type>
    pub(crate) fn type_(&self) -> String {
        // The type getter steps are to return this's response's type.
        self.response.borrow().type_.as_str().to_owned()
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-url>
    pub(crate) fn url(&self) -> String {
        // The url getter steps are to return the empty string if this's
        // response's URL is null; otherwise this's response's URL, serialized
        // with exclude fragment set to true.
        let response = self.response.borrow();
        let Some(url) = response.url_list.last() else {
            return String::new();
        };
        match url.split_once('#') {
            Some((without_fragment, _)) => without_fragment.to_owned(),
            None => url.clone(),
        }
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-redirected>
    pub(crate) fn redirected(&self) -> bool {
        // The redirected getter steps are to return true if this's response's
        // URL list has more than one item; otherwise false.
        let response = self.response.borrow();
        response.redirected || response.url_list.len() > 1
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-status>
    pub(crate) fn status(&self) -> u16 {
        // The status getter steps are to return this's response's status.
        self.response.borrow().status
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-ok>
    pub(crate) fn ok(&self) -> bool {
        // The ok getter steps are to return true if this's response's status
        // is an ok status; otherwise false.
        (200..=299).contains(&self.response.borrow().status)
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-statustext>
    pub(crate) fn status_text(&self) -> String {
        // The statusText getter steps are to return this's response's status
        // message.
        self.response.borrow().status_message.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-response-clone>
    pub(crate) fn clone_method(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: If this is unusable, then throw a TypeError.
        if self.body.unusable(ec) {
            return Err(ec.new_type_error("Response body is already used"));
        }

        // Step 2: Let clonedResponse be the result of cloning this's
        // response.
        let mut cloned_response = self.response.borrow().clone();
        let copied_header_list = cloned_response.header_list.borrow().clone();
        cloned_response.header_list = Rc::new(RefCell::new(copied_header_list));
        let cloned_body = self.body.clone_body();

        // Step 3: Return the result of creating a Response object, given
        // clonedResponse, this's headers's guard, and this's relevant realm.
        Self::create(cloned_response, cloned_body, self.headers.guard(), ec)
    }
}
