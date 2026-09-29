use js_engine::records::PromiseResolvers;
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::dom::AbortAlgorithm;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::rejected_promise;

use super::body::Body;
use super::request::{Request, RequestInfo, RequestInit};
use super::response::{InternalResponse, Response, ResponseType};
use super::{HeaderList, HeadersGuard, is_null_body_status};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// The parts of a request the net process receives (the fetch() method's
/// step 12 hands them to fetch).
#[derive(Debug, Clone)]
pub(crate) struct FetchRequestSnapshot {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) header_list: Vec<(String, String)>,
    pub(crate) body: Option<Vec<u8>>,
}

/// A response the net process delivered for a fetch (the fetch() method's
/// processResponse argument).
#[derive(Debug, Clone)]
pub(crate) struct FetchResponseData {
    pub(crate) final_url: String,
    pub(crate) status: u16,
    pub(crate) status_text: String,
    pub(crate) header_list: Vec<(String, String)>,
    pub(crate) content_type: String,
    pub(crate) body: Vec<u8>,
}

/// One call to fetch() between its start and the settlement of its promise
/// (the fetch() method's step 10 to step 13).
#[gc_struct]
pub(crate) struct PendingFetch {
    #[ignore_trace]
    pub(crate) fetch_id: u64,

    #[ignore_trace]
    pub(crate) request: FetchRequestSnapshot,

    /// The request has been handed to the net process.
    #[ignore_trace]
    pub(crate) dispatched: bool,

    /// <https://fetch.spec.whatwg.org/#dom-global-fetch>
    pub(crate) resolvers: PromiseResolvers<Types>,

    /// The request object of the fetch, kept so its signal's abort
    /// algorithm finds the fetch (step 11.3, "abort fetch").
    pub(crate) request_object: Request,
}

/// <https://fetch.spec.whatwg.org/#dom-global-fetch>
pub(crate) fn fetch(
    input: RequestInfo,
    init: RequestInit,
    base_url: Option<&str>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let p be a new promise.
    let (promise, resolvers) = ec.new_promise_pending()?;
    let promise = <Types as JsTypes>::value_as_object(&promise)
        .ok_or_else(|| ec.new_type_error("the new promise is not an object"))?;

    // Step 2: Let requestObject be the result of invoking the initial value
    // of Request as constructor with input and init as arguments. If this
    // throws an exception, reject p with it and return p.
    let request_object = match Request::constructor(input, init, base_url, ec) {
        Ok(request_object) => request_object,
        Err(exception) => return rejected_promise(exception, ec),
    };
    let request_object_value = create_interface_instance::<Types, Request>(request_object, ec)?;
    let request_object = ec
        .with_object_any(&request_object_value)
        .and_then(|data| data.downcast_ref::<Request>().cloned())
        .ok_or_else(|| ec.new_type_error("Request object has no platform data"))?;

    // Step 3: Let request be requestObject's request.
    let request = request_object.request();

    // Step 4: If requestObject's signal is aborted, then:
    if request_object.signal().aborted_value(ec) {
        // Step 4.1: Abort the fetch() call with p, request, null, and
        // requestObject's signal's abort reason.
        let reason = request_object.signal().reason_value(ec);
        resolvers.reject(reason, ec)?;

        // Step 4.2: Return p.
        return Ok(promise);
    }

    // Step 5: Let globalObject be request's client's global object.
    // Step 6: If globalObject is a ServiceWorkerGlobalScope object, then set
    // request's service-workers mode to "none".
    // Step 7: Let responseObject be null.
    // Step 8: Let relevantRealm be this's relevant realm.
    // Step 9: Let locallyAborted be false.
    // Step 10: Let controller be null.
    let body = request_object.body_mixin().body_bytes();
    let mut header_list = request.header_list.borrow().pairs().to_vec();
    // HTTP-network-or-cache fetch, step 8.12: If httpRequest's method is
    // neither `GET` nor `HEAD`, then append a request `Origin` header for
    // httpRequest.
    // <https://fetch.spec.whatwg.org/#append-a-request-origin-header>
    if !matches!(request.method.as_str(), "GET" | "HEAD")
        && !header_list
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("origin"))
    {
        let origin = with_global_scope(ec, |global_scope, _ec| {
            Ok(global_scope
                .creation_url()
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|| String::from("null")))
        })?;
        header_list.push((String::from("Origin"), origin));
    }
    let snapshot = FetchRequestSnapshot {
        url: request.url.clone(),
        method: request.method.clone(),
        header_list,
        body,
    };

    // Step 11: Add the following abort steps to requestObject's signal:
    // Step 11.1: Set locallyAborted to true.
    // Step 11.2: Assert: controller is non-null.
    // Step 11.3: Abort controller with requestObject's signal's abort reason.
    // Step 11.4: Abort the fetch() call with p, request, responseObject, and
    // requestObject's signal's abort reason.
    // Step 12: Set controller to the result of calling fetch given request
    // and processResponse given response being these steps: ...
    // Note: The request goes to the net process once the running task
    // completes (`ContentProcess::dispatch_pending_fetches`); its response
    // or failure settles the promise through `GlobalScope::complete_fetch`.
    let fetch_id = with_global_scope(ec, |global_scope, ec| {
        let fetch_id = global_scope.next_fetch_id();
        global_scope.add_pending_fetch(
            PendingFetch {
                fetch_id,
                request: snapshot,
                dispatched: false,
                resolvers,
                request_object: request_object.clone(),
            },
            ec,
        );
        Ok(fetch_id)
    })?;
    request_object
        .signal()
        .add_abort_algorithm(AbortAlgorithm::Fetch { fetch_id }, ec);

    // Step 13: Return p.
    Ok(promise)
}

