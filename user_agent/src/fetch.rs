use crossbeam_channel::{Receiver, Sender, select, unbounded};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_channel::router::ROUTER;
use ipc_messages::content::{
    DocumentFetchId, EventLoopId, FetchRequest as ContentFetchRequest,
    FetchResponse as ContentFetchResponse, HeaderList as ContentHeaderList, NavigationFetchId,
};
use ipc_messages::network::{
    Bootstrap as NetworkBootstrap, Request as NetworkRequest, Response as NetworkResponse,
};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};
use verification::TraceSender;

use crate::{UserAgentCommand, sidecar_executable_path};

/// graceful shutdown of the network process owned by the fetch worker.
const FETCH_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// <https://fetch.spec.whatwg.org/#concept-header-list>
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HeaderList {
    /// <https://fetch.spec.whatwg.org/#concept-header-list>
    headers: Vec<(String, String)>,
}

impl HeaderList {
    fn from_content_header_list(header_list: ContentHeaderList) -> Self {
        Self {
            headers: header_list.headers,
        }
    }

    fn to_content_header_list(&self) -> ContentHeaderList {
        // Note: Formal-web plumbing converts the fetch worker's header-list storage back into the
        // content IPC header-list transport shape.
        ContentHeaderList {
            headers: self.headers.clone(),
        }
    }

    /// <https://fetch.spec.whatwg.org/#concept-header-list-get>
    fn get(&self, name: &str) -> Option<String> {
        // Step 1: "If list does not contain name, then return null."
        let values = self
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

/// <https://fetch.spec.whatwg.org/#concept-request>
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InternalFetchRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request-url>
    url: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-method>
    method: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-header-list>
    header_list: HeaderList,
    /// <https://fetch.spec.whatwg.org/#concept-request-body>
    ///
    /// Note: This keeps the existing IPC body string transport instead of modeling a Fetch body
    /// stream.
    body: String,
    /// <https://fetch.spec.whatwg.org/#done-flag>
    done: bool,
    /// <https://fetch.spec.whatwg.org/#request-keepalive-flag>
    keepalive: bool,
}

impl InternalFetchRequest {
    fn from_content_fetch_request(request: ContentFetchRequest) -> Self {
        let ContentFetchRequest {
            handler_id: _,
            url,
            method,
            header_list,
            body,
        } = request;

        Self {
            url,
            method,
            header_list: HeaderList::from_content_header_list(header_list),
            body,
            done: false,
            keepalive: false,
        }
    }

    fn to_content_fetch_request(&self, handler_id: DocumentFetchId) -> ContentFetchRequest {
        // Note: Formal-web plumbing converts the fetch worker's request snapshot into the content
        // IPC request shape consumed by the net process.
        ContentFetchRequest {
            handler_id,
            url: self.url.clone(),
            method: self.method.clone(),
            header_list: self.header_list.to_content_header_list(),
            body: self.body.clone(),
        }
    }

    fn mark_done(&mut self) {
        self.done = true;
    }
}

/// <https://fetch.spec.whatwg.org/#concept-response>
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InternalFetchResponse {
    /// <https://fetch.spec.whatwg.org/#concept-response-url-list>
    url_list: Vec<String>,
    /// <https://fetch.spec.whatwg.org/#concept-response-status>
    status: u16,
    /// <https://fetch.spec.whatwg.org/#concept-response-status-message>
    status_text: String,
    /// <https://fetch.spec.whatwg.org/#concept-response-header-list>
    header_list: HeaderList,
    // Note: Formal-web's current content IPC exposes `content_type` as a separate convenience
    // field. The spec-shaped value above is `header_list`; this field is preserved only to round
    // trip the existing `FetchResponse` transport without changing behavior.
    content_type: String,
    /// <https://fetch.spec.whatwg.org/#concept-response-body>
    ///
    /// Note: This keeps the existing buffered byte transport and does not yet distinguish a null
    /// body from an empty body.
    body: Vec<u8>,
}

impl InternalFetchResponse {
    fn from_content_fetch_response(response: ContentFetchResponse) -> Self {
        let header_list = HeaderList::from_content_header_list(response.header_list);
        let content_type = if response.content_type.is_empty() {
            header_list.get("content-type").unwrap_or_default()
        } else {
            response.content_type
        };
        Self {
            url_list: if response.url_list.is_empty() {
                vec![response.final_url]
            } else {
                response.url_list
            },
            status: response.status,
            status_text: response.status_text,
            header_list,
            content_type,
            body: response.body,
        }
    }

