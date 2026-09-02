//! A dedicated worker agent runs on its own native thread nested to the
//! content process hosting its owner realm: the dedicated worker agent is
//! always part of the same agent cluster as the window that created it
//! (obtain a dedicated/shared worker agent with `isShared` false never
//! creates a new agent cluster), so its thread lives inside that window
//! agent's process.  Create an agent (canBlock true) is realized by the
//! native thread, whose event loop is the worker's.
//!
//! The dedicated worker agent runs run-a-worker
//! (<https://html.spec.whatwg.org/#run-a-worker>) against its own realm and
//! event loop: it builds its own engine (its own V8 isolate), fetches its
//! script over its own IPC channel to the net process (the reply comes back
//! on that channel, bridged over crossbeam into the agent's event-loop
//! select, like the content process's own loop), and is driven from the
//! content process main thread through a crossbeam command channel that also
//! joins the select.
//!
//! The content process main thread (the similar-origin window agent) stores
//! each dedicated worker agent's thread data (its command channel and join
//! handle, joined on shutdown), routes user-agent port tasks for
//! worker-owned ports to the agent's thread, and runs owner-side steps (the
//! owner realm's entanglement, queue enablement, error events) in the realm
//! that created the worker.

use std::cell::RefCell;
use std::rc::Rc;

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

use crate::dom::fire_event;
use crate::html::environment_settings_object::{EnvironmentSettingsObject, WorkerRealmWiring};
use crate::html::event_loop::{EventLoopTaskSources, Task, TaskQueue};
use crate::html::messageport::MessagePort;
use crate::html::timers::{MapOfActiveTimers, TimerRealm};
use crate::html::worker::WorkerType;
use crate::js::platform_objects::{with_global_scope, with_worker_global_scope};

use verification::TraceSender;

/// A request from a realm's GlobalScope to the content process's worker
/// manager.  Dedicated workers are entirely content-process-nested: worker
/// creation and termination never involve the user agent.
pub(crate) enum WorkerContentRequest {
    /// The Worker constructor's run-a-worker request.
    Create(WorkerRequest),
    /// A Worker object's terminate().
    Terminate(WorkerId),
}

/// A command from the content process main thread to a dedicated worker
/// agent.
pub(crate) enum WorkerCommand {
    /// <https://html.spec.whatwg.org/#terminate-a-worker>
    /// Set the closing flag and discard the queued tasks; the event loop
    /// then exits and the thread reports its teardown.
    Terminate,
    /// A routed message task for a port managed by this worker's event loop.
    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    PortTask { port: PortId, kind: PortTaskKind },
    /// An owner-side operation to run in this worker's realm (this worker is
    /// the owner of another worker).
    OwnerOperation(OwnerOperation),
}

/// An operation to run in the realm that owns a worker (the owner document,
/// or the owner worker global scope).
#[derive(Clone, Copy, Debug)]
pub(crate) enum OwnerOperation {
    /// run-a-worker step 12.8, owner half: entangle the outside port with the
    /// worker's inside port in the owner realm's channel messaging.
    /// <https://html.spec.whatwg.org/#run-a-worker>
    EntangleOutsidePort {
        outside_port: PortId,
        inside_port: PortId,
    },
    /// run-a-worker step 12.14: enable the outside port's port message queue.
    /// <https://html.spec.whatwg.org/#run-a-worker>
    EnableOutsidePortQueue { outside_port: PortId },
    /// run-a-worker step 12.4.1: fire an event named error at the Worker
    /// object (the outside port's message event target).
    /// <https://html.spec.whatwg.org/#run-a-worker>
    FireWorkerError { outside_port: PortId },
    /// terminate-a-worker step 4 (empty the port message queue of the port
    /// the worker's implicit port is entangled with) and run-a-worker step
    /// 12.20 (disentangle all the ports in the list of the worker's ports):
    /// empty the outside port's queue and sever its entanglement.
    /// <https://html.spec.whatwg.org/#terminate-a-worker>
    EmptyAndDisentangleOutsidePort { outside_port: PortId },
}

/// A notification from a dedicated worker agent to the content process main
/// thread.
pub(crate) enum WorkerEvent {
    /// The dedicated worker agent is exiting (its realm was torn down).
    Closed { worker_id: WorkerId },
    /// The worker's channel messaging now manages a port (the main thread
    /// routes user-agent port tasks for it to the worker thread).
    PortRegistered { worker_id: WorkerId, port: PortId },
    /// The worker's channel messaging no longer manages a port.
    PortUnregistered { worker_id: WorkerId, port: PortId },
    /// The worker needs an operation run in its owner realm.
    OwnerOperation {
        owner: WorkerOwner,
        operation: OwnerOperation,
    },
}

/// A reporter that tells the content process main thread about the ports a
/// worker realm's channel messaging manages, so the main thread can route
/// user-agent port tasks to the owning worker thread.
#[derive(Clone)]
pub(crate) struct PortOwnerReporter {
    worker_id: WorkerId,
    sender: crossbeam_channel::Sender<WorkerEvent>,
}

