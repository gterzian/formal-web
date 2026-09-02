//! Runs run-a-worker
//! (<https://html.spec.whatwg.org/#run-a-worker>) for one dedicated worker
//! on its own native thread: its own realm (engine), event loop, task queue,
//! and timer map, driven from the content process main thread over a command
//! channel.  The worker's channel to its owner is two direct crossbeam
//! channel ends that replace the spec's implicit MessagePort pair — see the
//! platform objects in `worker.rs` and the `// Note:` comments on the
//! channel types below.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use data_url::DataUrl;
use ipc::IpcSender;
use ipc_messages::content::{
    Command, DocumentFetchId, Event as ContentEvent, EventLoopId,
    FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse, PortId, WorkerId,
    WorkerOwner, WorkerRequest,
};
use ipc_messages::network::{Request as NetworkRequest, ResponseRecipient};
use log::error;
use url::Url;

use crate::dom::event::EventTarget;
use crate::dom::fire_event;
use crate::html::environment_settings_object::{EnvironmentSettingsObject, WorkerRealmWiring};
use crate::html::event_loop::{EventLoopTaskSources, Task, TaskQueue};
use crate::html::messageport::deliver_serialized_message;
use crate::html::structured_data::safe_passing_of_structured_data::SerializeWithTransferResult;
use crate::html::timers::{MapOfActiveTimers, TimerRealm};
use crate::html::worker::WorkerType;
use crate::js::platform_objects::{with_global_scope, with_worker_global_scope};
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

/// The owner end of one dedicated worker's channel, registered on the owner
/// realm's global scope by the Worker constructor (and dropped when the
/// worker closes): the message event target the messages the worker posts
/// back fire at — the worker's Worker object, the role the outside port's
/// message event target plays in the spec — and the owner-side message queue
/// gating their delivery.
/// Note: The worker's implicit port pair is bypassed (see `worker.rs`); this
/// record replaces the outside port's record of the port-based model.
#[gc_struct]
pub(crate) struct OwnedWorkerChannel {
    /// The Worker platform object's event target: the message event target
    /// of the messages the worker posts (the role the outside port's message
    /// event target played).
    pub(crate) worker_target: EventTarget,

    /// The worker this channel belongs to.
    #[ignore_trace]
    pub(crate) worker_id: WorkerId,

    /// The message queue of the owner end of the worker's channel: whether
    /// the messages the worker posts may fire as message events, and the
    /// messages that arrived before the queue was enabled.
    #[ignore_trace]
    pub(crate) queue: Rc<RefCell<WorkerMessageQueue>>,
}

/// A request from a realm's GlobalScope to the content process's worker
/// manager.  Dedicated workers are entirely content-process-nested: worker
/// creation and termination never involve the user agent.
pub(crate) enum WorkerContentRequest {
    /// The Worker constructor's run-a-worker request.
    Create(WorkerStartRequest),
    /// A Worker object's terminate().
    Terminate(WorkerId),
}

/// The Worker constructor's run-a-worker request plus the ends of the
/// worker's direct channels the constructor created in the owner realm.  The
/// sender ends stay with the platform objects in the owner realm; the ends
/// that travel here are handed to the dedicated worker agent's event loop
/// and to the owner's event loop.
pub(crate) struct WorkerStartRequest {
    /// <https://html.spec.whatwg.org/#run-a-worker>
    pub(crate) request: WorkerRequest,
    /// The receiver end of the owner→worker channel (the messages
    /// `worker.postMessage` sends): the dedicated worker agent's event loop
    /// selects on it.
    pub(crate) owner_to_worker: crossbeam_channel::Receiver<WorkerChannelMessage>,
    /// The sender end of the worker→owner channel: handed to the worker
    /// global scope for the worker's `postMessage`.
    pub(crate) worker_to_owner: crossbeam_channel::Sender<WorkerChannelMessage>,
    /// The receiver end of the worker→owner channel: kept by the owner's
    /// event loop (the content process main loop for a document owner, or
    /// forwarded to the owner worker agent's event loop).
    pub(crate) worker_to_owner_rx: crossbeam_channel::Receiver<WorkerChannelMessage>,
}

