use std::cell::RefCell;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, gc_struct};

use crate::dom::{AbortSignal, create_abort_signal, initialize_dependent_abort_signal};
use crate::js::Types;
use crate::url_standard::api_url_parser;
use crate::webidl::bindings::create_interface_instance;

use super::body::{BodyInit, BodyMixin, extract};
use super::{
    HeaderList, Headers, HeadersGuard, HeadersInit, is_forbidden_method, is_method,
    normalize_method,
};

/// <https://fetch.spec.whatwg.org/#concept-request>
#[derive(Debug, Clone)]
pub(crate) struct InternalRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request-method>
    pub(crate) method: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-url>
    pub(crate) url: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-header-list>
    pub(crate) header_list: Rc<RefCell<HeaderList>>,
    /// <https://fetch.spec.whatwg.org/#concept-request-referrer>
    pub(crate) referrer: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-referrer-policy>
    pub(crate) referrer_policy: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-mode>
    pub(crate) mode: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-credentials-mode>
    pub(crate) credentials_mode: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-cache-mode>
    pub(crate) cache_mode: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-redirect-mode>
    pub(crate) redirect_mode: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-integrity-metadata>
    pub(crate) integrity_metadata: String,
    /// <https://fetch.spec.whatwg.org/#request-keepalive-flag>
    pub(crate) keepalive: bool,
    /// <https://fetch.spec.whatwg.org/#request-duplex>
    pub(crate) duplex: String,
}

impl InternalRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request>
    fn new(url: String) -> Self {
        Self {
            method: String::from("GET"),
            url,
            header_list: Rc::new(RefCell::new(HeaderList::new())),
            referrer: String::from("client"),
            referrer_policy: String::new(),
            mode: String::from("no-cors"),
            credentials_mode: String::from("same-origin"),
            cache_mode: String::from("default"),
            redirect_mode: String::from("follow"),
            integrity_metadata: String::new(),
            keepalive: false,
            duplex: String::from("half"),
        }
    }
}

/// <https://fetch.spec.whatwg.org/#typedefdef-requestinfo>
pub(crate) enum RequestInfo {
    Request(Request),
    USVString(String),
}

/// <https://fetch.spec.whatwg.org/#requestinit>
#[derive(Default)]
pub(crate) struct RequestInit {
    pub(crate) method: Option<String>,
    pub(crate) headers: Option<HeadersInit>,
    pub(crate) body: Option<Option<BodyInit>>,
    pub(crate) referrer: Option<String>,
    pub(crate) referrer_policy: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) credentials: Option<String>,
    pub(crate) cache: Option<String>,
    pub(crate) redirect: Option<String>,
    pub(crate) integrity: Option<String>,
    pub(crate) keepalive: Option<bool>,
    pub(crate) signal: Option<Option<AbortSignal>>,
    pub(crate) duplex: Option<String>,
    pub(crate) window: Option<()>,
    /// Whether the dictionary had any member: the constructor's "init is
    /// not empty" tests.
    pub(crate) is_empty: bool,
}

/// <https://fetch.spec.whatwg.org/#request-class>
#[gc_struct]
pub(crate) struct Request {
    /// <https://fetch.spec.whatwg.org/#concept-request-request>
    #[ignore_trace]
    request: Rc<RefCell<InternalRequest>>,

    /// <https://fetch.spec.whatwg.org/#request-headers>
    headers: Headers,

    /// <https://fetch.spec.whatwg.org/#request-signal>
    signal: AbortSignal,

    /// <https://fetch.spec.whatwg.org/#concept-body-body>
    body: BodyMixin,
}