    fn into_content_fetch_response(self) -> ContentFetchResponse {
        // Note: Formal-web plumbing converts the fetch worker's response snapshot into the content
        // IPC response shape used by document and navigation continuations.
        let content_type = if self.content_type.is_empty() {
            self.header_list.get("content-type").unwrap_or_default()
        } else {
            self.content_type
        };
        ContentFetchResponse {
            final_url: self.url_list.last().cloned().unwrap_or_default(),
            url_list: self.url_list,
            status: self.status,
            status_text: self.status_text,
            header_list: self.header_list.to_content_header_list(),
            content_type,
            body: self.body,
        }
    }
}

/// <https://fetch.spec.whatwg.org/#fetch-controller-state>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FetchControllerState {
    #[default]
    Ongoing,
    Terminated,
    Aborted,
}

/// <https://fetch.spec.whatwg.org/#fetch-controller>
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FetchController {
    /// <https://fetch.spec.whatwg.org/#fetch-controller-state>
    state: FetchControllerState,
    /// <https://fetch.spec.whatwg.org/#fetch-controller-serialized-abort-reason>
    serialized_abort_reason: Option<String>,
}

impl FetchController {
    fn new() -> Self {
        Self::default()
    }

    /// <https://fetch.spec.whatwg.org/#fetch-controller-abort>
    // TODO: Content cannot initiate fetch aborts yet, so no production path calls this algorithm.
    // Note: Structured abort reasons are not carried across content, user-agent, and net yet.
    #[allow(dead_code)]
    pub(crate) fn abort(&mut self, error: Option<String>) {
        // Step 1: "Set controller's state to \"aborted\"."
        self.state = FetchControllerState::Aborted;
        // Step 2: "Let fallbackError be an \"AbortError\" DOMException."
        let fallback_error = String::from("AbortError");
        // Step 3: "Set error to fallbackError if it is not given."
        let error = error.unwrap_or_else(|| fallback_error.clone());
        // TODO: Step 4: "Let serializedError be StructuredSerialize(error)."
        // Note: This stores the serialized abort reason as a string placeholder until structured
        // clone / DOMException-shaped values are available across this boundary.
        let serialized_error = error;
        // Step 5: "Set controller's serialized abort reason to serializedError."
        self.serialized_abort_reason = Some(serialized_error);
    }

    /// <https://fetch.spec.whatwg.org/#fetch-controller-terminate>
    pub(crate) fn terminate(&mut self) {
        // Step 1: "Set controller's state to \"terminated\"."
        self.state = FetchControllerState::Terminated;
    }
}

/// <https://fetch.spec.whatwg.org/#fetch-params>
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FetchParams {
    /// <https://fetch.spec.whatwg.org/#fetch-params-request>
    request: InternalFetchRequest,
    /// <https://fetch.spec.whatwg.org/#fetch-params-controller>
    controller: FetchController,
}

impl FetchParams {
    fn new(request: InternalFetchRequest) -> Self {
        Self {
            request,
            controller: FetchController::new(),
        }
    }

    /// <https://fetch.spec.whatwg.org/#fetch-params-aborted>
    pub(crate) fn is_aborted(&self) -> bool {
        self.controller.state == FetchControllerState::Aborted
    }

