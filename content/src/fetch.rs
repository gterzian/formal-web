//
// Event-loop side implementation.
// <https://fetch.spec.whatwg.org/>
//
// `fetch` starts the Fetch algorithm: it records the response continuation and
// runs the scheme dispatch for `data:` and network fetches. Request
// construction, client population, default headers, preloading, CORS,
// redirects, timing, and response handover are not implemented; the step notes
// in `fetch`, `main_fetch`, and `scheme_fetch` mark each gap.
//
use std::collections::HashSet;

use blitz_traits::net::{Body, Request};
use data_url::DataUrl;
use ipc::IpcSender;
use ipc_messages::content::{
    Command, DocumentFetchId, EmbedderSchemeFetchRequested, Event as ContentEvent, EventLoopId,
    FetchRequest as WireFetchRequest, FetchResponse as WireFetchResponse, NavigableId,
};
use ipc_messages::network::{Request as NetworkRequest, ResponseRecipient};
use log::error;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{LocalContentStateRef, PendingNetworkHandler};

/// The header list a fetch carries to the net process or to the embedder.
/// <https://fetch.spec.whatwg.org/#concept-request-header-list>
pub(crate) fn request_header_list(request: &Request) -> Vec<(String, String)> {
    let mut header_list: Vec<(String, String)> = request
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    // Note: blitz carries the content type of a request body outside the
    // header list; the net process and the embedder see one list, so it is
    // appended here.
    if let Some(content_type) = &request.content_type
        && !header_list
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        header_list.push((String::from("content-type"), content_type.clone()));
    }
    header_list
}

/// The request body as the string the content/net wire request carries.
/// Note: blitz models a request body as `Body`; the wire request has a single
/// string field, so form bodies are serialized and byte bodies are lossily
/// decoded.
fn request_body_string(body: &Body) -> String {
    match body {
        Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Body::Form(form) => serde_json::to_string(form).unwrap_or_default(),
        Body::Empty => String::new(),
    }
}

/// <https://fetch.spec.whatwg.org/#concept-header-list>
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HeaderList {
    /// <https://fetch.spec.whatwg.org/#concept-header-list>
    headers: Vec<(String, String)>,
}

impl HeaderList {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_pairs(headers: Vec<(String, String)>) -> Self {
        Self { headers }
    }

    pub(crate) fn into_pairs(self) -> Vec<(String, String)> {
        self.headers
    }
}

/// <https://fetch.spec.whatwg.org/#concept-request>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InternalFetchRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request-url>
    pub(crate) url: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-method>
    pub(crate) method: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-header-list>
    pub(crate) header_list: HeaderList,
    /// <https://fetch.spec.whatwg.org/#concept-request-body>
    pub(crate) body: String,
}

impl InternalFetchRequest {
    pub(crate) fn get(url: String) -> Self {
        Self {
            url,
            method: String::from("GET"),
            header_list: HeaderList::new(),
            body: String::new(),
        }
    }

    pub(crate) fn from_request(request: &Request) -> Self {
        Self {
            url: request.url.to_string(),
            method: request.method.to_string(),
            header_list: HeaderList::from_pairs(request_header_list(request)),
            body: request_body_string(&request.body),
        }
    }
}

/// The content-process state needed to start a fetch: the IPC endpoints that
/// answer it and the table holding the response continuation.
pub(crate) struct FetchContext<'a> {
    pub(crate) event_loop_id: EventLoopId,
    pub(crate) navigable_id: NavigableId,
    pub(crate) content_command_sender: &'a IpcSender<Command>,
    pub(crate) event_sender: &'a IpcSender<ContentEvent>,
    pub(crate) network_extension_sender: &'a IpcSender<NetworkRequest>,
    pub(crate) embedder_schemes: &'a HashSet<String>,
    pub(crate) local_state: &'a LocalContentStateRef,
}

