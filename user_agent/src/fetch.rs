use crossbeam_channel::{Receiver, Sender, select};
use ipc_messages::content::{
    DocumentFetchId, EventLoopId, FetchRequest as ContentFetchRequest,
    FetchResponse as ContentFetchResponse, NavigationFetchId,
};
use ipc_messages::network::{Request as NetworkRequest, Response as NetworkResponse};
use log::error;
use std::collections::HashMap;
#[cfg(unix)]
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};
use verification::TraceSender;

use crate::UserAgentCommand;
use crate::ipc_manifest::NetExtensionManifest;

/// graceful shutdown of the network sidecar owned by the fetch worker.
const FETCH_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// <https://fetch.spec.whatwg.org/#concept-header-list>
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HeaderList {
    /// <https://fetch.spec.whatwg.org/#concept-header-list>
    headers: Vec<(String, String)>,
}

impl HeaderList {
    fn new() -> Self {
        Self::default()
    }

    fn from_content_type(content_type: &str) -> Self {
        // Note: Phase 1 maps the existing content IPC `content_type` field into a one-entry
        // header list. Full response headers belong in a later net/content IPC shape.
        if content_type.is_empty() {
            return Self::new();
        }

        Self {
            headers: vec![(String::from("content-type"), content_type.to_owned())],
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
    /// Note: Phase 1 preserves the current IPC body string here instead of modeling a Fetch body
    /// stream. Body streaming remains out of scope for this PR.
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
            body,
        } = request;

        Self {
            url,
            method,
            header_list: HeaderList::new(),
            body,
            done: false,
            keepalive: false,
        }
    }

