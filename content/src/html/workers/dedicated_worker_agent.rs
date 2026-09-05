use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::thread::JoinHandle;

use data_url::DataUrl;
use ipc::IpcSender;
use ipc_messages::content::{
    Command, DocumentFetchId, Event as ContentEvent, EventLoopId,
    FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse, PortId,
    PortTaskKind, WorkerId, WorkerOwner, WorkerRequest,
};
use ipc_messages::network::{Request as NetworkRequest, ResponseRecipient};
use log::error;
use url::Url;

use super::worker::WorkerType;
use crate::dom::event::EventTarget;
use crate::dom::fire_event;
use crate::html::environment_settings_object::{EnvironmentSettingsObject, WorkerRealmWiring};
use crate::html::event_loop::{EventLoopTaskSources, Task, TaskQueue};
use crate::html::messageport::deliver_serialized_message;
use crate::html::structured_data::safe_passing_of_structured_data::SerializeWithTransferResult;
use crate::html::timers::{MapOfActiveTimers, TimerRealm};
use crate::js::platform_objects::{
    with_dedicated_worker_global_scope, with_global_scope, with_worker_global_scope,
};
use js_engine::gc_struct;

use verification::TraceSender;

/// One message on a dedicated worker's channel: the message serialized in
/// the sending realm (structured serialize with transfer), deserialized in
/// the receiving realm when the message event fires.
/// <https://html.spec.whatwg.org/#structuredserializewithtransfer>
pub(crate) type WorkerChannelMessage = SerializeWithTransferResult;

/// The message queue of one end of a dedicated worker's channel: messages
/// that arrive while the queue is disabled wait here until it is enabled (a
/// port message queue can be enabled, and is initially disabled).
/// <https://html.spec.whatwg.org/#port-message-queue>
#[derive(Default)]
pub(crate) struct WorkerMessageQueue {
    pub(crate) enabled: bool,
    pub(crate) pending: VecDeque<WorkerChannelMessage>,
}

/// The owner-side record of one dedicated worker's channel, registered on
/// the owner realm's global scope by the Worker constructor and dropped
/// when the worker closes: the message event target the worker's posted
/// messages fire at, and the owner-side message queue gating their delivery.
/// Note: The worker's implicit port pair is bypassed (see `worker.rs`); this
/// record replaces the outside port's record of the port-based model.
#[gc_struct]
pub(crate) struct OwnedWorkerChannel {
    /// The Worker platform object's event target: the message event target
    /// the worker's posted messages fire at (the outside port's role).
    pub(crate) worker_target: EventTarget,

    /// The worker this channel belongs to.
    #[ignore_trace]
    pub(crate) worker_id: WorkerId,

    /// The owner end's message queue: whether posted messages may fire, and
    /// the messages that arrived while the queue was disabled.
    #[ignore_trace]
    pub(crate) queue: Rc<RefCell<WorkerMessageQueue>>,
}

/// One item the owner sends a worker over its owner→worker channel: the
/// messages the owner posts (as if on the Worker's outside port), and the
/// terminate command.
/// <https://html.spec.whatwg.org/#run-a-worker>
pub(crate) enum WorkerInbound {
    /// A message the owner posted (`Worker.postMessage`), serialized in the
    /// owner realm (message port post message steps; see `worker.rs`): the
    /// agent's event loop delivers it as a message event at the worker
    /// global scope once its message queue is enabled.
    /// <https://html.spec.whatwg.org/#dom-worker-postmessage>
    Message(WorkerChannelMessage),
    /// The terminate-a-worker command: `terminate()` on the Worker object,
    /// or the owner event loop terminating the worker (its document was
    /// destroyed, its owner worker closed, or the process is shutting down).
    /// The agent sets its closing flag and its event loop exits.
    /// <https://html.spec.whatwg.org/#terminate-a-worker>
    Terminate,
}

/// An event on an owner event loop's worker inbox: sent by a worker this
/// loop's realm owns (its posted messages and lifecycle reports) and by the
/// realm itself when it spawns a worker (the registration of its handle).
/// <https://html.spec.whatwg.org/#run-a-worker>
pub(crate) enum WorkerEvent {
    /// A worker this loop's realm owns was spawned: the Worker constructor
    /// registers the worker's handle (the owner→worker channel end used to
    /// terminate it, and its agent's thread, joined when the worker closes)
    /// with its owner event loop.
    NewWorker {
        worker_id: WorkerId,
        /// The realm that created the worker: a document of the window agent
        /// (the content process main loop), or the worker global scope of
        /// this agent (a nested worker).
        owner: WorkerOwner,
        /// The owner→worker channel end of the worker's channel (a clone of
        /// the Worker's outside port): how the owner event loop terminates
        /// the worker when its owner goes away.
        owner_to_worker: crossbeam_channel::Sender<WorkerInbound>,
        /// The dedicated worker agent's thread, joined when the worker
        /// closes.
        join_handle: JoinHandle<()>,
    },
    /// A message the worker posted back (`self.postMessage` on its global
    /// scope): the owner loop fires it as a message event at the worker's
    /// Worker object in the owner realm (see the owner-side record in the
    /// realm's global scope).
    /// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-postmessage>
    Message {
        worker_id: WorkerId,
        payload: WorkerChannelMessage,
    },
    /// run-a-worker step 12.14, owner half: enable the owner-side delivery of
    /// the worker's posted messages (the outside port's port message queue),
    /// so the messages that arrived while the queue was disabled fire as
    /// message tasks in the owner realm.
    /// <https://html.spec.whatwg.org/#run-a-worker>
    EnableQueue { worker_id: WorkerId },
    /// run-a-worker step 12.4.1: the worker's script could not be obtained
    /// (or failed to evaluate): fire an event named error at the worker's
    /// Worker object in the owner realm.
    /// <https://html.spec.whatwg.org/#run-a-worker>
    FireError { worker_id: WorkerId },
    /// The worker's agent is exiting: its owner event loop joins its thread
    /// and drops the owner end of its channel in the owner realm
    /// (run-a-worker steps 12.19-12.21 and terminate-a-worker step 4).
    Closed { worker_id: WorkerId },
}