/// <https://fetch.spec.whatwg.org/#concept-fetch>
pub(crate) fn fetch(
    context: &FetchContext<'_>,
    handler_id: DocumentFetchId,
    pending: PendingNetworkHandler,
    request: InternalFetchRequest,
) -> Result<(), String> {
    // Step 1: Assert: request’s mode is "navigate" or processEarlyHintsResponse is null.
    // Step 2: Let taskDestination be null.
    // Step 3: Let crossOriginIsolatedCapability be false.
    // Note: Request mode, early-hints callbacks, task destination, and
    // cross-origin isolation are not modeled. The response continuation is the
    // content event loop, which is also where `pending` is completed.
    // Step 4: Populate request from client given request.
    // Note: Request's client, origin, and policy container are not represented;
    // callers pass an absolute URL and method.
    // Step 5: If request’s client is non-null, then:
    // Step 5.1: Set taskDestination to request’s client’s global object.
    // Step 5.2: Set crossOriginIsolatedCapability to request’s client’s cross-origin isolated capability.
    // Note: Not modeled; see Steps 1-4.
    // Step 6: If useParallelQueue is true, then set taskDestination to the result of starting a new parallel queue.
    // Note: useParallelQueue is always false.
    // Step 7: Let timingInfo be a new fetch timing info whose start time and post-redirect start time are the coarsened shared current time given crossOriginIsolatedCapability, and render-blocking is set to request’s render-blocking.
    // Step 8: Let fetchParams be a new fetch params whose request is request, timing info is timingInfo, process request body chunk length is processRequestBodyChunkLength, process request end-of-body is processRequestEndOfBody, process early hints response is processEarlyHintsResponse, process response is processResponse, process response consume body is processResponseConsumeBody, process response end-of-body is processResponseEndOfBody, task destination is taskDestination, and cross-origin isolated capability is crossOriginIsolatedCapability.
    // Step 9: If request’s body is a byte sequence, then set request’s body to request’s body as a body.
    // Step 10: Run the WebDriver BiDi clone network request body steps with request.
    // Note: Fetch timing, the remaining fetch params fields, the Body union,
    // and the WebDriver BiDi clone steps are not modeled.
    // Step 11: If all of the following conditions are true:
    // Step 11.1: Assert: request’s origin is same origin with request’s client’s origin.
    // Step 11.2: Let onPreloadedResponseAvailable be an algorithm that runs the following step given a response response: set fetchParams’s preloaded response candidate to response.
    // Step 11.3: Let foundPreloadedResource be the result of invoking consume a preloaded resource for request’s client, given request’s URL, request’s destination, request’s mode, request’s credentials mode, request’s integrity metadata, and onPreloadedResponseAvailable.
    // Step 11.4: If foundPreloadedResource is true and fetchParams’s preloaded response candidate is null, then set fetchParams’s preloaded response candidate to "pending".
    // Note: Resource preloading is not implemented.
    // Step 12: If request’s header list does not contain `Accept`, then:
    // Step 12.1: Let value be `*/*`.
    // Step 12.2: If request’s initiator is "prefetch", then set value to the document `Accept` header value.
    // Step 12.3: Otherwise, the user agent should set value to the first matching statement, if any, switching on request’s destination:
    // Step 12.4: Append (`Accept`, value) to request’s header list.
    // Step 13: If request’s header list does not contain `Accept-Language` and request’s client is non-null, then:
    // Step 13.1: Let emulatedLanguage be the WebDriver BiDi emulated language for request’s client.
    // Step 13.2: If emulatedLanguage is non-null:
    // Step 13.2.1: Let encodedEmulatedLanguage be emulatedLanguage, isomorphic encoded.
    // Step 13.2.2: Append (`Accept-Language`, encodedEmulatedLanguage) to request’s header list.
    // Step 14: If request’s header list does not contain `Accept-Language`, then user agents should append (`Accept-Language`, an appropriate header value) to request’s header list.
    // Note: Default `Accept` and `Accept-Language` headers, and the emulated
    // language, are not implemented.
    // Step 15: If request’s internal priority is null, then use request’s priority, initiator, destination, and render-blocking in an implementation-defined manner to set request’s internal priority to an implementation-defined object.
    // Note: Request prioritization is not modeled.
    // Step 16: If request is a subresource request:
    // Step 16.1: Let record be a new fetch record whose request is request and controller is fetchParams’s controller.
    // Step 16.2: Append record to request’s client’s fetch group’s fetch records.
    // Note: Fetch-group bookkeeping (fetch records, controllers, abort) is not
    // modeled: content has no environment-settings-scoped fetch state to own it.
    // Step 17: Run main fetch given fetchParams.
    // Note: The response continuation is registered here; `pending` receives the
    // response through `Command::CompleteDocumentFetch` / `FailDocumentFetch`.
    let url = Url::parse(&request.url)
        .map_err(|error| format!("failed to parse fetch URL {:?}: {error}", request.url))?;
    context
        .local_state
        .borrow_mut()
        .pending_handlers
        .insert(handler_id, pending);
    let result = main_fetch(context, handler_id, request, &url);
    if result.is_err() {
        context
            .local_state
            .borrow_mut()
            .pending_handlers
            .remove(&handler_id);
    }
    // Step 18: Return fetchParams’s controller.
    // Note: The controller is not returned to callers; `FetchController` exists
    // but abort/terminate are not wired.
    result
}