/// <https://fetch.spec.whatwg.org/#dom-global-fetch>
pub(crate) fn process_response(
    pending: PendingFetch,
    response: Option<FetchResponseData>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 12.1: If locallyAborted is true, then abort these steps.
    // Note: An aborted fetch is removed from the pending list by abort fetch,
    // so it never reaches these steps.
    // Step 12.2: If response's aborted flag is set, then:
    // Step 12.2.1: Let deserializedError be the result of deserialize a
    // serialized abort reason given controller's serialized abort reason and
    // relevantRealm.
    // Step 12.2.2: Abort the fetch() call with p, request, responseObject,
    // and deserializedError.
    // Step 12.2.3: Abort these steps.
    // Step 12.3: If response is a network error, then reject p with a
    // TypeError and abort these steps.
    let Some(response) = response else {
        let error = ec.new_type_error("Failed to fetch");
        pending.resolvers.reject(error, ec)?;
        return Ok(());
    };

    // Step 12.4: Set responseObject to the result of creating a Response
    // object, given response, "immutable", and relevantRealm.
    let mut header_list = HeaderList::new();
    for (name, value) in &response.header_list {
        header_list.append(name, value);
    }
    if !response.content_type.is_empty() && !header_list.contains("content-type") {
        header_list.append("content-type", &response.content_type);
    }
    let internal_response = InternalResponse {
        type_: ResponseType::Basic,
        url_list: vec![response.final_url.clone()],
        status: response.status,
        status_message: response.status_text,
        header_list: std::rc::Rc::new(std::cell::RefCell::new(header_list)),
        redirected: response.final_url != pending.request.url,
    };
    // HTTP fetch: a response to a `HEAD` or `CONNECT` request, or one whose
    // status is a null body status, has a null body.
    // <https://fetch.spec.whatwg.org/#http-network-fetch>
    let body = (!is_null_body_status(response.status)
        && !matches!(pending.request.method.as_str(), "HEAD" | "CONNECT"))
    .then_some(Body {
        source: response.body,
        disturbed: false,
    });
    let response_object = Response::create(internal_response, body, HeadersGuard::Immutable, ec)?;
    let response_value: JsValue = <Types as JsTypes>::value_from_object(
        create_interface_instance::<Types, Response>(response_object, ec)?,
    );

    // Step 12.5: Resolve p with responseObject.
    pending.resolvers.resolve(response_value, ec)?;
    Ok(())
}

/// <https://fetch.spec.whatwg.org/#abort-fetch>
pub(crate) fn abort_fetch(
    pending: PendingFetch,
    error: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: Reject promise with error.
    pending.resolvers.reject(error, ec)?;

    // Step 2: If request's body is non-null and is readable, then cancel
    // request's body with error.
    // Step 3: If responseObject is null, then return.
    // Step 4: Let response be responseObject's response.
    // Step 5: If response's body is non-null and is readable, then error
    // response's body with error.
    // Note: The response object is created only when the promise resolves,
    // so an abort never finds one.
    Ok(())
}