impl PortOwnerReporter {
    pub(crate) fn new(worker_id: WorkerId, sender: crossbeam_channel::Sender<WorkerEvent>) -> Self {
        Self { worker_id, sender }
    }

    pub(crate) fn report(&self, port_id: PortId, registered: bool) {
        let event = if registered {
            WorkerEvent::PortRegistered {
                worker_id: self.worker_id,
                port: port_id,
            }
        } else {
            WorkerEvent::PortUnregistered {
                worker_id: self.worker_id,
                port: port_id,
            }
        };
        // A closed receiver means the content process is shutting down, the
        // same expected condition as a reply channel send.
        let _ = self.sender.send(event);
    }
}

/// Everything the content process main thread hands a new dedicated worker
/// agent's thread.
pub(crate) struct DedicatedWorkerAgentConfig {
    pub(crate) request: WorkerRequest,
    /// The content process's event loop id, shared by the worker's channel
    /// messaging: the user agent routes port tasks to this content process,
    /// which forwards them to the worker thread.
    pub(crate) event_loop_id: EventLoopId,
    /// The worker thread reports its teardown, its ports, and owner-side
    /// operations on this channel.
    pub(crate) worker_events: crossbeam_channel::Sender<WorkerEvent>,
    /// Commands from the content process main thread.
    pub(crate) worker_commands: crossbeam_channel::Receiver<WorkerCommand>,
    /// The content-to-user-agent event sender, for the worker's channel
    /// messaging (port routing) and the worker channel registration.
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
    pub(crate) outside_port_id: PortId,
    /// The worker's own task queue and timer map: the dedicated worker
    /// agent's event loop task sources (a dedicated worker agent has its own
    /// event loop).
    pub(crate) task_queue: TaskQueue,
    pub(crate) active_timers: Rc<RefCell<MapOfActiveTimers>>,
    pub(crate) event_sender: IpcSender<ContentEvent>,
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
    let outside_port_id = request.outside_port;
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
    // loop id for port routing, gets the trace sender for the MessagePort
    // specs, the worker creator channel (a nested worker's constructor runs
    // on this thread), and the port-owner reporter that tells the main
    // thread which ports this worker's event loop manages.
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
        worker_global_scope
            .global_scope
            .set_port_owner_reporter(Some(PortOwnerReporter::new(
                worker_id,
                config.worker_events.clone(),
            )));
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
        outside_port_id,
        task_queue,
        active_timers,
        event_sender: config.event_sender,
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
    // Note: The timer map and the inside port's record drop with the realm
    // (the settings object is dropped when this function returns); the
    // owner-side cleanup (empty and disentangle the outside port, terminate
    // a worker step 4) runs in the content process when it handles the
    // Closed report.
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
        let mut inside_port = MessagePort::new_port_with_id(PortId::new(), self.settings.ec())
            .map_err(|error| format!("failed to create inside port: {}", error.display()))?;
        // Step 12.7.1: "Set inside port's message event target to worker
        // global scope."
        let event_target =
            with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
                Ok(worker_global_scope.event_target.clone())
            })
            .map_err(|error| format!("failed to read worker event target: {}", error.display()))?;
        inside_port.set_message_event_target(event_target);
        // Step 12.7.2: "Set worker global scope's inside port to inside
        // port."
        let inside_port_id = inside_port.port_id;
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            worker_global_scope.set_inside_port(inside_port.clone(), ec);
            Ok(())
        })
        .map_err(|error| format!("failed to set worker inside port: {}", error.display()))?;
        // Step 12.8: "Entangle outside port and inside port."
        // Note: The worker half creates the inside port's record entangled
        // with the outside port; the owner half (the outside port's record,
        // in the owner realm) runs in the content process, which forwards it
        // to the owner's dedicated worker agent when the owner is a worker.
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            let messaging = worker_global_scope
                .global_scope
                .channel_messaging(ec)
                .ok_or_else(|| {
                    ec.new_type_error("worker entangle: worker realm has no channel messaging")
                })?;
            messaging.entangle_remote(inside_port.clone(), self.outside_port_id, ec);
            Ok(())
        })
        .map_err(|error| format!("worker entangle: {}", error.display()))?;
        let owner_operation = WorkerEvent::OwnerOperation {
            owner: self.owner,
            operation: OwnerOperation::EntangleOutsidePort {
                outside_port: self.outside_port_id,
                inside_port: inside_port_id,
            },
        };
        if let Err(error) = self.worker_events.send(owner_operation) {
            error!(
                "worker {}: failed to report entanglement: {error}",
                self.worker_id
            );
        }
        // The user agent must know both ports to route messages to either
        // one's owning event loop (`MessagePortExtraFG.tla`'s `NewChannel`).
        if let Err(error) = self.event_sender.send(ContentEvent::PortChannelCreated {
            port1: self.outside_port_id,
            port2: inside_port_id,
        }) {
            error!(
                "worker {}: failed to register worker channel: {error}",
                self.worker_id
            );
        }
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
        // Note: The outside port's record lives in the owner realm; the
        // content process enables it there (forwarding to the owner worker
        // thread when the owner is a worker).
        if let Err(error) = self.worker_events.send(WorkerEvent::OwnerOperation {
            owner: self.owner,
            operation: OwnerOperation::EnableOutsidePortQueue {
                outside_port: self.outside_port_id,
            },
        }) {
            error!(
                "worker {}: failed to report outside queue enable: {error}",
                self.worker_id
            );
        }
        // Step 12.15: "If is shared is false, enable the port message queue
        // of the worker's implicit port."
        with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            if let Some(messaging) = worker_global_scope.global_scope.channel_messaging(ec) {
                messaging.start(inside_port_id, ec);
            }
            Ok(())
        })
        .map_err(|error| format!("worker enable inside port: {}", error.display()))?;
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
                    outside_port: self.outside_port_id,
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
            _ => Err(format!(
                "worker {} event loop received a document task",
                self.worker_id
            )),
        }
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
                // Closed report (`EmptyAndDisentangleOutsidePort`).
                with_worker_global_scope(self.settings.ec(), |worker_global_scope, _ec| {
                    worker_global_scope.closing_flag.set(true);
                    Ok(())
                })
                .map_err(|error| format!("worker terminate: {}", error.display()))?;
                Ok(())
            }
            WorkerCommand::PortTask { port, kind } => self.handle_port_task(port, kind),
            WorkerCommand::OwnerOperation(operation) => {
                // This worker is the owner of another worker; run the
                // operation in this worker's realm.
                execute_owner_operation(&mut self.settings, operation)
            }
        }
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

    /// Handle a port task queued by the user agent's routing for a port this
    /// worker's event loop manages: land the routed message in the port's
    /// queue (or return the task to the routing queue when the port left the
    /// loop), and fire the message task inline when the port is enabled.
    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    fn handle_port_task(&mut self, port_id: PortId, kind: PortTaskKind) -> Result<(), String> {
        if self.closing_flag() {
            return Ok(());
        }
        let event_sender = self.event_sender.clone();
        let fire = with_worker_global_scope(self.settings.ec(), |worker_global_scope, ec| {
            let Some(messaging) = worker_global_scope.global_scope.channel_messaging(ec) else {
                return Ok(false);
            };
            messaging
                .handle_port_task(port_id, kind, &event_sender, ec)
                .map_err(|error| ec.new_type_error(&format!("port task: {error}")))
        })
        .map_err(|error| format!("port task failed: {}", error.display()))?;
        if fire {
            // The delivering task runs the message task itself (the message
            // event fires within this task's slot).
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
    // net command (the script fetch completion), or the earliest expiry time
    // in the worker's map of active timers.  One task runs per iteration,
    // so a task that queues another task does not starve the other inputs.
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

        let mut select = crossbeam_channel::Select::new();
        let task_arm = select.recv(&task_queue);
        let worker_command_arm = select.recv(worker_command_rx);
        let net_arm = select.recv(net_command_rx);
        let timer_arm = select.recv(&timer_expiry);

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
        } else if arm == timer_arm && operation.recv(&timer_expiry).is_ok() {
            state.run_steps_after_a_timeout();
        }

        if state.closing_flag() {
            return Ok(());
        }
    }
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
            OwnerOperation::EntangleOutsidePort {
                outside_port,
                inside_port,
            } => {
                // Step 12.8, owner half: the outside port's record was
                // registered by the Worker constructor; its entanglement is
                // set here.
                if let Some(messaging) = global_scope.channel_messaging(ec) {
                    messaging.set_entanglement(outside_port, inside_port, ec);
                }
            }
            OwnerOperation::EnableOutsidePortQueue { outside_port } => {
                // Step 12.14: enable the outside port's port message queue.
                if let Some(messaging) = global_scope.channel_messaging(ec) {
                    messaging.start(outside_port, ec);
                }
            }
            OwnerOperation::FireWorkerError { outside_port } => {
                let Some(messaging) = global_scope.channel_messaging(ec) else {
                    return Ok(());
                };
                let Some(port) = messaging.port_object(outside_port, ec) else {
                    return Ok(());
                };
                // The outside port's message event target is the Worker
                // object, so firing at the port's event target fires at the
                // Worker.
                fire_event(ec, &port.event_target, "error", time_millis, true)
                    .map(|_| ())
                    .map_err(|error| {
                        ec.new_type_error(&format!("failed to fire worker error event: {error:?}"))
                    })?;
            }
            OwnerOperation::EmptyAndDisentangleOutsidePort { outside_port } => {
                // terminate-a-worker step 4 (empty the outside port's queue)
                // and run-a-worker step 12.20 (disentangle the worker's
                // ports).
                if let Some(messaging) = global_scope.channel_messaging(ec) {
                    messaging.empty_and_disentangle(outside_port, ec);
                }
            }
        }
        Ok(())
    })
    .map_err(|error| format!("owner operation failed: {}", error.display()))
}