    /// <https://fetch.spec.whatwg.org/#fetch-params-canceled>
    pub(crate) fn is_canceled(&self) -> bool {
        matches!(
            self.controller.state,
            FetchControllerState::Aborted | FetchControllerState::Terminated
        )
    }
}

/// Commands that the user-agent thread can send into the dedicated fetch worker.
pub enum FetchCommand {
    StartDocumentFetch {
        event_loop_id: EventLoopId,
        request: ContentFetchRequest,
    },
    StartNavigationFetch {
        fetch_id: NavigationFetchId,
        request: ContentFetchRequest,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// a pending document fetch that must resume one event loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingDocumentFetch {
    /// Event loop that should receive the fetch completion/failure.
    pub event_loop_id: EventLoopId,
    /// Content-side handler id for the document fetch request.
    pub handler_id: DocumentFetchId,
}

/// a pending navigation fetch keyed by the user-agent's fetch id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingNavigationFetch {
    /// Formal-web navigation continuation id, not a Fetch Standard request/controller field.
    pub fetch_id: NavigationFetchId,
}

/// distinguishing document and navigation fetch continuations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingFetch {
    Document(PendingDocumentFetch),
    Navigation(PendingNavigationFetch),
}

/// <https://fetch.spec.whatwg.org/#fetch-record>
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FetchRecord {
    /// <https://fetch.spec.whatwg.org/#concept-fetch-record-request>
    request: InternalFetchRequest,
    /// <https://fetch.spec.whatwg.org/#concept-fetch-record-fetch>
    controller: Option<FetchController>,
    // Note: Formal-web continuation resumed when the network process completes this fetch.
    continuation: PendingFetch,
}

impl FetchRecord {
    fn navigation_transport_handler_id() -> DocumentFetchId {
        // Note: Navigation fetches are keyed by `NavigationFetchId` in the user agent, and the net
        // process ignores `FetchRequest.handler_id`. This placeholder exists because the current
        // network path reuses the content document-fetch IPC request shape for all network fetches.
        // The stable value makes the intentionally-unused field obvious.
        DocumentFetchId::from_u128(0)
    }

    fn from_content_fetch_request(
        request: ContentFetchRequest,
        continuation: PendingFetch,
    ) -> Self {
        let params = FetchParams::new(InternalFetchRequest::from_content_fetch_request(request));
        Self::from_fetch_params(params, continuation)
    }

    fn from_fetch_params(params: FetchParams, continuation: PendingFetch) -> Self {
        // Note: `FetchParams` is the setup-time bookkeeping struct from the Fetch algorithm. A
        // fetch group's `FetchRecord` stores the request/controller pair after the fetch is in the
        // group's active fetch record list, plus formal-web continuation plumbing.
        Self {
            request: params.request,
            controller: Some(params.controller),
            continuation,
        }
    }

    fn network_request(&self) -> ContentFetchRequest {
        let handler_id = match &self.continuation {
            PendingFetch::Document(pending_fetch) => pending_fetch.handler_id,
            PendingFetch::Navigation(_pending_fetch) => Self::navigation_transport_handler_id(),
        };
        self.request.to_content_fetch_request(handler_id)
    }

    fn fetch_params_snapshot(&self) -> Option<FetchParams> {
        // Note: This reconstructs a read-only FetchParams view so predicate helpers can share the
        // exact Fetch Standard definitions without storing a second copy of request/controller.
        self.controller.clone().map(|controller| FetchParams {
            request: self.request.clone(),
            controller,
        })
    }

    fn is_aborted(&self) -> bool {
        self.fetch_params_snapshot()
            .is_some_and(|params| params.is_aborted())
    }

    fn is_canceled(&self) -> bool {
        self.fetch_params_snapshot()
            .is_some_and(|params| params.is_canceled())
    }
}

// Note: Placeholder item for the fetch group's deferred fetch records list. The actual
// `deferred fetch record` fields are not modeled because deferred fetch processing is not
// implemented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeferredFetchRecord;