/// <https://fetch.spec.whatwg.org/#main-fetch>
fn main_fetch(
    context: &FetchContext<'_>,
    handler_id: DocumentFetchId,
    request: InternalFetchRequest,
    url: &Url,
) -> Result<(), String> {
    // Step 1: Let request be fetchParams’s request.
    // Step 2: Let response be null.
    // Step 3: If request’s local-URLs-only flag is set and request’s current URL is not local, then set response to a network error.
    // Step 4: Run report Content Security Policy violations for request.
    // Step 5: Upgrade request to a potentially trustworthy URL, if appropriate.
    // Step 6: Upgrade a mixed content request to a potentially trustworthy URL, if appropriate.
    // Step 7: If should request be blocked due to a bad port, should fetching request be blocked as mixed content, should request be blocked by Content Security Policy, or should request be blocked by Integrity Policy Policy returns blocked, then set response to a network error.
    // Step 8: If request’s referrer policy is the empty string, then set request’s referrer policy to request’s policy container’s referrer policy.
    // Step 9: If request’s referrer is not "no-referrer", then set request’s referrer to the result of invoking determine request’s referrer.
    // Step 10: Set request’s current URL’s scheme to "https" if all of the following conditions are true:
    // Note: Steps 3-10 (local-URL restriction, CSP, URL upgrades, mixed
    // content, bad ports, referrer policy, and HSTS) are not implemented.
    // Step 11: If recursive is false, then run the remaining steps in parallel.
    // Note: This function starts the fetch and returns; for network and
    // embedder schemes the parallel steps run in net/the user agent, and the
    // response returns as a content command.
    // Step 12: If response is null, then set response to the result of running the steps corresponding to the first matching statement:
    // Step 12.1: Wait until fetchParams’s preloaded response candidate is not "pending".
    // Step 12.2: Assert: fetchParams’s preloaded response candidate is a response.
    // Step 12.3: Return fetchParams’s preloaded response candidate.
    // Step 12.4: Set request’s response tainting to "basic".
    // Step 12.5: Return the result of running override fetch given "scheme-fetch" and fetchParams.
    // Step 12.6: If request’s redirect mode is not "follow", then return a network error.
    // Step 12.7: Set request’s response tainting to "opaque".
    // Step 12.8: Return the result of running override fetch given "scheme-fetch" and fetchParams.
    // Step 12.9: Set request’s response tainting to "cors".
    // Step 12.10: Let corsWithPreflightResponse be the result of running override fetch given "http-fetch", fetchParams, and true.
    // Step 12.11: If corsWithPreflightResponse is a network error, then clear cache entries using request.
    // Step 12.12: Return corsWithPreflightResponse.
    // Step 12.13: Set request’s response tainting to "cors".
    // Step 12.14: Return the result of running override fetch given "http-fetch" and fetchParams.
    // Note: Request mode, response tainting, preloading, CORS, and redirect
    // handling are not modeled; the dispatch reduces to `scheme_fetch`.
    scheme_fetch(context, handler_id, request, url)
    // Step 13: If recursive is true, then return response.
    // Step 14: If response is not a network error and response is not a filtered response, then:
    // Step 15: Let internalResponse be response, if response is a network error; otherwise response’s internal response.
    // Step 16: If internalResponse’s URL list is empty, then set it to a clone of request’s URL list.
    // Step 17: Set internalResponse’s redirect taint to request’s redirect-taint.
    // Step 18: If request is a navigation request, then set internalResponse’s navigation timing allow values list to a clone of request’s navigation timing allow values list.
    // Step 19: If request’s timing allow failed flag is unset, then set internalResponse’s timing allow passed flag.
    // Step 20: If response is not a network error and any of the following returns blocked ... then set response and internalResponse to a network error.
    // Step 21: If response’s type is "opaque", internalResponse’s status is a range status, internalResponse’s range-requested flag is set, and request’s header list does not contain `Range`, then set response and internalResponse to a network error.
    // Step 22: If response is not a network error and either request’s method is `HEAD` or `CONNECT`, or internalResponse’s status is a null body status, set internalResponse’s body to null and disregard any enqueuing toward it (if any).
    // Step 23: If request’s integrity metadata is not the empty string, then:
    // Step 24: Otherwise, run fetch response handover given fetchParams and response.
    // Note: Steps 13-24 (filtered responses, response metadata, mixed content /
    // CSP / MIME blocking, integrity metadata, and fetch response handover)
    // are not implemented. `ContentProcess::complete_document_fetch` delivers
    // the response body to the caller.
}