/// The record an owner event loop keeps for one worker it owns: what the
/// loop needs beyond the Worker platform object in its realm — the
/// owner→worker channel end used to terminate the worker, and the agent's
/// thread, joined when the worker closes.
/// <https://html.spec.whatwg.org/#run-a-worker>
pub(crate) struct WorkerHandle {
    /// <https://html.spec.whatwg.org/#concept-WorkerGlobalScope-owner-set>
    pub(crate) worker_id: WorkerId,
    /// <https://html.spec.whatwg.org/#concept-WorkerGlobalScope-owner-set>
    pub(crate) owner: WorkerOwner,
    /// The owner→worker channel end (a clone of the Worker's outside port):
    /// how the owner terminates the worker.
    pub(crate) owner_to_worker: crossbeam_channel::Sender<WorkerInbound>,
    /// The dedicated worker agent's thread, joined when the worker closes.
    pub(crate) join_handle: Option<JoinHandle<()>>,
}

/// Everything the Worker constructor hands its new dedicated worker agent's
/// thread.
pub(crate) struct DedicatedWorkerAgentConfig {
    pub(crate) request: WorkerRequest,
    /// The receiver end of the owner→worker channel (the messages
    /// `worker.postMessage` sends and the terminate command): the agent's
    /// event loop selects on it.
    pub(crate) owner_to_worker: crossbeam_channel::Receiver<WorkerInbound>,
    /// The owner's worker inbox: the worker reports its posted messages and
    /// lifecycle over it (a clone also becomes its global scope's inside
    /// port).
    pub(crate) owner_inbox: crossbeam_channel::Sender<WorkerEvent>,
    /// <https://html.spec.whatwg.org/#worker-event-loop-2>
    /// The event loop id of the similar-origin window agent whose content
    /// process hosts this worker's thread (the agent cluster the worker
    /// agent belongs to): the id carried in the worker's net fetch requests
    /// as the network partition key, so the worker shares the network
    /// partition of its owner's window event loop.  Not the worker agent's
    /// own event loop id, which the worker allocates for itself when it
    /// starts.
    pub(crate) network_partition_event_loop_id: EventLoopId,
    /// The content-to-user-agent event sender, wired into the worker realm's
    /// global scope for its channel messaging, and used to report the
    /// obtained agent (and its close) to the user agent.
    pub(crate) event_sender: IpcSender<ContentEvent>,
    /// The worker's own channel to the net process, for script fetches.
    pub(crate) network_extension_sender: IpcSender<ipc_messages::network::Request>,
    pub(crate) trace_sender: Option<TraceSender>,
}

/// The run-a-worker state owned by a dedicated worker agent (running on its
/// native thread).
pub(crate) struct DedicatedWorkerAgentState {
    pub(crate) worker_id: WorkerId,
    pub(crate) settings: EnvironmentSettingsObject,
    /// The owner→worker end of the worker's channel: the messages
    /// `worker.postMessage` sends (and the terminate command), delivered as
    /// message events at the worker global scope once its message queue is
    /// enabled.
    pub(crate) owner_to_worker: crossbeam_channel::Receiver<WorkerInbound>,
    /// This agent's worker inbox: the events of the workers its realm owns
    /// (nested workers) — their registration, posted messages and lifecycle.
    pub(crate) inbox: crossbeam_channel::Receiver<WorkerEvent>,
    /// The workers this realm owns (nested workers), keyed by worker id:
    /// their handles (the owner→worker channel end to terminate them, and
    /// their threads, joined when this agent closes and its nested workers
    /// are terminated with it).
    pub(crate) nested_workers: HashMap<WorkerId, WorkerHandle>,
    /// The owner's worker inbox: this worker reports its posted messages and
    /// lifecycle to its owner over it.
    pub(crate) owner_inbox: crossbeam_channel::Sender<WorkerEvent>,
    /// The worker's own task queue and timer map: the dedicated worker
    /// agent's event loop task sources (a dedicated worker agent has its own
    /// event loop).
    pub(crate) task_queue: TaskQueue,
    pub(crate) active_timers: Rc<RefCell<MapOfActiveTimers>>,
    /// The event loop id of the similar-origin window agent whose content
    /// process hosts this worker's thread: the id carried in the worker's
    /// net script-fetch request as the network partition key (the worker
    /// shares its owner window's network partition).  Not the worker
    /// agent's own event loop id.
    pub(crate) network_partition_event_loop_id: EventLoopId,
    pub(crate) network_extension_sender: IpcSender<ipc_messages::network::Request>,
    /// The worker's own command channel to the net process; the net process
    /// sends the script fetch completion commands here.
    pub(crate) net_command_sender: IpcSender<Command>,
    /// The id of the worker's in-flight script fetch, matched against
    /// CompleteDocumentFetch/FailDocumentFetch on its net channel.
    pub(crate) pending_script_fetch: Option<DocumentFetchId>,
}