/// A command from the content process main thread to a dedicated worker
/// agent.
pub(crate) enum WorkerCommand {
    /// <https://html.spec.whatwg.org/#terminate-a-worker>
    /// Set the closing flag and discard the queued tasks; the event loop
    /// then exits and the thread reports its teardown.
    Terminate,
    /// An owner-side operation to run in this worker's realm (this worker is
    /// the owner of another worker).
    OwnerOperation(OwnerOperation),
    /// This worker (the owner of a nested worker) receives the receiver end
    /// of the nested worker's outbound channel; it joins this agent's
    /// event-loop select so messages the nested worker posts back fire as
    /// message events at the nested worker's Worker object in this realm.
    AddNestedWorkerChannel {
        worker_id: WorkerId,
        receiver: crossbeam_channel::Receiver<WorkerChannelMessage>,
    },
    /// A nested worker this realm owns closed; drop its outbound channel's
    /// receiver from this agent's event-loop select.
    RemoveNestedWorkerChannel { worker_id: WorkerId },
}

/// An operation to run in the realm that owns a worker (the owner document,
/// or the owner worker global scope).
#[derive(Clone, Copy, Debug)]
pub(crate) enum OwnerOperation {
    /// run-a-worker step 12.14, owner half: enable the delivery of the
    /// messages the worker posts (the owner end of the worker's channel), so
    /// messages that arrived while the queue was disabled fire as tasks.
    /// <https://html.spec.whatwg.org/#run-a-worker>
    EnableWorkerMessages { worker_id: WorkerId },
    /// run-a-worker step 12.4.1: fire an event named error at the Worker
    /// object of a failed or failed-to-evaluate worker.
    /// <https://html.spec.whatwg.org/#run-a-worker>
    FireWorkerError { worker_id: WorkerId },
    /// terminate-a-worker step 4 and run-a-worker step 12.20: the worker is
    /// gone, so empty the owner end of its channel (its pending messages) and
    /// drop its delivery state.
    /// <https://html.spec.whatwg.org/#terminate-a-worker>
    DiscardWorkerMessages { worker_id: WorkerId },
}

/// A notification from a dedicated worker agent to the content process main
/// thread.
pub(crate) enum WorkerEvent {
    /// The dedicated worker agent is exiting (its realm was torn down).
    Closed { worker_id: WorkerId },
    /// The worker needs an operation run in its owner realm.
    OwnerOperation {
        owner: WorkerOwner,
        operation: OwnerOperation,
    },
}

/// Everything the content process main thread hands a new dedicated worker
/// agent's thread.
pub(crate) struct DedicatedWorkerAgentConfig {
    pub(crate) request: WorkerRequest,
    /// The receiver end of the owner→worker channel (the messages
    /// `worker.postMessage` sends): the agent's event loop selects on it and
    /// delivers each message as a message event at the worker global scope.
    pub(crate) owner_to_worker: crossbeam_channel::Receiver<WorkerChannelMessage>,
    /// The sender end of the worker→owner channel, stored on the worker
    /// global scope for the worker's `postMessage`.
    pub(crate) worker_to_owner: crossbeam_channel::Sender<WorkerChannelMessage>,
    /// The content process's event loop id, shared by the worker realm's
    /// channel messaging (a worker realm has no event loop of its own on the
    /// user-agent side).
    pub(crate) event_loop_id: EventLoopId,
    /// The worker thread reports its teardown and owner-side operations on
    /// this channel.
    pub(crate) worker_events: crossbeam_channel::Sender<WorkerEvent>,
    /// Commands from the content process main thread.
    pub(crate) worker_commands: crossbeam_channel::Receiver<WorkerCommand>,
    /// The content-to-user-agent event sender, wired into the worker realm's
    /// global scope for its channel messaging.
    pub(crate) event_sender: IpcSender<ContentEvent>,
    /// The worker's own channel to the net process, for script fetches.
    pub(crate) network_extension_sender: IpcSender<ipc_messages::network::Request>,
    /// The channel to the content process's worker manager (nested workers).
    pub(crate) worker_creator: crossbeam_channel::Sender<WorkerContentRequest>,
    pub(crate) trace_sender: Option<TraceSender>,
}