impl Request {
    /// <https://fetch.spec.whatwg.org/#dom-request>
    pub(crate) fn constructor(
        input: RequestInfo,
        init: RequestInit,
        base_url: Option<&str>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let request be null.
        // Step 2: Let fallbackMode be null.
        let mut fallback_mode: Option<&str> = None;

        // Step 3: Let baseURL be this's relevant settings object's API base
        // URL.
        // Step 4: Let signal be null.
        let mut signal: Option<AbortSignal> = None;

        // Step 5: If input is a string, then:
        let (mut request, input_body) = match input {
            RequestInfo::USVString(input) => {
                // Step 5.1: Let parsedURL be the result of parsing input with
                // baseURL.
                // Step 5.2: If parsedURL is failure, then throw a TypeError.
                let Some(parsed_url) = api_url_parser(&input, base_url) else {
                    return Err(ec.new_type_error(&format!("Failed to parse URL from {input}")));
                };

                // Step 5.3: If parsedURL includes credentials, then throw a
                // TypeError.
                if !parsed_url.username().is_empty() || parsed_url.password().is_some() {
                    return Err(ec.new_type_error(
                        "Request cannot be constructed from a URL that includes credentials",
                    ));
                }

                // Step 5.4: Set request to a new request whose URL is
                // parsedURL.
                // Step 5.5: Set fallbackMode to "cors".
                fallback_mode = Some("cors");
                (InternalRequest::new(parsed_url.to_string()), None)
            }
            // Step 6: Otherwise:
            RequestInfo::Request(input) => {
                // Step 6.1: Assert: input is a Request object.
                // Step 6.2: Set request to input's request.
                let request = input.request.borrow().clone();

                // Step 6.3: Set signal to input's signal.
                signal = Some(input.signal.clone());
                (request, Some(input))
            }
        };

        // Step 7: Let origin be this's relevant settings object's origin.
        // Step 8: Let traversableForUserPrompts be "client".
        // Step 9: If request's traversable for user prompts is an environment
        // settings object and its origin is same origin with origin, then set
        // traversableForUserPrompts to request's traversable for user
        // prompts.
        // Step 10: If init["window"] exists and is non-null, then throw a
        // TypeError.
        // Note: The binding rejects a non-null window member before calling
        // the constructor.
        // Step 11: If init["window"] exists, then set traversableForUserPrompts
        // to "no-traversable".
        // Step 12: Set request to a new request with the following
        // properties: ... header list: A copy of request's header list. ...
        let copied_header_list = request.header_list.borrow().clone();
        request.header_list = Rc::new(RefCell::new(copied_header_list));

        // Step 13: If init is not empty, then:
        if !init.is_empty {
            // Step 13.1: If request's mode is "navigate", then set it to
            // "same-origin".
            if request.mode == "navigate" {
                request.mode = String::from("same-origin");
            }

            // Step 13.2: Unset request's reload-navigation flag.
            // Step 13.3: Unset request's history-navigation flag.
            // Step 13.4: Set request's origin to "client".
            // Step 13.5: Set request's referrer to "client".
            request.referrer = String::from("client");

            // Step 13.6: Set request's referrer policy to the empty string.
            request.referrer_policy = String::new();

            // Step 13.7: Set request's URL to request's current URL.
            // Step 13.8: Set request's URL list to « request's URL ».
        }

        // Step 14: If init["referrer"] exists, then:
        if let Some(referrer) = &init.referrer {
            // Step 14.1: Let referrer be init["referrer"].
            // Step 14.2: If referrer is the empty string, then set request's
            // referrer to "no-referrer".
            if referrer.is_empty() {
                request.referrer = String::from("no-referrer");
            } else {
                // Step 14.3: Otherwise:
                // Step 14.3.1: Let parsedReferrer be the result of parsing
                // referrer with baseURL.
                // Step 14.3.2: If parsedReferrer is failure, then throw a
                // TypeError.
                let Some(parsed_referrer) = api_url_parser(referrer, base_url) else {
                    return Err(
                        ec.new_type_error(&format!("Referrer '{referrer}' is not a valid URL"))
                    );
                };

                // Step 14.3.3: If one of the following is true: parsedReferrer's
                // scheme is "about" and path is the string "client";
                // parsedReferrer's origin is not same origin with origin, then
                // set request's referrer to "client".
                // Step 14.3.4: Otherwise, set request's referrer to
                // parsedReferrer.
                // Note: The same-origin test is not run: the referrer is
                // recorded and the net process applies its policy.
                request.referrer =
                    if parsed_referrer.scheme() == "about" && parsed_referrer.path() == "client" {
                        String::from("client")
                    } else {
                        parsed_referrer.to_string()
                    };
            }
        }

        // Step 15: If init["referrerPolicy"] exists, then set request's
        // referrer policy to it.
        if let Some(referrer_policy) = &init.referrer_policy {
            request.referrer_policy = referrer_policy.clone();
        }

        // Step 16: Let mode be init["mode"] if it exists, and fallbackMode
        // otherwise.
        let mode = init
            .mode
            .clone()
            .or_else(|| fallback_mode.map(str::to_owned));

        // Step 17: If mode is "navigate", then throw a TypeError.
        if mode.as_deref() == Some("navigate") {
            return Err(ec.new_type_error("Cannot construct a Request with a RequestInit whose mode member is set as 'navigate'"));
        }

        // Step 18: If mode is non-null, set request's mode to mode.
        if let Some(mode) = mode {
            request.mode = mode;
        }

        // Step 19: If init["credentials"] exists, then set request's
        // credentials mode to it.
        if let Some(credentials) = &init.credentials {
            request.credentials_mode = credentials.clone();
        }

        // Step 20: If init["cache"] exists, then set request's cache mode to
        // it.
        if let Some(cache) = &init.cache {
            request.cache_mode = cache.clone();
        }

        // Step 21: If request's cache mode is "only-if-cached" and request's
        // mode is not "same-origin", then throw a TypeError.
        if request.cache_mode == "only-if-cached" && request.mode != "same-origin" {
            return Err(
                ec.new_type_error("'only-if-cached' can be set only with 'same-origin' mode")
            );
        }

        // Step 22: If init["redirect"] exists, then set request's redirect
        // mode to it.
        if let Some(redirect) = &init.redirect {
            request.redirect_mode = redirect.clone();
        }

        // Step 23: If init["integrity"] exists, then set request's integrity
        // metadata to it.
        if let Some(integrity) = &init.integrity {
            request.integrity_metadata = integrity.clone();
        }

        // Step 24: If init["keepalive"] exists, then set request's keepalive
        // to it.
        if let Some(keepalive) = init.keepalive {
            request.keepalive = keepalive;
        }

        // Step 25: If init["method"] exists, then:
        if let Some(method) = &init.method {
            // Step 25.1: Let method be init["method"].
            // Step 25.2: If method is not a method or method is a forbidden
            // method, then throw a TypeError.
            if !is_method(method) {
                return Err(ec.new_type_error(&format!("'{method}' is not a valid HTTP method")));
            }
            if is_forbidden_method(method) {
                return Err(ec.new_type_error(&format!("'{method}' HTTP method is unsupported")));
            }

            // Step 25.3: Normalize method.
            // Step 25.4: Set request's method to method.
            request.method = normalize_method(method);
        }

        // Step 26: If init["signal"] exists, then set signal to it.
        if let Some(init_signal) = init.signal {
            signal = init_signal;
        }

        // Step 27: If init["priority"] exists, then: ...
        // Step 28: Set this's request to request.
        let request = Rc::new(RefCell::new(request));

        // Step 29: Let signals be « signal » if signal is non-null; otherwise
        // « ».
        let signals: Vec<AbortSignal> = signal.into_iter().collect();

        // Step 30: Set this's signal to the result of creating a dependent
        // abort signal from signals, using AbortSignal and this's relevant
        // realm.
        let this_signal = create_abort_signal(AbortSignal::new(ec), ec)?;
        initialize_dependent_abort_signal(&this_signal, &signals, ec);

        // Step 31: Set this's headers to a new Headers object with this's
        // relevant realm, whose header list is request's header list and
        // guard is "request".
        let header_list = Rc::clone(&request.borrow().header_list);
        let headers = Headers::new(header_list, HeadersGuard::Request);
        let headers_object = create_interface_instance::<Types, Headers>(headers, ec)?;
        let headers = ec
            .with_object_any(&headers_object)
            .and_then(|data| data.downcast_ref::<Headers>().cloned())
            .ok_or_else(|| ec.new_type_error("Headers object has no platform data"))?;

        // Step 32: If this's request's mode is "no-cors", then:
        if request.borrow().mode == "no-cors" {
            // Step 32.1: If this's request's method is not a CORS-safelisted
            // method, then throw a TypeError.
            if !["GET", "HEAD", "POST"].contains(&request.borrow().method.as_str()) {
                return Err(ec.new_type_error("'no-cors' mode requires a CORS-safelisted method"));
            }

            // Step 32.2: Set this's headers's guard to "request-no-cors".
            headers.set_guard(HeadersGuard::RequestNoCors);
        }

        // Step 33: If init is not empty, then:
        if !init.is_empty {
            // Step 33.1: Let headers be a copy of this's headers and its
            // associated header list.
            let copied_pairs = headers.header_list().borrow().pairs().to_vec();

            // Step 33.2: If init["headers"] exists, then set headers to
            // init["headers"].
            // Step 33.3: Empty this's headers's header list.
            headers.header_list().borrow_mut().clear();

            // Step 33.4: If headers is a Headers object, then for each header
            // of its header list, append header to this's headers.
            // Step 33.5: Otherwise, fill this's headers with headers.
            match init.headers {
                Some(init_headers) => headers.fill(init_headers, ec)?,
                None => {
                    for (name, value) in copied_pairs {
                        headers.append(&name, &value, ec)?;
                    }
                }
            }
        }

        // Step 34: Let inputBody be input's request's body if input is a
        // Request object; otherwise null.
        let input_request = input_body;
        let input_body = input_request
            .as_ref()
            .and_then(|input| input.body.body_state());

        // Step 35: If either init["body"] exists and is non-null or inputBody
        // is non-null, and request's method is `GET` or `HEAD`, then throw a
        // TypeError.
        let init_body_is_non_null = matches!(init.body, Some(Some(_)));
        if (init_body_is_non_null || input_body.is_some())
            && matches!(request.borrow().method.as_str(), "GET" | "HEAD")
        {
            return Err(ec.new_type_error("Request with GET/HEAD method cannot have body"));
        }

        // Step 36: Let initBody be null.
        let mut init_body = None;

        // Step 37: If init["body"] exists and is non-null, then:
        if let Some(Some(body_init)) = init.body {
            // Step 37.1: Let bodyWithType be the result of extracting
            // init["body"], with keepalive set to request's keepalive.
            let body_with_type = extract(body_init);

            // Step 37.2: Set initBody to bodyWithType's body.
            init_body = Some(body_with_type.body);

            // Step 37.3: Let type be bodyWithType's type.
            // Step 37.4: If type is non-null and this's headers's header list
            // does not contain `Content-Type`, then append (`Content-Type`,
            // type) to this's headers.
            if let Some(type_) = body_with_type.type_
                && !headers.header_list().borrow().contains("Content-Type")
            {
                headers.append("Content-Type", &type_, ec)?;
            }
        }

        // Step 38: Let inputOrInitBody be initBody if it is non-null;
        // otherwise inputBody.
        let init_body_is_null = init_body.is_none();
        let mut input_or_init_body = init_body.or(input_body);

        // Step 39: If inputOrInitBody is non-null and inputOrInitBody's
        // source is null, then:
        // Step 39.1: If initBody is non-null and init["duplex"] does not
        // exist, then throw a TypeError.
        // Step 39.2: If this's request's mode is neither "same-origin" nor
        // "cors", then throw a TypeError.
        // Step 39.3: Set this's request's use-CORS-preflight flag.
        // Note: Every body has a source: bodies are extracted from in-memory
        // byte sequences.
        // Step 40: Let finalBody be inputOrInitBody.
        // Step 41: If initBody is null and inputBody is non-null, then:
        if let Some(input) = &input_request
            && input.body.body_state().is_some()
            && let Some(body) = input_or_init_body.as_mut()
        {
            // Step 41.1: If input is unusable, then throw a TypeError.
            if init_body_is_null && input.body.unusable(ec) {
                return Err(ec.new_type_error(
                    "Cannot construct a Request with a Request object that has already been used",
                ));
            }

            // Step 41.2: Set finalBody to the result of creating a proxy for
            // inputBody.
            // Note: The proxy reads the input's stream, which disturbs it; the
            // new body holds a copy of the source bytes.  The input is disturbed
            // whenever its body is non-null, also when init["body"] replaces it,
            // as the request-disturbed WPT tests expect.
            input.body.mark_disturbed(ec);
            body.disturbed = false;
        }

        // Step 42: Set this's request's body to finalBody.
        if let Some(duplex) = init.duplex {
            request.borrow_mut().duplex = duplex;
        }
        Ok(Self {
            request,
            headers,
            signal: this_signal,
            body: BodyMixin::new(input_or_init_body, ec),
        })
    }