/// <https://html.spec.whatwg.org/#run-a-worker>
pub(crate) fn run_a_worker(config: DedicatedWorkerAgentConfig) -> Result<(), String> {
    let request = config.request;
    let worker_id = request.worker_id;
    let owner = request.owner;

    // Step 1: "Let is shared be true if worker is a SharedWorker object, and
    // false otherwise."
    // Note: Only the dedicated path is implemented: is shared is false.
    // Step 2: "Let owner be the relevant owner to add given outside settings."
    // Note: The owner was computed by the Worker constructor and carried in
    // the request.
    // Step 3: "Let unsafeWorkerCreationTime be the unsafe shared current
    // time."
    // Note: Not implemented.
    // Step 4: "Let agent be the result of obtaining a dedicated/shared worker
    // agent given outside settings and is shared. Run the rest of these
    // steps in that agent."
    // Note: The two halves of step 4 map onto the two sides of the
    // dedicated-worker-agent split.  Obtaining the agent — creating this
    // native thread — ran in the Worker constructor: it ran the dedicated
    // start of run a worker (steps 1-3 and this step) synchronously in the
    // owner realm (see worker.rs).  This function body is the second half of
    // the step, "run the rest of these steps in that agent": with is shared
    // false, obtaining the agent never creates a new agent cluster, so the
    // agent is nested to the content process hosting the owner realm (the
    // same agent cluster as its owner's similar-origin window agent), and
    // the thread's event loop below is the agent's event loop — its own
    // worker event loop.  The user-agent half of obtaining the agent runs
    // here first: this thread creates its own UA command channel and reports
    // the obtained agent (`DedicatedWorkerAgentObtained`, carrying the
    // agent's own worker event loop id and the user-agent end of its own
    // command channel), so the user agent routes port tasks for ports of
    // this event loop directly to this thread over that channel.
    // <https://html.spec.whatwg.org/#dedicated-worker-agent>
    let (ua_command_sender, ua_command_receiver) = ipc::channel::<Command>()
        .map_err(|error| format!("worker {worker_id}: failed to create UA channel: {error}"))?;
    let ua_command_rx = ipc::crossbeam_proxy(ua_command_receiver);
    // <https://html.spec.whatwg.org/#worker-event-loop-2>
    let worker_event_loop_id = EventLoopId::new();
    if let Err(error) = config
        .event_sender
        .send(ContentEvent::DedicatedWorkerAgentObtained {
            worker_id,
            event_loop_id: worker_event_loop_id,
            owner,
            ua_command_sender: ua_command_sender.clone(),
        })
    {
        error!("failed to report dedicated worker agent {worker_id} to the user agent: {error}");
    }
    // This agent's own worker inbox: the channel the workers its realm
    // spawns (nested workers) report on; its sender is stored on the realm's
    // global scope when the realm is wired below.
    let (inbox_sender, inbox_receiver) = crossbeam_channel::unbounded::<WorkerEvent>();

    let task_queue = TaskQueue::new();
    let active_timers = Rc::new(RefCell::new(MapOfActiveTimers::default()));
    // Step 5: "Let realm execution context be the result of creating a new
    // realm given agent and the following customizations: For the global
    // object ... create a new DedicatedWorkerGlobalScope object."
    // Step 6: "Let worker global scope be the global object of realm
    // execution context's Realm component."
    // Step 7: "Set up a worker environment settings object with realm
    // execution context, outside settings, and unsafeWorkerCreationTime, and
    // let inside settings be the result."
    // Step 8: "Set worker global scope's name to options["name"]."
    // Step 9: "Append owner to worker global scope's owner set."
    // Note: The realm is built on this thread with a fresh engine (its own
    // JS heap), and `new_worker_in_realm` creates the worker environment
    // settings object with the worker's own task sources.
    // Step 10: (shared-worker fields) — Not applicable: is shared is false.
    // Step 11: Let destination be "sharedworker" if is shared is true, and
    //          "worker" otherwise.
    // Note: destination is "worker"; the fetch request sent to the net
    // process carries no destination.
    let wiring = WorkerRealmWiring {
        event_sender: config.event_sender.clone(),
        task_sources: EventLoopTaskSources::new(task_queue.clone(), Rc::clone(&active_timers)),
    };
    let (mut settings, _worker_global_scope) = EnvironmentSettingsObject::new_worker_in_realm(
        Url::parse(&request.script_url)
            .map_err(|error| format!("invalid worker script URL: {error}"))?,
        worker_id,
        request.name.clone(),
        WorkerType::from_idl(&request.worker_type),
        wiring,
    )?;
    // The worker realm's global scope gets its own event loop id (this
    // dedicated worker agent's worker event loop), its own worker inbox (the
    // nested workers its realm spawns report on it), the window agent's
    // event loop id as its network partition key, the trace sender, and the
    // owner→worker end of the worker's channel as its inside port, so the
    // worker's postMessage can reach its owner.
    with_dedicated_worker_global_scope(settings.ec(), |dedicated_scope, _ec| {
        dedicated_scope
            .worker_global_scope
            .global_scope
            .set_event_loop_id(worker_event_loop_id);
        dedicated_scope
            .worker_global_scope
            .global_scope
            .set_worker_inbox(inbox_sender);
        dedicated_scope
            .worker_global_scope
            .global_scope
            .set_network_partition_event_loop_id(config.network_partition_event_loop_id);
        dedicated_scope
            .worker_global_scope
            .global_scope
            .set_trace_sender(config.trace_sender.clone());
        dedicated_scope
            .worker_global_scope
            .global_scope
            .set_network_extension_sender(config.network_extension_sender.clone());
        dedicated_scope.set_inside_port(config.owner_inbox.clone());
        Ok(())
    })
    .map_err(|error| format!("failed to wire worker realm: {}", error.display()))?;

    // The worker fetches its script over its own IPC channel to the net
    // process: the request carries this channel's sender as its response
    // recipient, and the net process sends
    // Command::CompleteDocumentFetch/FailDocumentFetch back on it.  The
    // receiver is bridged over crossbeam into the worker's event-loop
    // select, like the content process's own loop.
    let (net_command_sender, net_command_receiver) = ipc::channel::<Command>()
        .map_err(|error| format!("worker {worker_id}: failed to create net channel: {error}"))?;
    let net_command_rx = ipc::crossbeam_proxy(net_command_receiver);

    let mut state = DedicatedWorkerAgentState {
        worker_id,
        settings,
        owner_to_worker: config.owner_to_worker,
        inbox: inbox_receiver,
        nested_workers: HashMap::new(),
        owner_inbox: config.owner_inbox,
        task_queue,
        active_timers,
        network_partition_event_loop_id: config.network_partition_event_loop_id,
        network_extension_sender: config.network_extension_sender,
        net_command_sender,
        pending_script_fetch: None,
    };

    // Step 12: "Obtain script by switching on options["type"]: "classic":
    // Fetch a classic worker script given url, outside settings, destination,
    // inside settings, and with onComplete ..."
    // Note: The fetch is the limited fetch implementation the content
    // process already uses: data: URLs are decoded locally, other schemes go
    // through the net process.  The onComplete steps (12.1-12.15) run when
    // the fetch completes (for a data: URL, inline below; for a net fetch,
    // when the reply arrives on the worker's net channel).
    state.start_script_fetch(request.script_url)?;

    // Step 12.18: "Event loop: Run the responsible event loop specified by
    // inside settings until it is destroyed."
    let loop_result = run_worker_event_loop(&mut state, &net_command_rx, &ua_command_rx);

    // The owner-set cascade of this worker's teardown: the workers this
    // realm owns (nested workers) die with their owner — terminate each and
    // join its thread here, so the cascade completes on this thread before
    // it returns (see `terminate_and_join_nested_workers`).  The nested
    // workers' threads report their own closed events to this agent's inbox,
    // which the exited loop no longer reads; the joins below wait for them
    // to finish.
    state.terminate_and_join_nested_workers();

    // Step 12.19: "Clear the worker global scope's map of active timers."
    // Step 12.20: "Disentangle all the ports in the list of the worker's
    // ports."
    // Step 12.21: "Empty worker global scope's owner set."
    // Note: The timer map, the worker's inbound channel receiver and the
    // realm drop with this state when the function returns; the owner-side
    // cleanup (empty the owner end of the worker's channel, terminate a
    // worker step 4) runs in the owner event loop when it handles this
    // worker's closed event.
    loop_result
}