/// The run-a-worker state owned by a dedicated worker agent (running on its
/// native thread).
pub(crate) struct DedicatedWorkerAgentState {
    pub(crate) worker_id: WorkerId,
    pub(crate) settings: EnvironmentSettingsObject,
    pub(crate) owner: WorkerOwner,
    /// The owner→worker end of the worker's channel: the messages
    /// `worker.postMessage` sends, delivered as message events at the worker
    /// global scope once its message queue is enabled.
    pub(crate) owner_to_worker: crossbeam_channel::Receiver<WorkerChannelMessage>,
    /// The receiver ends of the outbound channels of the workers this
    /// worker owns (nested workers), keyed by worker id: the messages each
    /// nested worker posts back fire as message events at its Worker
    /// object in this worker's realm.
    pub(crate) nested_workers: HashMap<WorkerId, crossbeam_channel::Receiver<WorkerChannelMessage>>,
    /// The worker's own task queue and timer map: the dedicated worker
    /// agent's event loop task sources (a dedicated worker agent has its own
    /// event loop).
    pub(crate) task_queue: TaskQueue,
    pub(crate) active_timers: Rc<RefCell<MapOfActiveTimers>>,
    pub(crate) worker_events: crossbeam_channel::Sender<WorkerEvent>,
    pub(crate) event_loop_id: EventLoopId,
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
    let worker_id = config.request.worker_id;
    let worker_events = config.worker_events.clone();
    let result = run_a_worker_inner(config);
    // The dedicated worker agent always reports its teardown (also on early
    // failure, so the content process can join its thread and run the
    // owner-side cleanup).
    let _ = worker_events.send(WorkerEvent::Closed { worker_id });
    result
}