    pub(crate) fn body_mixin(&self) -> &BodyMixin {
        &self.body
    }

    pub(crate) fn headers(&self) -> &Headers {
        &self.headers
    }

    pub(crate) fn signal(&self) -> &AbortSignal {
        &self.signal
    }

    pub(crate) fn request(&self) -> InternalRequest {
        self.request.borrow().clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-method>
    pub(crate) fn method(&self) -> String {
        // The method getter steps are to return this's request's method.
        self.request.borrow().method.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-url>
    pub(crate) fn url(&self) -> String {
        // The url getter steps are to return this's request's URL,
        // serialized.
        self.request.borrow().url.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-destination>
    pub(crate) fn destination(&self) -> String {
        // The destination getter are to return this's request's destination.
        String::new()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-referrer>
    pub(crate) fn referrer(&self) -> String {
        // Step 1: If this's request's referrer is "no-referrer", then return
        // the empty string.
        let referrer = self.request.borrow().referrer.clone();
        if referrer == "no-referrer" {
            return String::new();
        }

        // Step 2: If this's request's referrer is "client", then return
        // "about:client".
        if referrer == "client" {
            return String::from("about:client");
        }

        // Step 3: Return this's request's referrer, serialized.
        referrer
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-referrerpolicy>
    pub(crate) fn referrer_policy(&self) -> String {
        // The referrerPolicy getter steps are to return this's request's
        // referrer policy.
        self.request.borrow().referrer_policy.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-mode>
    pub(crate) fn mode(&self) -> String {
        // The mode getter steps are to return this's request's mode.
        self.request.borrow().mode.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-credentials>
    pub(crate) fn credentials(&self) -> String {
        // The credentials getter steps are to return this's request's
        // credentials mode.
        self.request.borrow().credentials_mode.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-cache>
    pub(crate) fn cache(&self) -> String {
        // The cache getter steps are to return this's request's cache mode.
        self.request.borrow().cache_mode.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-redirect>
    pub(crate) fn redirect(&self) -> String {
        // The redirect getter steps are to return this's request's redirect
        // mode.
        self.request.borrow().redirect_mode.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-integrity>
    pub(crate) fn integrity(&self) -> String {
        // The integrity getter steps are to return this's request's integrity
        // metadata.
        self.request.borrow().integrity_metadata.clone()
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-keepalive>
    pub(crate) fn keepalive(&self) -> bool {
        // The keepalive getter steps are to return this's request's
        // keepalive.
        self.request.borrow().keepalive
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-isreloadnavigation>
    pub(crate) fn is_reload_navigation(&self) -> bool {
        // The isReloadNavigation getter steps are to return true if this's
        // request's reload-navigation flag is set; otherwise false.
        false
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-ishistorynavigation>
    pub(crate) fn is_history_navigation(&self) -> bool {
        // The isHistoryNavigation getter steps are to return true if this's
        // request's history-navigation flag is set; otherwise false.
        false
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-duplex>
    pub(crate) fn duplex(&self) -> String {
        // The duplex getter steps are to return "half".
        String::from("half")
    }

    /// <https://fetch.spec.whatwg.org/#dom-request-clone>
    pub(crate) fn clone_method(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: If this is unusable, then throw a TypeError.
        if self.body.unusable(ec) {
            return Err(ec.new_type_error("Request body is already used"));
        }

        // Step 2: Let clonedRequest be the result of cloning this's request.
        let mut cloned_request = self.request.borrow().clone();
        let copied_header_list = cloned_request.header_list.borrow().clone();
        cloned_request.header_list = Rc::new(RefCell::new(copied_header_list));
        let cloned_body = self.body.clone_body();

        // Step 3: Assert: this's signal is non-null.
        // Step 4: Let clonedSignal be the result of creating a dependent abort
        // signal from « this's signal », using AbortSignal and this's relevant
        // realm.
        let cloned_signal = create_abort_signal(AbortSignal::new(ec), ec)?;
        initialize_dependent_abort_signal(&cloned_signal, std::slice::from_ref(&self.signal), ec);

        // Step 5: Let clonedRequestObject be the result of creating a Request
        // object, given clonedRequest, this's headers's guard, clonedSignal
        // and this's relevant realm.
        let header_list = Rc::clone(&cloned_request.header_list);
        let headers = Headers::new(header_list, self.headers.guard());
        let headers_object = create_interface_instance::<Types, Headers>(headers, ec)?;
        let headers = ec
            .with_object_any(&headers_object)
            .and_then(|data| data.downcast_ref::<Headers>().cloned())
            .ok_or_else(|| ec.new_type_error("Headers object has no platform data"))?;

        // Step 6: Return clonedRequestObject.
        Ok(Self {
            request: Rc::new(RefCell::new(cloned_request)),
            headers,
            signal: cloned_signal,
            body: BodyMixin::new(cloned_body, ec),
        })
    }
}