/// <https://fetch.spec.whatwg.org/#concept-fetch-group>
// Note: formal-web keeps one fetch group on the fetch worker. The Fetch Standard associates a
// fetch group with an environment settings object, but content does not expose environment-scoped
// Fetch API state yet.
#[derive(Debug, Default)]
pub(crate) struct FetchGroup {
    /// <https://fetch.spec.whatwg.org/#concept-fetch-record>
    fetch_records: HashMap<u64, FetchRecord>,
    /// <https://fetch.spec.whatwg.org/#fetch-group-deferred-fetch-records>
    pub(crate) deferred_fetch_records: Vec<DeferredFetchRecord>,
}

impl FetchGroup {
    fn new() -> Self {
        Self::default()
    }

    fn insert_fetch_record(&mut self, request_id: u64, record: FetchRecord) {
        self.fetch_records.insert(request_id, record);
    }

    fn remove_fetch_record(&mut self, request_id: u64) -> Option<FetchRecord> {
        self.fetch_records.remove(&request_id)
    }

    fn drain_fetch_records(&mut self) -> Vec<FetchRecord> {
        self.fetch_records
            .drain()
            .map(|(_request_id, record)| record)
            .collect()
    }

    /// <https://fetch.spec.whatwg.org/#concept-fetch-group-terminate>
    pub(crate) fn terminate(&mut self) {
        // Step 1: "For each fetch record record of fetchGroup's fetch records, if record's
        // controller is non-null and record's request's done flag is unset and keepalive is
        // false, terminate record's controller."
        for record in self.fetch_records.values_mut() {
            if let Some(controller) = record.controller.as_mut() {
                if !record.request.done && !record.request.keepalive {
                    controller.terminate();
                }
            }
        }
        // TODO: Step 2: "Process deferred fetches for fetchGroup."
        let _has_deferred_fetch_records = !self.deferred_fetch_records.is_empty();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FetchCompletion {
    DocumentCompleted {
        event_loop_id: EventLoopId,
        handler_id: DocumentFetchId,
        response: ContentFetchResponse,
    },
    DocumentFailed {
        event_loop_id: EventLoopId,
        handler_id: DocumentFetchId,
    },
    NavigationCompleted {
        fetch_id: NavigationFetchId,
        response: ContentFetchResponse,
    },
    NavigationFailed {
        fetch_id: NavigationFetchId,
    },
    Ignored,
}

// Note: Formal-web continuation plumbing, not a Fetch Standard algorithm. This maps an active
// fetch record plus a net-process response back to the document or navigation caller that started
// the fetch.
fn completion_for_network_result(
    mut record: FetchRecord,
    result: Result<ContentFetchResponse, String>,
) -> FetchCompletion {
    if record.is_aborted() {
        if let Err(error) = result {
            eprintln!("ignored aborted fetch failure: {error}");
        }
        return FetchCompletion::Ignored;
    }

    if record.is_canceled() {
        if let Err(error) = result {
            eprintln!("ignored canceled fetch failure: {error}");
        }
        return FetchCompletion::Ignored;
    }

    match (record.continuation, result) {
        (PendingFetch::Document(pending_fetch), Ok(fetch_response)) => {
            record.request.mark_done();
            let response = InternalFetchResponse::from_content_fetch_response(fetch_response)
                .into_content_fetch_response();
            FetchCompletion::DocumentCompleted {
                event_loop_id: pending_fetch.event_loop_id,
                handler_id: pending_fetch.handler_id,
                response,
            }
        }
        (PendingFetch::Document(pending_fetch), Err(_error)) => FetchCompletion::DocumentFailed {
            event_loop_id: pending_fetch.event_loop_id,
            handler_id: pending_fetch.handler_id,
        },
        (PendingFetch::Navigation(pending_fetch), Ok(fetch_response)) => {
            record.request.mark_done();
            let response = InternalFetchResponse::from_content_fetch_response(fetch_response)
                .into_content_fetch_response();
            FetchCompletion::NavigationCompleted {
                fetch_id: pending_fetch.fetch_id,
                response,
            }
        }
        (PendingFetch::Navigation(pending_fetch), Err(_error)) => FetchCompletion::NavigationFailed {
            fetch_id: pending_fetch.fetch_id,
        },
    }
}

/// Stateful owner of the network-facing half of HTML's parallel fetch work plus document-fetch
/// plumbing that resumes event-loop-local fetch handlers.
struct FetchWorker {
    /// Receiver for user-agent fetch commands.
    command_receiver: Receiver<FetchCommand>,
    /// Sender back into the user-agent thread for navigation/document fetch completions.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// IPC sender to the dedicated network process.
    network_request_sender: IpcSender<NetworkRequest>,
    /// IPC receiver for network process responses.
    network_event_receiver: Receiver<Result<NetworkResponse, String>>,
    /// Child process handle for the network process.
    child: Option<Child>,
    /// Transport-local request id allocator for the network IPC bridge.
    next_request_id: u64,
    /// <https://fetch.spec.whatwg.org/#concept-fetch-group>
    fetch_group: FetchGroup,
    /// Deferred shutdown reply completed after the network process exits.
    shutdown_reply: Option<Sender<Result<(), String>>>,
}

/// waiting on the network process during shutdown.
fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Ok(true),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!("failed to poll network process exit: {error}"));
            }
        }
    }
}