/// <https://fetch.spec.whatwg.org/#scheme-fetch>
fn scheme_fetch(
    context: &FetchContext<'_>,
    handler_id: DocumentFetchId,
    request: InternalFetchRequest,
    url: &Url,
) -> Result<(), String> {
    // Step 1: If fetchParams is canceled, then return the appropriate network error for fetchParams.
    // Note: Fetch cancellation (`FetchController`) is not wired.
    // Step 2: Let request be fetchParams’s request.
    // Step 3: Switch on request’s current URL’s scheme and run the associated steps:
    match url.scheme() {
        "data" => {
            // Step 3.16: Let dataURLStruct be the result of running the data: URL processor on request’s current URL.
            let data_url_struct = data_url_processor(url);
            // Step 3.17: If dataURLStruct is failure, then return a network error.
            let response = match data_url_struct {
                Err(error) => {
                    error!("failed to process a data URL: {error}");
                    Command::FailDocumentFetch { handler_id }
                }
                Ok((mime_type, body)) => {
                    // Step 3.18: Let mimeType be dataURLStruct’s MIME type, serialized.
                    // Step 3.19: Return a new response whose status message is `OK`, header list is « (`Content-Type`, mimeType) », and body is dataURLStruct’s body as a body.
                    Command::CompleteDocumentFetch {
                        handler_id,
                        response: WireFetchResponse {
                            final_url: url.to_string(),
                            status: 200,
                            content_type: mime_type,
                            body,
                        },
                    }
                }
            };
            context
                .content_command_sender
                .send(response)
                .map_err(|error| format!("failed to deliver a data URL response: {error}"))
        }
        scheme if context.embedder_schemes.contains(scheme) => {
            // Note: Embedder-served schemes are answered by the user agent ahead
            // of the spec's scheme switch; this is a formal-web extension.
            context
                .event_sender
                .send(ContentEvent::EmbedderSchemeFetchRequested(
                    EmbedderSchemeFetchRequested {
                        navigable_id: context.navigable_id,
                        handler_id,
                        request: wire_request(handler_id, request),
                    },
                ))
                .map_err(|error| {
                    format!("failed to send an embedder-scheme fetch to the user agent: {error}")
                })
        }
        _ => {
            // Note: The "HTTP(S) scheme" branch returns the result of running
            // HTTP fetch given fetchParams; HTTP fetch runs in the net process.
            // Step 4: Return a network error.
            // Note: `file:` and the other non-data schemes are also sent to net,
            // which produces the network error for schemes it cannot handle.
            let network_request = NetworkRequest::Fetch {
                event_loop_id: context.event_loop_id,
                request_id: Uuid::new_v4(),
                request: wire_request(handler_id, request),
                reply_to: ResponseRecipient::ContentProcess {
                    content_command_sender: context.content_command_sender.clone(),
                    handler_id,
                },
            };
            context
                .network_extension_sender
                .send(network_request)
                .map_err(|error| format!("failed to send fetch request to net: {error}"))
        }
    }
}