impl DedicatedWorkerAgentState {
    /// Whether the worker's closing flag is set (close-a-worker, terminate-a
    /// -worker step 1): the event loop exits once it is.
    fn closing_flag(&mut self) -> bool {
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
            Ok(worker_global_scope.closing_flag.get())
        })
        .map_err(|error| format!("failed to read worker closing flag: {}", error.display()))
        .unwrap_or(true)
    }

    /// Terminate the workers this realm owns (nested workers) and join their
    /// threads: their owner realm is going away.  Runs when this agent's
    /// event loop exits (its closing flag was set by close(),
    /// terminate-a-worker, or the failure of its own script fetch), before
    /// this thread returns, so shutdown travels down the owner chain.
    fn terminate_and_join_nested_workers(&mut self) {
        let nested: Vec<WorkerHandle> = self
            .nested_workers
            .drain()
            .map(|(_worker_id, handle)| handle)
            .collect();
        for handle in nested {
            // <https://html.spec.whatwg.org/#terminate-a-worker>
            // The nested worker's own event loop exits on the terminate
            // command; the join waits for its thread (which terminates its
            // own nested workers first) to finish.
            let _ = handle.owner_to_worker.send(WorkerInbound::Terminate);
            if let Some(join_handle) = handle.join_handle
                && let Err(panic) = join_handle.join()
            {
                error!(
                    "worker {}: nested worker thread panicked during teardown: {panic:?}",
                    self.worker_id
                );
            }
        }
    }

    /// Report the worker's teardown to its owner event loop (the owner joins
    /// the agent's thread and drops the owner end of its channel).  Runs as
    /// the thread's last act, also on failure or panic.

    /// <https://html.spec.whatwg.org/#fetch-a-classic-worker-script>
    fn start_script_fetch(&mut self, script_url: String) -> Result<(), String> {
        // Note: Partial implementation of fetch a classic worker script (the
        // fetch run-a-worker step 12's "classic" branch performs): only the
        // URL is resolved and the fetch dispatched — a data: URL decodes
        // locally and completes inline, any other scheme goes through the
        // net process.  The request itself (destination, mode, credentials)
        // and the algorithm's response steps are not modeled; the
        // implemented response handling runs in
        // complete_worker_script_fetch once the script response arrives.
        let resolved_url = Url::parse(&script_url)
            .map_err(|error| format!("invalid worker script URL: {error}"))?;
        if resolved_url.scheme() == "data" {
            // data: worker scripts are decoded locally, mirroring the
            // deferred script path (the net process does not fetch data:).
            let (bytes, _fragment) = DataUrl::process(&script_url)
                .map_err(|error| format!("failed to decode data: worker script: {error}"))?
                .decode_to_vec()
                .map_err(|error| format!("failed to read data: worker script body: {error}"))?;
            let response = ContentFetchResponse {
                final_url: script_url,
                status: 200,
                content_type: String::from("text/javascript"),
                body: bytes,
            };
            return self.complete_worker_script_fetch(response);
        }
        // Other schemes go through the net process; the reply comes back on
        // this worker's own net command channel (the request's
        // ResponseRecipient).
        let handler_id = DocumentFetchId::new();
        let fetch_request = ContentFetchRequest {
            handler_id,
            url: resolved_url.to_string(),
            method: String::from("GET"),
            body: String::new(),
        };
        let network_request = NetworkRequest::Fetch {
            // The network partition key: the host window agent's event loop
            // id, never the worker agent's own event loop id.
            event_loop_id: self.network_partition_event_loop_id,
            request_id: uuid::Uuid::new_v4(),
            request: fetch_request,
            reply_to: ResponseRecipient::ContentProcess {
                content_command_sender: self.net_command_sender.clone(),
                handler_id,
            },
        };
        self.network_extension_sender
            .send(network_request)
            .map_err(|error| {
                format!("failed to send worker script fetch request to net: {error}")
            })?;
        self.pending_script_fetch = Some(handler_id);
        Ok(())
    }

    /// The completion of the worker's script fetch: the run-a-worker
    /// step-12 steps that are implemented (12.3.1-12.15) run here once the
    /// script response has been obtained — inline for a data: URL, or when
    /// the net process replies on this worker's net command channel
    /// (handle_net_command).
    fn complete_worker_script_fetch(
        &mut self,
        response: ContentFetchResponse,
    ) -> Result<(), String> {
        // The onComplete steps of the script fetch, run once the script has
        // been obtained.
        // Step 12.3.1: "Set worker global scope's url to response's URL."
        let final_url = Url::parse(&response.final_url)
            .map_err(|error| format!("invalid worker script URL: {error}"))?;
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
            worker_global_scope.set_url(final_url.clone());
            worker_global_scope
                .global_scope
                .set_creation_url(final_url.clone());
            Ok(())
        })
        .map_err(|error| format!("failed to set worker global scope url: {}", error.display()))?;
        // Step 12.3.2: "Set inside settings's creation URL to response's URL."
        self.settings.creation_url = final_url.clone();
        // Step 12.3.3-12.3.9: policy container, CSP, embedder policy and
        // cross-origin isolation steps.
        // Note: Not implemented.
        // Step 12.4: "If script is null or if script's error to rethrow is
        // non-null:"
        // Note: The executable check (status + JavaScript MIME type) stands
        // in for a null script; parse failures are handled after evaluation.
        if !crate::deferred_script_response_is_executable(&response) {
            return self.fail_worker_script_fetch();
        }
        // Step 12.5: "Associate worker with worker global scope."
        // Note: The association is the worker's handle in the content
        // process (registered when the dedicated worker agent's thread was
        // spawned); the Worker platform object's own slot is not filled
        // (terminate and close route through the content process, which
        // holds the association).
        // Step 12.6: "Let inside port be a new MessagePort object in inside
        // settings's realm."
        // Step 12.7.1: "Set inside port's message event target to worker
        // global scope."
        // Step 12.7.2: "Set worker global scope's inside port to inside
        // port."
        // Step 12.8: "Entangle outside port and inside port."
        // Note: The worker's implicit port is bypassed: the constructor
        // created the owner→worker end of the worker's channel in the owner
        // realm, this agent's event loop selects on its receiver, and the
        // worker global scope's inside port is the owner's worker inbox (the
        // channel its posted messages and lifecycle reports travel on).  The
        // owner-side delivery state was registered by the constructor in the
        // owner realm's global scope.
        // Step 12.9: "Create a new WorkerLocation object and associate it
        // with worker global scope."
        // Note: The WorkerLocation is created lazily on first access
        // (WorkerGlobalScope::location_value), so there is no eager creation
        // here.
        // Step 12.10: "Closing orphan workers: Start monitoring worker
        // global scope ..."
        // Step 12.11: "Suspending workers: Start monitoring worker global
        // scope ..."
        // Note: Not implemented: the owner-set protection and suspension
        // monitoring is not tracked (see the html README, owner-set
        // lifetime management).
        // Step 12.12: "Set inside settings's execution ready flag."
        // Note: Not implemented: the execution ready flag is not tracked.
        // Step 12.13: "If script is a classic script, then run the classic
        // script script. Otherwise, it is a module script; run the module
        // script script."
        // Note: Module workers are not implemented yet (fetch a module
        // worker script graph); a "module" worker is evaluated as a classic
        // script.
        let source = String::from_utf8_lossy(&response.body).into_owned();
        if let Err(error) = self.settings.evaluate_script(&source) {
            error!(
                "worker script {} failed to evaluate: {error}",
                self.worker_id
            );
            // Note: An uncaught top-level exception is a runtime script
            // error, not the step 12.4 fetch/parse-failure branch: per
            // report an exception (runtime script errors) it first fires an
            // error event at the worker global scope, and only when that is
            // unhandled fires an error event at the Worker object (step
            // 7.2), while the worker keeps running.  The worker-global error
            // event and ErrorEvent are not implemented; the Worker-object
            // event, fired directly (outside a task, as for step 12.4.1),
            // stands in for both, and the worker continues, matching
            // report-an-exception's non-aborting semantics.  A script parse
            // failure (the classic script's error to rethrow) should instead
            // have aborted the worker at step 12.4; the engine evaluation
            // combines parse and run, so the two are not distinguished here.
            if let Err(fire_error) = self.fire_worker_error() {
                error!("failed to fire error event at worker: {fire_error}");
            }
        }
        // Step 12.14: "Enable outside port's port message queue."
        // Note: The owner end of the worker's channel lives in the owner
        // realm; the owner event loop enables it there (its worker inbox
        // receives the enable request), flushing the messages the worker
        // posted before the queue was enabled.
        let _ = self.owner_inbox.send(WorkerEvent::EnableQueue {
            worker_id: self.worker_id,
        });
        // Step 12.15: "If is shared is false, enable the port message queue
        // of the worker's implicit port."
        // Note: Enables the worker end of the channel (its inbound message
        // queue), flushing the messages the owner posted before the queue
        // was enabled.
        with_dedicated_worker_global_scope(self.settings.ec(), |dedicated_scope, _ec| {
            dedicated_scope.enable_inbound_messages();
            Ok(())
        })
        .map_err(|error| format!("worker enable inbound messages: {}", error.display()))?;
        Ok(())
    }

    /// The failure branch of the worker's script fetch: run-a-worker step
    /// 12.4's steps run here when the script could not be obtained — a
    /// failed fetch, or a response that fails the executable check in
    /// complete_worker_script_fetch — firing an event named error at the
    /// Worker object (12.4.1) and discarding the worker's environment
    /// (12.4.2-12.4.3).
    fn fail_worker_script_fetch(&mut self) -> Result<(), String> {
        // Step 12.4's failure branch: the worker script could not be obtained.
        // Step 12.4.1: "Queue a global task on the DOM manipulation task
        // source given worker's relevant global object to fire an event named
        // error at worker."
        // Note: Fired directly, outside a task (see complete_worker_script_fetch).
        self.fire_worker_error()?;
        // Step 12.4.2: "Run the environment discarding steps for inside
        // settings."
        // Step 12.4.3: "Abort these steps."
        // Note: Setting the closing flag makes the worker's event loop exit
        // and the realm drop; the owner event loop runs the owner-side
        // cleanup when it handles the closed report.
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
            worker_global_scope.closing_flag.set(true);
            Ok(())
        })
        .map_err(|error| format!("failed to set worker closing flag: {}", error.display()))?;
        Ok(())
    }

    /// Fire an event named error at the Worker object of a failed or
    /// failed-to-evaluate worker, through the owner realm (the realm that
    /// created the Worker platform object).
    fn fire_worker_error(&mut self) -> Result<(), String> {
        self.owner_inbox
            .send(WorkerEvent::FireError {
                worker_id: self.worker_id,
            })
            .map_err(|error| format!("failed to report worker error event: {error}"))
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    fn run_steps_after_a_timeout(&mut self) {
        // The in-parallel steps of run steps after a timeout: the worker's
        // own event loop wakes on its own timer map's earliest expiry and
        // queues the expired timers' tasks on its own task queue (see the
        // content process's run_steps_after_a_timeout for the step notes).
        let expired = self.active_timers.borrow_mut().take_expired_timers();
        for timer in expired {
            match timer.realm {
                TimerRealm::Worker(worker_id) => {
                    self.task_queue.queue_a_task(Task::RunWorkerTimer {
                        worker_id,
                        timer_id: timer.timer_id,
                        timer_key: timer.timer_key,
                        nesting_level: timer.nesting_level,
                    });
                }
                TimerRealm::Document(document_id) => {
                    error!(
                        "worker {} timer map holds a document timer for {document_id}",
                        self.worker_id
                    );
                }
            }
        }
    }

    /// Run one task off the worker's event-loop task queue, then perform
    /// the microtask checkpoint of the event loop processing model (step
    /// 2.8), mirroring the content process's run_task for the worker
    /// agent's loop.
    fn run_task(&mut self, task: Task) -> Result<(), String> {
        let steps = match task {
            Task::RunPortMessage { port } => {
                // close-a-worker step 1 and terminate-a-worker step 2
                // discard the queued tasks of a closing worker; the closing
                // flag is checked here, at the task's start.
                if self.closing_flag() {
                    return Ok(());
                }
                self.handle_run_port_message_task(port)
            }
            Task::RunWorkerTimer {
                worker_id,
                timer_id,
                timer_key,
                nesting_level,
            } => {
                // Terminate-a-worker step 2 discards the queued tasks of a
                // closing worker; the closing flag is checked here, at the
                // task's start.
                if self.closing_flag() {
                    return Ok(());
                }
                if worker_id != self.worker_id {
                    return Err(format!(
                        "worker event loop received a timer for worker {worker_id}"
                    ));
                }
                self.settings
                    .run_window_timer(timer_id, timer_key, nesting_level)
            }
            Task::RunWorkerInboundMessage { worker_id, payload } => {
                // The message task of the worker's inbound channel: a
                // message the owner posted, fired as a message event at the
                // worker global scope (its inside port's message event
                // target).
                if self.closing_flag() {
                    return Ok(());
                }
                if worker_id != self.worker_id {
                    return Err(format!(
                        "worker event loop received an inbound message for worker {worker_id}"
                    ));
                }
                self.fire_inbound_worker_message(payload)
            }
            Task::RunWorkerOutboundMessage { worker_id, payload } => {
                // A message a worker this realm owns posted back: fire it as
                // a message event at the worker's Worker object (in this
                // realm, which is the owner of that worker).
                if self.closing_flag() {
                    return Ok(());
                }
                self.deliver_worker_outbound_message(worker_id, payload)
            }
            Task::PortRouting { port, kind } => {
                // A user-agent port task forwarded from the content process
                // main thread; the closing flag is checked here, at the
                // task's start, as for the other task arms.
                if self.closing_flag() {
                    return Ok(());
                }
                self.handle_worker_port_task(port, kind)
            }

            _ => Err(format!(
                "worker {} event loop received a document task",
                self.worker_id
            )),
        };
        steps?;
        // Step 2.8 of the event loop processing model: "Perform a microtask
        // checkpoint."
        // Note: `run_window_timer` performs its own checkpoint, so timer
        // tasks checkpoint twice; the extra checkpoint after an empty
        // microtask queue is a no-op.
        self.settings.perform_a_microtask_checkpoint()
    }

    /// The message task of one message the owner posted to this worker: run
    /// the delivery steps (deserialize, fire a message event at the worker
    /// global scope) for the message.
    fn fire_inbound_worker_message(&mut self, payload: WorkerChannelMessage) -> Result<(), String> {
        let time_millis = self.settings.current_time_millis();
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            // The message event target of the worker's inside port is the
            // worker global scope itself.
            let target = worker_global_scope.event_target.clone();
            deliver_serialized_message(&target, &payload, time_millis, ec)
        })
        .map_err(|error| format!("worker inbound message task failed: {}", error.display()))
    }

    /// The message task of one message a dedicated worker this realm owns
    /// posted back: run the delivery steps at the worker's Worker platform
    /// object (this realm is the owner).
    fn deliver_worker_outbound_message(
        &mut self,
        worker_id: WorkerId,
        payload: WorkerChannelMessage,
    ) -> Result<(), String> {
        fire_worker_posted_message(&mut self.settings, worker_id, payload)
    }

    /// Receive a message the owner posted to this worker over the owner→
    /// worker channel: the owner's message is either queued as a message
    /// task (the worker's message queue is enabled) or waits in the queue
    /// until the queue is enabled (run-a-worker step 12.15, or the first
    /// onmessage handler).  Runs on the agent's event-loop select.
    fn handle_owner_message(&mut self, inbound: WorkerInbound) -> Result<(), String> {
        if self.closing_flag() {
            return Ok(());
        }
        match inbound {
            WorkerInbound::Message(payload) => {
                with_dedicated_worker_global_scope(self.settings.ec(), |dedicated_scope, _ec| {
                    dedicated_scope.enqueue_inbound_message(payload);
                    Ok(())
                })
                .map_err(|error| format!("worker inbound message failed: {}", error.display()))
            }
            WorkerInbound::Terminate => {
                // <https://html.spec.whatwg.org/#terminate-a-worker>
                // Step 1: "Set the worker's WorkerGlobalScope object's
                // closing flag to true."
                // Step 2: "If there are any tasks queued in the
                // WorkerGlobalScope object's relevant agent's event loop's
                // task queues, discard them without processing them."
                // Note: The closing flag makes the event loop exit (its
                // queued tasks drop with the queue); the teardown report
                // follows.
                // Step 3: "Abort the script currently running in the
                // worker."
                // Note: Terminate is processed by the agent's event loop
                // between tasks, so it does not abort a script mid-
                // evaluation: a worker stuck in an unbounded top-level or
                // handler script cannot be terminated until it yields.
                // Step 4: "If the worker's WorkerGlobalScope object is
                // actually a DedicatedWorkerGlobalScope object ..., then
                // empty the port message queue of the port that the worker's
                // implicit port is entangled with."
                // Note: Runs in the owner event loop when it handles this
                // worker's closed report, dropping the owner end of the
                // worker's channel.
                with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
                    worker_global_scope.closing_flag.set(true);
                    Ok(())
                })
                .map_err(|error| format!("worker terminate: {}", error.display()))
            }
        }
    }

    /// Handle an event from a worker this realm owns (a nested worker) on
    /// this agent's worker inbox.
    fn handle_inbox_event(&mut self, event: WorkerEvent) -> Result<(), String> {
        match event {
            WorkerEvent::NewWorker {
                worker_id,
                owner,
                owner_to_worker,
                join_handle,
            } => {
                if owner != WorkerOwner::Worker(self.worker_id) {
                    return Err(format!(
                        "worker {} inbox received a NewWorker for owner {owner:?}",
                        self.worker_id
                    ));
                }
                // A worker this realm owns was spawned; its handle joins this
                // agent's records: the owner→worker channel end used to
                // terminate it when this agent closes, and its thread, joined
                // after the terminate (see terminate_and_join_nested_workers).
                self.nested_workers.insert(
                    worker_id,
                    WorkerHandle {
                        worker_id,
                        owner,
                        owner_to_worker,
                        join_handle: Some(join_handle),
                    },
                );
                Ok(())
            }
            WorkerEvent::Message { worker_id, payload } => {
                // A message a nested worker this realm owns posted back over
                // its owner inbox: queue it as a message task at the
                // worker's Worker object (or buffer it until that worker's
                // queue is enabled), in this realm.
                if self.closing_flag() {
                    return Ok(());
                }
                with_global_scope(self.settings.ec(), |global_scope, ec| {
                    global_scope.handle_worker_posted_message(worker_id, payload, ec);
                    Ok(())
                })
                .map_err(|error| {
                    format!(
                        "nested worker {} message failed: {}",
                        worker_id,
                        error.display()
                    )
                })
            }
            WorkerEvent::EnableQueue { worker_id } => {
                // run-a-worker step 12.14, owner half: enable the delivery of
                // the nested worker's posted messages in this realm.
                with_global_scope(self.settings.ec(), |global_scope, ec| {
                    global_scope.enable_owned_worker_messages(worker_id, ec);
                    Ok(())
                })
                .map_err(|error| {
                    format!(
                        "failed to enable nested worker {worker_id} messages: {}",
                        error.display()
                    )
                })
            }
            WorkerEvent::FireError { worker_id } => {
                // run-a-worker step 12.4.1: fire an event named error at the
                // nested worker's Worker object in this realm.
                let time_millis = self.settings.current_time_millis();
                with_global_scope(self.settings.ec(), |global_scope, ec| {
                    let Some(target) = global_scope.owned_worker_event_target(worker_id, ec) else {
                        return Ok(());
                    };
                    fire_event(ec, &target, "error", time_millis, true)
                        .map(|_| ())
                        .map_err(|error| {
                            ec.new_type_error(&format!(
                                "failed to fire worker error event: {error:?}"
                            ))
                        })?;
                    Ok(())
                })
                .map_err(|error| {
                    format!(
                        "failed to fire nested worker {worker_id} error: {}",
                        error.display()
                    )
                })
            }
            WorkerEvent::Closed { worker_id } => {
                // A nested worker this realm owns closed: join its thread and
                // drop the owner end of its channel in this realm
                // (terminate-a-worker step 4 and run-a-worker step 12.20).
                let Some(mut handle) = self.nested_workers.remove(&worker_id) else {
                    return Ok(());
                };
                if let Some(join_handle) = handle.join_handle.take()
                    && let Err(panic) = join_handle.join()
                {
                    error!("worker {} thread panicked: {panic:?}", self.worker_id);
                }
                with_global_scope(self.settings.ec(), |global_scope, ec| {
                    global_scope.discard_owned_worker(worker_id, ec);
                    Ok(())
                })
                .map_err(|error| {
                    format!(
                        "failed to discard closed nested worker {worker_id}: {}",
                        error.display()
                    )
                })?;
                Ok(())
            }
        }
    }

    /// Handle a command the user agent sent this dedicated worker agent
    /// directly, over the agent's own UA channel (its worker event loop's
    /// channel).
    fn handle_ua_command(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::PortTask { port, task } => {
                // A user-agent port task routed to a port of this realm's
                // event loop: queue it as a routing task on this agent's
                // event loop, mirroring how the main loop handles
                // Command::PortTask (Task::PortRouting), so the delivery runs
                // through the processing-model steps (task + microtask
                // checkpoint).  When this realm does not hold the port the
                // routing task is a no-op.
                self.task_queue
                    .queue_a_task(Task::PortRouting { port, kind: task });
                Ok(())
            }
            other => {
                // The window-agent commands (document lifecycle, viewport,
                // rendering) have no counterpart on a worker event loop; a
                // worker command channel receives only the commands the
                // user agent routes to a worker agent's event loop.
                error!(
                    "worker {} received unexpected UA command: {other:?}",
                    self.worker_id
                );
                Ok(())
            }
        }
    }

    /// Deliver a port task the user agent routed to a port of this realm's
    /// event loop, sent over the agent's own UA command channel.  Mirrors
    /// the main loop's handling for this realm: the task is appended to the
    /// port's queue, or the message task (the message port post message
    /// steps 7.4-7.7) fires in this task's slot when the port's queue is
    /// enabled.  A port this realm does not hold is ignored.
    fn handle_worker_port_task(
        &mut self,
        port_id: PortId,
        task: PortTaskKind,
    ) -> Result<(), String> {
        let fire = with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            let Some(messaging) = worker_global_scope.global_scope.channel_messaging(ec) else {
                return Ok(false);
            };
            let Some(event_sender) = worker_global_scope.global_scope.event_sender() else {
                return Ok(false);
            };
            messaging
                .handle_port_task(port_id, task, &event_sender, ec)
                .map_err(|error| ec.new_type_error(&format!("port task: {error}")))
        })
        .map_err(|error| format!("port task failed: {}", error.display()))?;
        if fire {
            self.handle_run_port_message_task(port_id)?;
        }
        Ok(())
    }

    /// Fire one queued message event on a port of this worker's event loop
    /// (the message task of the message port post message steps).
    fn handle_run_port_message_task(&mut self, port_id: PortId) -> Result<(), String> {
        let time_millis = self.settings.current_time_millis();
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            let Some(messaging) = worker_global_scope.global_scope.channel_messaging(ec) else {
                return Ok(());
            };
            let Some(port) = messaging.port_object(port_id, ec) else {
                return Ok(());
            };
            port.run_message_task(time_millis, ec)
        })
        .map_err(|error| format!("port message task failed: {}", error.display()))?;
        Ok(())
    }

    /// Handle a net command on the worker's own channel to the net process:
    /// the completion (or failure) of the worker's script fetch.
    fn handle_net_command(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::CompleteDocumentFetch {
                handler_id,
                response,
            } => {
                if self.pending_script_fetch == Some(handler_id) {
                    self.pending_script_fetch = None;
                    self.complete_worker_script_fetch(response)?;
                }
                Ok(())
            }
            Command::FailDocumentFetch { handler_id } => {
                if self.pending_script_fetch == Some(handler_id) {
                    self.pending_script_fetch = None;
                    self.fail_worker_script_fetch()?;
                }
                Ok(())
            }
            other => {
                error!(
                    "worker {} received unexpected net command: {other:?}",
                    self.worker_id
                );
                Ok(())
            }
        }
    }
}