/// gracefully shutting down the network process owned by the fetch worker.
fn finish_shutdown(mut child: Option<Child>) {
    if let Some(child) = child.as_mut() {
        match wait_for_child_exit(child, FETCH_SHUTDOWN_GRACE_TIMEOUT) {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = child.kill() {
                    eprintln!("failed to kill network process: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("failed to wait for network process exit: {error}");
                }
            }
            Err(error) => {
                eprintln!("fetch shutdown poll error: {error}");
            }
        }
    }
}

/// bootstrapping the dedicated network process used by
/// fetch-backed navigation and document fetch continuations.
pub fn start_network_bridge(
    trace_sender: Option<TraceSender>,
) -> Result<
    (
        IpcSender<NetworkRequest>,
        Receiver<Result<NetworkResponse, String>>,
        Child,
    ),
    String,
> {
    let (server, token) = IpcOneShotServer::<NetworkBootstrap>::new()
        .map_err(|error| format!("failed to create network IPC one-shot server: {error}"))?;

    let executable_path = sidecar_executable_path("formal-web-net")?;

    let mut child_process = Command::new(&executable_path);
    #[cfg(unix)]
    child_process.arg0("formal-web-net");
    child_process.arg("--net-token").arg(&token);

    let child = child_process
        .spawn()
        .map_err(|error| format!("failed to start network process: {error}"))?;
    let (_receiver, bootstrap) = server
        .accept()
        .map_err(|error| format!("failed to accept network bootstrap: {error}"))?;

    bootstrap
        .request_sender
        .send(NetworkRequest::SetTraceSender(trace_sender))
        .map_err(|error| format!("failed to send trace sender to network process: {error}"))?;

    let (event_sender, event_receiver) = unbounded();
    ROUTER.add_typed_route(
        bootstrap.response_receiver,
        Box::new(move |message| {
            if let Err(error) = event_sender.send(
                message
                    .map_err(|error| format!("failed to decode network IPC response: {error}")),
            ) {
                eprintln!("failed to route network IPC response to fetch worker: {error}");
            }
        }),
    );

    Ok((bootstrap.request_sender, event_receiver, child))
}

impl FetchWorker {
    /// starting the fetch worker with its owned network process.
    fn new(
        command_receiver: Receiver<FetchCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
        trace_sender: Option<TraceSender>,
    ) -> Result<Self, String> {
        let (network_request_sender, network_event_receiver, child) =
            start_network_bridge(trace_sender)?;
        Ok(Self {
            command_receiver,
            user_agent_command_sender,
            network_request_sender,
            network_event_receiver,
            child: Some(child),
            next_request_id: 1,
            fetch_group: FetchGroup::new(),
            shutdown_reply: None,
        })
    }