fn run_a_worker_inner(config: DedicatedWorkerAgentConfig) -> Result<(), String> {
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
    // Note: This thread IS the dedicated worker agent: with is shared false,
    // obtaining the agent never creates a new agent cluster, so the agent is
    // nested to the content process hosting the owner realm (the same agent
    // cluster as its owner's similar-origin window agent).  Create an agent
    // (canBlock true) is realized by the native thread, whose event loop
    // below is the agent's event loop.
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
    // The worker realm's global scope shares the content process's event
    // loop id, gets the trace sender, and the worker creator channel (a
    // nested worker's constructor runs on this thread).  It also gets the
    // worker→owner end of the worker's channel, so the worker's postMessage
    // can reach its owner.
    with_worker_global_scope(settings.ec(), |worker_global_scope, _ec| {
        worker_global_scope
            .global_scope
            .set_event_loop_id(config.event_loop_id);
        worker_global_scope
            .global_scope
            .set_trace_sender(config.trace_sender.clone());
        worker_global_scope
            .global_scope
            .set_worker_creator(config.worker_creator.clone());
        worker_global_scope.set_inside_port(config.worker_to_owner);
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
        owner,
        owner_to_worker: config.owner_to_worker,
        nested_workers: HashMap::new(),
        task_queue,
        active_timers,
        worker_events: config.worker_events,
        event_loop_id: config.event_loop_id,
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
    run_worker_event_loop(&mut state, &net_command_rx, &config.worker_commands)?;

    // Step 12.19: "Clear the worker global scope's map of active timers."
    // Step 12.20: "Disentangle all the ports in the list of the worker's
    // ports."
    // Step 12.21: "Empty worker global scope's owner set."
    // Note: The timer map, the worker's inbound channel receiver and the
    // realm drop with this state when the function returns; the owner-side
    // cleanup (empty the owner end of the worker's channel, terminate a
    // worker step 4) runs in the content process when it handles the Closed
    // report.
    Ok(())
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

    /// <https://html.spec.whatwg.org/#run-a-worker>
    fn start_script_fetch(&mut self, script_url: String) -> Result<(), String> {
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
            event_loop_id: self.event_loop_id,
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

    /// <https://html.spec.whatwg.org/#run-a-worker>
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
        // Note: The association is the ContentWorker entry in the content
        // process (created when the dedicated worker agent's thread was
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
        // created the two ends of the worker's direct channel in the owner
        // realm, this agent's event loop selects on the owner→worker end
        // (its receiver is in the agent state), and the worker global scope
        // holds the worker→owner end.  There is no inside port, no
        // entanglement, and no channel to register with the user agent; the
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
            // Step 12.4.1: "Queue a global task on the DOM manipulation task
            // source given worker's relevant global object to fire an event
            // named error at worker."
            // Note: Fired directly, outside a task (the document lifecycle
            // commands share this deviation; see the content README).
            if let Err(fire_error) = self.fire_worker_error() {
                error!("failed to fire error event at worker: {fire_error}");
            }
        }
        // Step 12.14: "Enable outside port's port message queue."
        // Note: The owner end of the worker's channel lives in the owner
        // realm; the content process enables it there (forwarding to the
        // owner worker thread when the owner is a worker), flushing the
        // messages the worker posted before the queue was enabled.
        if let Err(error) = self.worker_events.send(WorkerEvent::OwnerOperation {
            owner: self.owner,
            operation: OwnerOperation::EnableWorkerMessages {
                worker_id: self.worker_id,
            },
        }) {
            error!(
                "worker {}: failed to report queue enable: {error}",
                self.worker_id
            );
        }
        // Step 12.15: "If is shared is false, enable the port message queue
        // of the worker's implicit port."
        // Note: Enables the worker end of the channel (its inbound message
        // queue), flushing the messages the owner posted before the queue
        // was enabled.
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
            worker_global_scope.enable_inbound_messages();
            Ok(())
        })
        .map_err(|error| format!("worker enable inbound messages: {}", error.display()))?;
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#run-a-worker>
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
        // and the realm drop; the content process runs the owner-side
        // cleanup when it handles the Closed report.
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
        self.worker_events
            .send(WorkerEvent::OwnerOperation {
                owner: self.owner,
                operation: OwnerOperation::FireWorkerError {
                    worker_id: self.worker_id,
                },
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

    /// Run one task off the worker's event-loop task queue.
    /// <https://html.spec.whatwg.org/#event-loop-processing-model>
    fn run_task(&mut self, task: Task) -> Result<(), String> {
        match task {
            Task::RunPortMessage { port } => self.handle_run_port_message_task(port),
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
                // worker global scope (its implicit port's message event
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
            _ => Err(format!(
                "worker {} event loop received a document task",
                self.worker_id
            )),
        }
    }

    /// The message task of one message the owner posted to this worker: run
    /// the delivery steps (deserialize, fire a message event at the worker
    /// global scope) for the message.
    fn fire_inbound_worker_message(&mut self, payload: WorkerChannelMessage) -> Result<(), String> {
        let time_millis = self.settings.current_time_millis();
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            // The message event target of the worker's implicit port is the
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
    fn handle_owner_to_worker_message(
        &mut self,
        payload: WorkerChannelMessage,
    ) -> Result<(), String> {
        if self.closing_flag() {
            return Ok(());
        }
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
            worker_global_scope.enqueue_inbound_message(payload);
            Ok(())
        })
        .map_err(|error| format!("worker inbound message failed: {}", error.display()))
    }

    /// Receive a message a nested worker this realm owns posted back over
    /// its outbound channel: queue it as a message task at the worker's
    /// Worker object (or buffer it until that worker's queue is enabled).
    /// Runs on the agent's event-loop select.
    fn handle_nested_worker_message(
        &mut self,
        worker_id: WorkerId,
        payload: WorkerChannelMessage,
    ) -> Result<(), String> {
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

    /// Handle a command from the content process main thread.
    fn handle_worker_command(&mut self, command: WorkerCommand) -> Result<(), String> {
        match command {
            WorkerCommand::Terminate => {
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
                // Note: Not applicable: terminate arrives as a command
                // between tasks, so no worker script is running when it is
                // processed.
                // Step 4: "If the worker's WorkerGlobalScope object is
                // actually a DedicatedWorkerGlobalScope object ..., then
                // empty the port message queue of the port that the worker's
                // implicit port is entangled with."
                // Note: Runs in the content process when it handles the
                // Closed report (`DiscardWorkerMessages`, which drops the
                // owner end of the worker's channel).
                with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
                    worker_global_scope.closing_flag.set(true);
                    Ok(())
                })
                .map_err(|error| format!("worker terminate: {}", error.display()))?;
                Ok(())
            }
            WorkerCommand::OwnerOperation(operation) => {
                // This worker is the owner of another worker; run the
                // operation in this worker's realm.
                execute_owner_operation(&mut self.settings, operation)
            }
            WorkerCommand::AddNestedWorkerChannel {
                worker_id,
                receiver,
            } => {
                // A worker this realm owns was spawned; its outbound
                // channel's receiver joins this agent's event-loop select.
                self.nested_workers.insert(worker_id, receiver);
                Ok(())
            }
            WorkerCommand::RemoveNestedWorkerChannel { worker_id } => {
                // A worker this realm owns closed; drop its outbound
                // channel's receiver from the select.
                self.nested_workers.remove(&worker_id);
                Ok(())
            }
        }
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
    worker_command_rx: &crossbeam_channel::Receiver<WorkerCommand>,
) -> Result<(), String> {
    // Step 12.18 of run a worker: the responsible event loop specified by
    // inside settings, running until it is destroyed (the closing flag is
    // set).  The loop waits here for the next input: the oldest task on the
    // worker's task queue, a command from the content process main thread, a
    // net command (the script fetch completion), a message the owner posted
    // over the worker's inbound channel, a message a nested worker posted
    // over its outbound channel, or the earliest expiry time in the worker's
    // map of active timers.  One task runs per iteration, so a task that
    // queues another task does not starve the other inputs.
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
        // The channel ends that join the select: the owner→worker end of
        // this worker's channel, and the outbound-channel ends of the
        // workers this worker owns (each message identifies its worker by
        // the receiver it arrived on).
        let owner_to_worker = state.owner_to_worker.clone();
        let nested_workers: Vec<(WorkerId, crossbeam_channel::Receiver<WorkerChannelMessage>)> =
            state
                .nested_workers
                .iter()
                .map(|(worker_id, receiver)| (*worker_id, receiver.clone()))
                .collect();

        let mut select = crossbeam_channel::Select::new();
        let task_arm = select.recv(&task_queue);
        let worker_command_arm = select.recv(worker_command_rx);
        let net_arm = select.recv(net_command_rx);
        let timer_arm = select.recv(&timer_expiry);
        let owner_arm = select.recv(&owner_to_worker);
        let nested_arms: Vec<(WorkerId, usize)> = nested_workers
            .iter()
            .map(|(worker_id, receiver)| (*worker_id, select.recv(receiver)))
            .collect();

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
        } else if arm == worker_command_arm {
            match operation.recv(worker_command_rx) {
                Ok(command) => {
                    if let Err(error) = state.handle_worker_command(command) {
                        error!("worker {} command error: {error}", state.worker_id);
                    }
                }
                // The main thread dropped its command sender (the process is
                // shutting down); the worker exits.
                Err(_) => return Ok(()),
            }
        } else if arm == net_arm {
            if let Ok(incoming) = operation.recv(net_command_rx)
                && let Err(error) = state.handle_net_command(incoming.payload)
            {
                error!("worker {} net command error: {error}", state.worker_id);
            }
        } else if arm == owner_arm {
            if let Ok(payload) = operation.recv(&owner_to_worker)
                && let Err(error) = state.handle_owner_to_worker_message(payload)
            {
                error!("worker {} inbound message error: {error}", state.worker_id);
            }
        } else if let Some((worker_id, _)) = nested_arms.iter().find(|(_, index)| arm == *index) {
            if let Some(receiver) = nested_workers
                .iter()
                .find(|(nested_id, _)| nested_id == worker_id)
                .map(|(_, receiver)| receiver)
                && let Ok(payload) = operation.recv(receiver)
                && let Err(error) = state.handle_nested_worker_message(*worker_id, payload)
            {
                error!(
                    "worker {} nested worker {} message error: {error}",
                    state.worker_id, worker_id
                );
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
/// owner: fire a message event at the worker's Worker platform object, in
/// the realm of the given settings (the realm that created the worker: the
/// owner window document, or the owner worker global scope).  A worker this
/// realm no longer owns (it closed) drops the message.
/// <https://html.spec.whatwg.org/#message-port-post-message-steps>
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

/// Run an owner-side operation in the given realm (the realm that created a
/// worker: a window document or a worker global scope).
pub(crate) fn execute_owner_operation(
    settings: &mut EnvironmentSettingsObject,
    operation: OwnerOperation,
) -> Result<(), String> {
    let time_millis = settings.current_time_millis();
    with_global_scope(settings.ec(), |global_scope, ec| {
        match operation {
            OwnerOperation::EnableWorkerMessages { worker_id } => {
                // Step 12.14, owner half: enable the delivery of the
                // messages the worker posts, so the messages that arrived
                // while the queue was disabled fire as message tasks.
                global_scope.enable_owned_worker_messages(worker_id, ec);
            }
            OwnerOperation::FireWorkerError { worker_id } => {
                let Some(target) = global_scope.owned_worker_event_target(worker_id, ec) else {
                    return Ok(());
                };
                // The Worker object is the message event target of the
                // messages its worker posts (the owner end of the channel);
                // firing at it fires at the Worker.
                fire_event(ec, &target, "error", time_millis, true)
                    .map(|_| ())
                    .map_err(|error| {
                        ec.new_type_error(&format!("failed to fire worker error event: {error:?}"))
                    })?;
            }
            OwnerOperation::DiscardWorkerMessages { worker_id } => {
                // terminate-a-worker step 4 (empty the queue of the port the
                // worker's implicit port is entangled with) and run-a-worker
                // step 12.20 (disentangle the worker's ports): the owner end
                // of the worker's channel is dropped, discarding its
                // pending messages.
                global_scope.discard_owned_worker(worker_id, ec);
            }
        }
        Ok(())
    })
    .map_err(|error| format!("owner operation failed: {}", error.display()))
}