    fn to_content_fetch_request(&self, handler_id: DocumentFetchId) -> ContentFetchRequest {
        ContentFetchRequest {
            handler_id,
            url: self.url.clone(),
            method: self.method.clone(),
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
    /// <https://fetch.spec.whatwg.org/#concept-response-header-list>
    header_list: HeaderList,
    // Note: Formal-web's current content IPC exposes `content_type` as a separate convenience
    // field. The spec-shaped value above is `header_list`; this field is preserved only to round
    // trip the existing `FetchResponse` transport without changing behavior.
    content_type: String,
    /// <https://fetch.spec.whatwg.org/#concept-response-body>
    body: Vec<u8>,
}

impl InternalFetchResponse {
    fn from_content_fetch_response(response: ContentFetchResponse) -> Self {
        Self {
            url_list: vec![response.final_url],
            status: response.status,
            header_list: HeaderList::from_content_type(&response.content_type),
            content_type: response.content_type,
            body: response.body,
        }
    }

    fn into_content_fetch_response(self) -> ContentFetchResponse {
        ContentFetchResponse {
            final_url: self.url_list.last().cloned().unwrap_or_default(),
            status: self.status,
            content_type: self.content_type,
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
    // TODO: Wire this to AbortSignal/controller integration once content can initiate aborts and
    // formal-web can carry structured abort reasons across content, user-agent, and net.
    // Note: Phase 1 keeps the spec algorithm present so upcoming AbortSignal work has a precise
    // controller entry point, but no production path calls it yet.
    #[allow(dead_code)]
    pub(crate) fn abort(&mut self, error: Option<String>) {
        // Step 1. Set controller's state to "aborted".
        self.state = FetchControllerState::Aborted;
        // Step 2. Let fallbackError be an "AbortError" DOMException.
        let fallback_error = String::from("AbortError");
        // Step 3. Set error to fallbackError if it is not given.
        let error = error.unwrap_or_else(|| fallback_error.clone());
        // Step 4. Let serializedError be StructuredSerialize(error).
        // TODO: Step 4. Replace this placeholder with StructuredSerialize(error).
        // Note: formal-web does not yet expose DOMException or structured clone values across
        // this worker boundary, so Phase 1 stores the serialized reason as a string placeholder.
        let serialized_error = error;
        // Step 5. Set controller's serialized abort reason to serializedError.
        self.serialized_abort_reason = Some(serialized_error);
    }

    /// <https://fetch.spec.whatwg.org/#fetch-controller-terminate>
    pub(crate) fn terminate(&mut self) {
        // Step 1. Set controller's state to "terminated".
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
    /// Formal-web continuation resumed when the network process completes this fetch.
    continuation: PendingFetch,
}

impl FetchRecord {
    fn navigation_transport_handler_id() -> DocumentFetchId {
        // Note: Navigation fetches are keyed by `NavigationFetchId` in the user agent, and the net
        // process ignores `FetchRequest.handler_id`. This placeholder exists only because Phase 1
        // still reuses the content document-fetch IPC request shape for all network fetches. It
        // is stable to make the intentionally-unused value obvious, and should disappear when net
        // receives a Fetch-owned request type.
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
// `deferred fetch record` fields are intentionally deferred until deferred fetch processing exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeferredFetchRecord;

/// <https://fetch.spec.whatwg.org/#concept-fetch-group>
// Note: Phase 1 keeps one fetch group on the fetch worker. The Fetch Standard associates a fetch
// group with an environment settings object; that ownership split will be introduced when content
// exposes environment-scoped Fetch API state.
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
        // Step 1. For each fetch record record of fetchGroup's fetch records, if record's
        // controller is non-null and record's request's done flag is unset and keepalive is
        // false, terminate record's controller.
        for record in self.fetch_records.values_mut() {
            if let Some(controller) = record.controller.as_mut() {
                if !record.request.done && !record.request.keepalive {
                    controller.terminate();
                }
            }
        }
        // TODO: Step 2. "Process deferred fetches for fetchGroup."
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
            error!("ignored aborted fetch failure: {error}");
        }
        return FetchCompletion::Ignored;
    }

    if record.is_canceled() {
        if let Err(error) = result {
            error!("ignored canceled fetch failure: {error}");
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
        (PendingFetch::Navigation(pending_fetch), Err(_error)) => {
            FetchCompletion::NavigationFailed {
                fetch_id: pending_fetch.fetch_id,
            }
        }
    }
}

/// Stateful owner of the network-facing half of HTML's parallel fetch work plus document-fetch
/// plumbing that resumes event-loop-local fetch handlers.
struct FetchWorker {
    /// Receiver for user-agent fetch commands.
    command_receiver: Receiver<FetchCommand>,
    /// Sender back into the user-agent thread for navigation/document fetch completions.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// IPC sender to the dedicated network sidecar process.
    network_request_sender: ipc::IpcSender<NetworkRequest>,
    /// IPC receiver for network sidecar responses.
    network_event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
    /// Child process handle for the network sidecar.
    child: Option<Child>,
    /// Transport-local request id allocator for the network IPC bridge.
    next_request_id: u64,
    /// <https://fetch.spec.whatwg.org/#concept-fetch-group>
    fetch_group: FetchGroup,
    /// Deferred shutdown reply completed after the network sidecar exits.
    shutdown_reply: Option<Sender<Result<(), String>>>,
}

/// waiting on the network sidecar during shutdown.
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

/// gracefully shutting down the network sidecar owned by the fetch worker.
fn finish_shutdown(mut child: Option<Child>) {
    if let Some(child) = child.as_mut() {
        match wait_for_child_exit(child, FETCH_SHUTDOWN_GRACE_TIMEOUT) {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = child.kill() {
                    error!("failed to kill network process: {error}");
                }
                if let Err(error) = child.wait() {
                    error!("failed to wait for network process exit: {error}");
                }
            }
            Err(error) => {
                error!("fetch shutdown poll error: {error}");
            }
        }
    }
}

/// Start the net extension using the new IPC abstraction layer.
pub fn start_net_extension(
    trace_sender: Option<TraceSender>,
) -> Result<
    (
        ipc::IpcSender<NetworkRequest>,
        crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
        Option<std::process::Child>,
    ),
    String,
> {
    let manifest = NetExtensionManifest;
    let mut client =
        ipc::start_extension::<NetExtensionManifest, NetworkRequest, NetworkResponse>(&manifest)
            .map_err(|error| format!("failed to start net extension: {error}"))?;

    // Send initial trace sender if set
    if let Some(trace_sender) = trace_sender {
        client
            .sender
            .send(NetworkRequest::SetTraceSender(Some(trace_sender)))
            .map_err(|error| format!("failed to send trace sender to net: {error}"))?;
    }

    let child = client.take_child();
    Ok((
        client.sender.clone(),
        ipc::crossbeam_proxy(client.receiver.clone()),
        child,
    ))
}

impl FetchWorker {
    /// starting the fetch worker with its owned network sidecar.
    fn new(
        command_receiver: Receiver<FetchCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
        trace_sender: Option<TraceSender>,
    ) -> Result<Self, String> {
        let (network_request_sender, network_event_receiver, child) =
            start_net_extension(trace_sender)?;
        Ok(Self {
            command_receiver,
            user_agent_command_sender,
            network_request_sender,
            network_event_receiver,
            child,
            next_request_id: 1,
            fetch_group: FetchGroup::new(),
            shutdown_reply: None,
        })
    }

    /// failing every pending fetch if the network sidecar stops before
    /// producing a response.
    fn fail_pending_fetches(&mut self) {
        // If the network bridge stops early, report every outstanding fetch back through the
        // same user-agent continuations that would have handled an ordinary network failure.
        for fetch_record in self.fetch_group.drain_fetch_records() {
            match fetch_record.continuation {
                PendingFetch::Document(pending_fetch) => {
                    let _ = self.user_agent_command_sender.send(
                        UserAgentCommand::DocumentFetchFailed {
                            event_loop_id: pending_fetch.event_loop_id,
                            handler_id: pending_fetch.handler_id,
                        },
                    );
                }
                PendingFetch::Navigation(pending_fetch) => {
                    let _ = self.user_agent_command_sender.send(
                        UserAgentCommand::NavigationFetchFailed {
                            fetch_id: pending_fetch.fetch_id,
                        },
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
                // Document fetches reuse the same network sidecar, but the completion returns
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
                                let _ = self.user_agent_command_sender.send(
                                    UserAgentCommand::DocumentFetchFailed {
                                        event_loop_id: pending_fetch.event_loop_id,
                                        handler_id: pending_fetch.handler_id,
                                    },
                                );
                            }
                            PendingFetch::Navigation(pending_fetch) => {
                                let _ = self.user_agent_command_sender.send(
                                    UserAgentCommand::NavigationFetchFailed {
                                        fetch_id: pending_fetch.fetch_id,
                                    },
                                );
                            }
                        }
                    }
                    error!("failed to send document fetch request to network process: {error}");
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
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::NavigationFetchFailed { fetch_id });
                    error!("failed to send navigation fetch request to network process: {error}");
                }
            }
            FetchCommand::Shutdown { reply } => {
                self.fetch_group.terminate();
                let _ = self.network_request_sender.send(NetworkRequest::Shutdown);
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
                error!("fetch failed: {error}");
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
                let _ =
                    self.user_agent_command_sender
                        .send(UserAgentCommand::DocumentFetchCompleted {
                            event_loop_id,
                            handler_id,
                            response,
                        });
            }
            FetchCompletion::DocumentFailed {
                event_loop_id,
                handler_id,
            } => {
                // Document fetch failures reenter the owning event loop so the content-side
                // fetch algorithm can fail the handler in place.
                let _ =
                    self.user_agent_command_sender
                        .send(UserAgentCommand::DocumentFetchFailed {
                            event_loop_id,
                            handler_id,
                        });
            }
            FetchCompletion::NavigationCompleted { fetch_id, response } => {
                // Successful navigation fetches resume the user-agent-side document creation
                // and finalization continuation keyed by `fetch_id`.
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchCompleted { fetch_id, response });
            }
            FetchCompletion::NavigationFailed { fetch_id } => {
                // Navigation fetch failures resume the same pending navigation record so the
                // user agent can clear `ongoing_navigation_id` and surface failure to the embedder.
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchFailed { fetch_id });
            }
            FetchCompletion::Ignored => {}
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn run(&mut self) {
        // The fetch worker owns the network-facing half of HTML's parallel fetch branch and
        // drains either new user-agent requests or sidecar responses until shutdown.
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
                    match response {
                        Ok(incoming) => {
                            self.handle_network_response(incoming.payload);
                        }
                        Err(error) => {
                            error!("network response route closed: {error}");
                            break;
                        }
                    }
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
                error!("fetch thread startup failed: {error}");
                return;
            }
        };
    worker.run();
}