/// <https://html.spec.whatwg.org/#event-loop-processing-model>
fn run_worker_event_loop(
    state: &mut DedicatedWorkerAgentState,
    net_command_rx: &crossbeam_channel::Receiver<ipc::IpcIncoming<Command>>,
    ua_command_rx: &crossbeam_channel::Receiver<ipc::IpcIncoming<Command>>,
) -> Result<(), String> {
    // Step 12.18 of run a worker: the responsible event loop specified by
    // inside settings, running until it is destroyed (the closing flag is
    // set).  The loop waits here for the next input: the oldest task on the
    // worker's task queue, a command from the user agent (over the agent's
    // own direct UA channel), a net command (the script fetch completion), a
    // message the owner posted over the worker's inbound channel, an event
    // from a worker this realm owns on the agent's worker inbox, or the
    // earliest expiry time in the worker's map of active timers.  One task
    // runs per iteration, so a task that queues another task does not starve
    // the other inputs.
    loop {
        // A script that ran before the loop (e.g. a data: worker's script,
        // evaluated while the fetch completed) may already have set the
        // closing flag (close() or a failed fetch); exit without waiting.
        if state.closing_flag() {
            return Ok(());
        }
        let timer_expiry = match state.active_timers.borrow().earliest_expiry_wait() {
            Some(wait) => crossbeam_channel::after(wait),
            None => crossbeam_channel::never(),
        };
        let task_queue = state.task_queue.receiver();
        // The owner→worker end of this worker's channel and this agent's
        // worker inbox (the events of its nested workers).
        let owner_to_worker = state.owner_to_worker.clone();
        let inbox = state.inbox.clone();

        let mut select = crossbeam_channel::Select::new();
        let task_arm = select.recv(&task_queue);
        let net_arm = select.recv(net_command_rx);
        let ua_arm = select.recv(ua_command_rx);
        let timer_arm = select.recv(&timer_expiry);
        let owner_arm = select.recv(&owner_to_worker);
        let inbox_arm = select.recv(&inbox);

        let operation = select.select();
        let arm = operation.index();
        if arm == task_arm {
            match operation.recv(&task_queue) {
                Ok(oldest_task) => {
                    if let Err(error) = state.run_task(oldest_task) {
                        error!("worker {} task error: {error}", state.worker_id);
                    }
                }
                Err(_) => return Ok(()),
            }
        } else if arm == net_arm {
            if let Ok(incoming) = operation.recv(net_command_rx)
                && let Err(error) = state.handle_net_command(incoming.payload)
            {
                error!("worker {} net command error: {error}", state.worker_id);
            }
        } else if arm == ua_arm {
            if let Ok(incoming) = operation.recv(ua_command_rx)
                && let Err(error) = state.handle_ua_command(incoming.payload)
            {
                error!("worker {} UA command error: {error}", state.worker_id);
            }
        } else if arm == owner_arm {
            if let Ok(inbound) = operation.recv(&owner_to_worker)
                && let Err(error) = state.handle_owner_message(inbound)
            {
                error!("worker {} inbound message error: {error}", state.worker_id);
            }
        } else if arm == inbox_arm {
            if let Ok(event) = operation.recv(&inbox)
                && let Err(error) = state.handle_inbox_event(event)
            {
                error!("worker {} inbox event error: {error}", state.worker_id);
            }
        } else if arm == timer_arm && operation.recv(&timer_expiry).is_ok() {
            state.run_steps_after_a_timeout();
        }

        if state.closing_flag() {
            return Ok(());
        }
    }
}

/// The message task of one message a dedicated worker posted back to its
/// owner: fire a message event at the worker's Worker platform object in
/// the realm that created the worker (the owner window document, or the
/// owner worker global scope).  The delivery steps (message port post
/// message steps 7.4-7.7) run in deliver_serialized_message (messageport.rs).
pub(crate) fn fire_worker_posted_message(
    settings: &mut EnvironmentSettingsObject,
    worker_id: WorkerId,
    payload: WorkerChannelMessage,
) -> Result<(), String> {
    let time_millis = settings.current_time_millis();
    with_global_scope(settings.ec(), |global_scope, ec| {
        let Some(target) = global_scope.owned_worker_event_target(worker_id, ec) else {
            return Ok(());
        };
        deliver_serialized_message(&target, &payload, time_millis, ec)
    })
    .map_err(|error| format!("worker outbound message task failed: {}", error.display()))
}