/// <https://fetch.spec.whatwg.org/#data-url-processor>
fn data_url_processor(data_url: &Url) -> Result<(String, Vec<u8>), String> {
    // Step 1: Assert: dataURL’s scheme is "data".
    assert_eq!(data_url.scheme(), "data");
    // Step 2: Let input be the result of running the URL serializer on dataURL with exclude fragment set to true.
    // Step 3: Remove the leading "data:" from input.
    // Step 4: Let position point at the start of input.
    // Step 5: Let mimeType be the result of collecting a sequence of code points that are not equal to U+002C (,), given position.
    // Step 6: Strip leading and trailing ASCII whitespace from mimeType.
    // Step 7: If position is past the end of input, then return failure.
    // Step 8: Advance position by 1.
    // Step 9: Let encodedBody be the remainder of input.
    // Step 10: Let body be the percent-decoding of encodedBody.
    // Step 11: If mimeType ends with U+003B (;), followed by zero or more U+0020 SPACE, followed by an ASCII case-insensitive match for "base64", then:
    // Step 11.1: Let stringBody be the isomorphic decode of body.
    // Step 11.2: Set body to the forgiving-base64 decode of stringBody.
    // Step 11.3: If body is failure, then return failure.
    // Step 11.4: Remove the last 6 code points from mimeType.
    // Step 11.5: Remove trailing U+0020 SPACE code points from mimeType, if any.
    // Step 11.6: Remove the last U+003B (;) from mimeType.
    // Step 12: If mimeType starts with ";", then prepend "text/plain" to mimeType.
    // Step 13: Let mimeTypeRecord be the result of parsing mimeType.
    // Step 14: If mimeTypeRecord is failure, then set mimeTypeRecord to text/plain;charset=US-ASCII.
    // Step 15: Return a new data: URL struct whose MIME type is mimeTypeRecord and body is body.
    // Note: Steps 2-15 are implemented by the `data_url` crate.
    let data_url = DataUrl::process(data_url.as_str())
        .map_err(|error| format!("invalid data URL: {error}"))?;
    let mime_type = data_url.mime_type().to_string();
    let (body, _fragment) = data_url
        .decode_to_vec()
        .map_err(|error| format!("failed to decode data URL body: {error}"))?;
    Ok((mime_type, body))
}

/// Marshals an [`InternalFetchRequest`] into the content/net wire request.
/// The wire shape carries no done or keepalive flags yet.
fn wire_request(handler_id: DocumentFetchId, request: InternalFetchRequest) -> WireFetchRequest {
    WireFetchRequest {
        handler_id,
        url: request.url,
        method: request.method,
        header_list: request.header_list.into_pairs(),
        body: request.body,
    }
}