    fn send_user_agent_command(&self, command: UserAgentCommand, operation: &str) {
        // Note: Formal-web plumbing logs failed cross-thread user-agent notifications before
        // dropping them so fetch failures keep their diagnostic path.
        if let Err(error) = self.user_agent_command_sender.send(command) {
            eprintln!("{operation}: {error}");
        }
    }

    /// failing every pending fetch if the network process stops before
    /// producing a response.
    fn fail_pending_fetches(&mut self) {
        // If the network bridge stops early, report every outstanding fetch back through the
        // same user-agent continuations that would have handled an ordinary network failure.
        for fetch_record in self.fetch_group.drain_fetch_records() {
            match fetch_record.continuation {
                PendingFetch::Document(pending_fetch) => {
                    self.send_user_agent_command(
                        UserAgentCommand::DocumentFetchFailed {
                            event_loop_id: pending_fetch.event_loop_id,
                            handler_id: pending_fetch.handler_id,
                        },
                        "failed to report pending document fetch failure",
                    );
                }
                PendingFetch::Navigation(pending_fetch) => {
                    self.send_user_agent_command(
                        UserAgentCommand::NavigationFetchFailed {
                            fetch_id: pending_fetch.fetch_id,
                        },
                        "failed to report pending navigation fetch failure",
                    );
                }
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn handle_command(&mut self, command: FetchCommand) {
        match command {
            FetchCommand::StartDocumentFetch {
                event_loop_id,
                request,
            } => {
                // Note: Document fetches reuse the same network process, but the completion returns
                // directly to the owning event loop instead of the navigation finalization path.
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                let handler_id = request.handler_id;
                let fetch_record = FetchRecord::from_content_fetch_request(
                    request,
                    PendingFetch::Document(PendingDocumentFetch {
                        event_loop_id,
                        handler_id,
                    }),
                );
                let network_request = fetch_record.network_request();
                self.fetch_group
                    .insert_fetch_record(request_id, fetch_record);
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Fetch {
                    request_id,
                    request: network_request,
                }) {
                    if let Some(fetch_record) = self.fetch_group.remove_fetch_record(request_id) {
                        match fetch_record.continuation {
                            PendingFetch::Document(pending_fetch) => {
                                self.send_user_agent_command(
                                    UserAgentCommand::DocumentFetchFailed {
                                        event_loop_id: pending_fetch.event_loop_id,
                                        handler_id: pending_fetch.handler_id,
                                    },
                                    "failed to report document fetch send failure",
                                );
                            }
                            PendingFetch::Navigation(pending_fetch) => {
                                self.send_user_agent_command(
                                    UserAgentCommand::NavigationFetchFailed {
                                        fetch_id: pending_fetch.fetch_id,
                                    },
                                    "failed to report navigation fetch send failure",
                                );
                            }
                        }
                    }
                    eprintln!("failed to send document fetch request to network process: {error}");
                }
            }
            FetchCommand::StartNavigationFetch { fetch_id, request } => {
                // Step 1: Assert: this is running in parallel.
                // The fetch worker is the concrete owner of the parallel navigation fetch
                // branch started by `UserAgentWorker::create_navigation_params_by_fetching`.
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                let fetch_record = FetchRecord::from_content_fetch_request(
                    request,
                    PendingFetch::Navigation(PendingNavigationFetch { fetch_id }),
                );
                let network_request = fetch_record.network_request();
                self.fetch_group
                    .insert_fetch_record(request_id, fetch_record);
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Fetch {
                    request_id,
                    request: network_request,
                }) {
                    self.fetch_group.remove_fetch_record(request_id);
                    self.send_user_agent_command(
                        UserAgentCommand::NavigationFetchFailed { fetch_id },
                        "failed to report navigation fetch send failure",
                    );
                    eprintln!(
                        "failed to send navigation fetch request to network process: {error}"
                    );
                }
            }
            FetchCommand::Shutdown { reply } => {
                self.fetch_group.terminate();
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Shutdown) {
                    eprintln!("failed to send network process shutdown request: {error}");
                }
                self.shutdown_reply = Some(reply);
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn handle_network_response(&mut self, response: NetworkResponse) {
        let Some(fetch_record) = self.fetch_group.remove_fetch_record(response.request_id) else {
            return;
        };

        let completion = match response.result {
            Ok(fetch_response) => completion_for_network_result(fetch_record, Ok(fetch_response)),
            Err(error) => {
                eprintln!("fetch failed: {error}");
                completion_for_network_result(fetch_record, Err(error))
            }
        };

        match completion {
            FetchCompletion::DocumentCompleted {
                event_loop_id,
                handler_id,
                response,
            } => {
                // Successful document fetches resume the owning event loop's content-side
                // continuation.
                self.send_user_agent_command(
                    UserAgentCommand::DocumentFetchCompleted {
                        event_loop_id,
                        handler_id,
                        response,
                    },
                    "failed to report document fetch completion",
                );
            }
            FetchCompletion::DocumentFailed {
                event_loop_id,
                handler_id,
            } => {
                // Document fetch failures reenter the owning event loop so the content-side
                // fetch algorithm can fail the handler in place.
                self.send_user_agent_command(
                    UserAgentCommand::DocumentFetchFailed {
                        event_loop_id,
                        handler_id,
                    },
                    "failed to report document fetch failure",
                );
            }
            FetchCompletion::NavigationCompleted { fetch_id, response } => {
                // Successful navigation fetches resume the user-agent-side document creation
                // and finalization continuation keyed by `fetch_id`.
                self.send_user_agent_command(
                    UserAgentCommand::NavigationFetchCompleted {
                        fetch_id,
                        response,
                    },
                    "failed to report navigation fetch completion",
                );
            }
            FetchCompletion::NavigationFailed { fetch_id } => {
                // Navigation fetch failures resume the same pending navigation record so the
                // user agent can clear `ongoing_navigation_id` and surface failure to the embedder.
                self.send_user_agent_command(
                    UserAgentCommand::NavigationFetchFailed { fetch_id },
                    "failed to report navigation fetch failure",
                );
            }
            FetchCompletion::Ignored => {}
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn run(&mut self) {
        // The fetch worker owns the network-facing half of HTML's parallel fetch branch and
        // drains either new user-agent requests or network-process responses until shutdown.
        loop {
            let command_receiver = &self.command_receiver;
            let network_event_receiver = &self.network_event_receiver;
            select! {
                recv(command_receiver) -> command => {
                    let Ok(command) = command else {
                        break;
                    };
                    let shutting_down = matches!(command, FetchCommand::Shutdown { .. });
                    self.handle_command(command);
                    if shutting_down {
                        break;
                    }
                }
                recv(network_event_receiver) -> response => {
                    let response = match response {
                        Ok(Ok(response)) => response,
                        Ok(Err(error)) => {
                            eprintln!("network process route error: {error}");
                            break;
                        }
                        Err(error) => {
                            eprintln!("network response route closed: {error}");
                            break;
                        }
                    };
                    self.handle_network_response(response);
                }
            }
        }

        self.fail_pending_fetches();
        finish_shutdown(self.child.take());
        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.send(Ok(()));
        }
    }
}

/// spawning the dedicated fetch worker thread owned by `UserAgentWorker`.
pub fn run_fetch_thread(
    command_receiver: Receiver<FetchCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
    trace_sender: Option<TraceSender>,
) {
    let mut worker =
        match FetchWorker::new(command_receiver, user_agent_command_sender, trace_sender) {
            Ok(worker) => worker,
            Err(error) => {
                eprintln!("fetch thread startup failed: {error}");
                return;
            }
        };
    worker.run();
}
