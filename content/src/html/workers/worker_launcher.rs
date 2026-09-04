use ipc::IpcSender;
use ipc_messages::content::{
    DocumentId, Event as ContentEvent, EventLoopId, WorkerId, WorkerOwner,
};
use ipc_messages::network::Request as NetworkRequest;
use log::error;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use verification::TraceSender;

use super::dedicated_worker_agent::{
    DedicatedWorkerAgentConfig, WorkerBootstrap, WorkerChannelMessage, WorkerCommand, WorkerEvent,
    run_a_worker,
};

/// The content-process (similar-origin window agent) state of one dedicated
/// worker: the worker runs on its own dedicated worker agent (a native thread
/// nested to this content process; see
/// `content/src/html/workers/dedicated_worker_agent.rs`), so this entry
/// stores only the thread-safe handles the main thread needs: its command
/// channel, its join handle, and (when the owner is a document in this
/// process) the receiver end of the worker's outbound channel, whose messages
/// the main event loop fires as message events at the worker's Worker object.
/// When the owner is another worker, that receiver was forwarded to the owner
/// worker agent's event loop instead.
/// <https://html.spec.whatwg.org/#run-a-worker>
pub(crate) struct ContentWorker {
    /// <https://html.spec.whatwg.org/#concept-WorkerGlobalScope-owner-set>
    pub(crate) owner: WorkerOwner,
    /// The receiver end of the worker's outbound channel (the messages
    /// `self.postMessage` in the worker sends), selected on by the main
    /// event loop; `None` when the owner is a worker (the receiver was
    /// forwarded to the owner worker agent's event loop).
    /// <https://html.spec.whatwg.org/#dedicated-workers-and-the-worker-interface>
    pub(crate) worker_to_owner: Option<crossbeam_channel::Receiver<WorkerChannelMessage>>,
    /// Commands sent to the dedicated worker agent (its native thread).
    pub(crate) command_sender: crossbeam_channel::Sender<WorkerCommand>,
    /// The dedicated worker agent's thread, joined when the worker reports
    /// its teardown and when the content process shuts down.
    pub(crate) join_handle: Option<std::thread::JoinHandle<()>>,
}

/// The dedicated workers running in this content process, keyed by worker
/// id, shared between the content process's main loop and the
/// [`WorkerLauncher`] every realm holds (a document's realm on the main
/// thread, a dedicated worker agent's realm on its native thread).  Each
/// worker's realm and event loop live on its own dedicated worker agent (a
/// native thread); the record holds the agent's thread handle and the
/// routing data the main loop needs.
/// <https://html.spec.whatwg.org/#run-a-worker>
#[derive(Clone, Default)]
pub(crate) struct WorkerRegistry {
    workers: Arc<Mutex<HashMap<WorkerId, ContentWorker>>>,
}

impl WorkerRegistry {
    /// Record a dedicated worker.  The Worker constructor registers each
    /// worker synchronously when it creates the worker's agent (see
    /// `WorkerLauncher::run_a_worker`), on whichever thread the constructor
    /// ran on.
    pub(crate) fn register(&self, worker_id: WorkerId, worker: ContentWorker) {
        if let Ok(mut workers) = self.workers.lock() {
            workers.insert(worker_id, worker);
        }
    }

    /// Remove a worker's record: the content process's Closed handling joins
    /// its agent's thread and runs the owner-side cleanup.
    pub(crate) fn remove(&self, worker_id: &WorkerId) -> Option<ContentWorker> {
        self.workers
            .lock()
            .ok()
            .and_then(|mut workers| workers.remove(worker_id))
    }

    /// The command channel to a worker's dedicated worker agent, if the
    /// worker is still registered.
    pub(crate) fn command_sender(
        &self,
        worker_id: WorkerId,
    ) -> Option<crossbeam_channel::Sender<WorkerCommand>> {
        self.workers.lock().ok().and_then(|workers| {
            workers
                .get(&worker_id)
                .map(|worker| worker.command_sender.clone())
        })
    }

    /// The owner of a registered worker, if it is still registered.
    pub(crate) fn owner(&self, worker_id: WorkerId) -> Option<WorkerOwner> {
        self.workers
            .lock()
            .ok()
            .and_then(|workers| workers.get(&worker_id).map(|worker| worker.owner))
    }

    /// The receiver ends of the outbound channels of the registered workers
    /// owned by a document: selected on by the main event loop, which
    /// delivers each message the worker posts in the owner document's realm.
    /// (The outbound channels of worker-owned workers were forwarded to the
    /// owner worker agent's event loop.)
    pub(crate) fn document_owned_receivers(
        &self,
    ) -> Vec<(WorkerId, crossbeam_channel::Receiver<WorkerChannelMessage>)> {
        let Ok(workers) = self.workers.lock() else {
            return Vec::new();
        };
        workers
            .iter()
            .filter(|(_, worker)| matches!(worker.owner, WorkerOwner::Document(_)))
            .filter_map(|(worker_id, worker)| {
                worker
                    .worker_to_owner
                    .clone()
                    .map(|receiver| (*worker_id, receiver))
            })
            .collect()
    }

    /// The registered workers a worker owns: when the owner's agent closes,
    /// they are terminated (their owner realm is gone).
    pub(crate) fn owned_worker_ids(&self, owner_worker_id: WorkerId) -> Vec<WorkerId> {
        let Ok(workers) = self.workers.lock() else {
            return Vec::new();
        };
        workers
            .iter()
            .filter(|(_, worker)| {
                matches!(
                    worker.owner,
                    WorkerOwner::Worker(owner_id) if owner_id == owner_worker_id
                )
            })
            .map(|(worker_id, _)| *worker_id)
            .collect()
    }

    /// The registered workers a document owns: when the document is
    /// destroyed, they are terminated.
    pub(crate) fn document_owned_worker_ids(&self, document_id: DocumentId) -> Vec<WorkerId> {
        let Ok(workers) = self.workers.lock() else {
            return Vec::new();
        };
        workers
            .iter()
            .filter(|(_, worker)| {
                matches!(worker.owner, WorkerOwner::Document(owner_id) if owner_id == document_id)
            })
            .map(|(worker_id, _)| *worker_id)
            .collect()
    }

    /// Whether a worker is still registered.
    pub(crate) fn contains(&self, worker_id: WorkerId) -> bool {
        self.workers
            .lock()
            .map(|workers| workers.contains_key(&worker_id))
            .unwrap_or(false)
    }

    /// Take every registered worker (shutdown): each agent is terminated
    /// first, then the entries' threads are joined.
    pub(crate) fn take_all(&self) -> Vec<(WorkerId, ContentWorker)> {
        self.workers
            .lock()
            .ok()
            .map(|mut workers| workers.drain().collect())
            .unwrap_or_default()
    }

    /// The registered worker ids (shutdown terminates each agent).
    pub(crate) fn worker_ids(&self) -> Vec<WorkerId> {
        self.workers
            .lock()
            .ok()
            .map(|workers| workers.keys().copied().collect())
            .unwrap_or_default()
    }

    /// The command channels to every registered worker agent: the main
    /// thread forwards a user-agent port task that no document realm owns to
    /// every worker agent, and the agent whose realm holds the port delivers
    /// it.
    pub(crate) fn all_command_senders(&self) -> Vec<crossbeam_channel::Sender<WorkerCommand>> {
        self.workers
            .lock()
            .ok()
            .map(|workers| {
                workers
                    .values()
                    .map(|worker| worker.command_sender.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The resources a realm uses to create a dedicated worker's agent
/// synchronously.  The Worker constructor runs the dedicated start of run a
/// worker then and there in the owner realm — the spec's "in parallel" hop
/// (constructor step 9) only exists for shared workers — creating the agent
/// (a native thread) here and registering the worker with the content
/// process's [`WorkerRegistry`] for the main loop's lifecycle handling.  The
/// agent thread then runs the rest of run a worker (steps 5-12.21; see
/// dedicated_worker_agent.rs).  One launcher is shared by every realm of
/// this content process — cloned onto window global scopes at document
/// creation and onto worker global scopes when their agent is created — so
/// nested workers are created the same way from a worker agent's thread.
#[derive(Clone)]
pub(crate) struct WorkerLauncher {
    event_loop_id: EventLoopId,
    event_sender: IpcSender<ContentEvent>,
    network_extension_sender: IpcSender<NetworkRequest>,
    trace_sender: Option<TraceSender>,
    worker_event_sender: crossbeam_channel::Sender<WorkerEvent>,
    registry: WorkerRegistry,
}

impl WorkerLauncher {
    pub(crate) fn new(
        event_loop_id: EventLoopId,
        event_sender: IpcSender<ContentEvent>,
        network_extension_sender: IpcSender<NetworkRequest>,
        trace_sender: Option<TraceSender>,
        worker_event_sender: crossbeam_channel::Sender<WorkerEvent>,
        registry: WorkerRegistry,
    ) -> Self {
        Self {
            event_loop_id,
            event_sender,
            network_extension_sender,
            trace_sender,
            worker_event_sender,
            registry,
        }
    }

    /// Run-a-worker step 4's dedicated path, run synchronously by the Worker
    /// constructor in the owner realm: create the worker's dedicated worker
    /// agent (a native thread nested to this content process; see
    /// dedicated_worker_agent.rs), register the worker, and hand the
    /// owner-side end of its channel to the event loop that owns it.  The
    /// agent itself runs the rest of run a worker: the realm (steps 5-9),
    /// the script fetch (step 12, over its own net channel), and the
    /// onComplete steps (12.3-12.15).
    pub(crate) fn run_a_worker(&self, start: WorkerBootstrap) -> Result<(), String> {
        let WorkerBootstrap {
            request,
            owner_to_worker,
            worker_to_owner,
            worker_to_owner_rx,
        } = start;
        let worker_id = request.worker_id;
        let owner = request.owner;
        let (worker_command_tx, worker_command_rx) = crossbeam_channel::unbounded();
        let launcher = self.clone();
        let join_handle = std::thread::Builder::new()
            .name(format!("formal-web:worker-{worker_id}"))
            .spawn(move || {
                let config = DedicatedWorkerAgentConfig {
                    request,
                    owner_to_worker,
                    worker_to_owner,
                    event_loop_id: launcher.event_loop_id,
                    worker_events: launcher.worker_event_sender.clone(),
                    worker_commands: worker_command_rx,
                    event_sender: launcher.event_sender.clone(),
                    network_extension_sender: launcher.network_extension_sender.clone(),
                    worker_launcher: launcher.clone(),
                    trace_sender: launcher.trace_sender.clone(),
                };
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_a_worker(config)));
                match result {
                    Ok(Err(error)) => error!("worker thread {worker_id} failed: {error}"),
                    Ok(Ok(())) => {}
                    Err(panic) => error!("worker thread {worker_id} panicked: {panic:?}"),
                }
                // The agent thread reports its teardown on every path (also
                // on failure or panic), so the main thread joins the agent's
                // thread and runs the owner-side cleanup.
                let _ = launcher
                    .worker_event_sender
                    .send(WorkerEvent::Closed { worker_id });
            })
            .map_err(|error| format!("failed to spawn worker thread: {error}"))?;
        // The receiver end of the worker's outbound channel joins the event
        // loop that owns the worker: the main loop when the owner is a
        // document in this process (the messages fire as message events at
        // the worker's Worker object in the owner document's realm), or the
        // owner worker agent's event loop (forwarded as a command) when the
        // owner is a worker.
        let worker_to_owner_rx = match owner {
            WorkerOwner::Document(_) => Some(worker_to_owner_rx),
            WorkerOwner::Worker(owner_worker_id) => {
                let owner_command_sender = self
                    .registry
                    .command_sender(owner_worker_id)
                    .ok_or_else(|| {
                        format!(
                            "run a worker: owner worker {owner_worker_id} closed before the worker {worker_id} could start"
                        )
                    })?;
                owner_command_sender
                    .send(WorkerCommand::AddNestedWorkerChannel {
                        worker_id,
                        receiver: worker_to_owner_rx,
                    })
                    .map_err(|error| {
                        format!(
                            "run a worker: failed to route worker {worker_id} channel to owner worker {owner_worker_id}: {error}"
                        )
                    })?;
                None
            }
        };
        self.registry.register(
            worker_id,
            ContentWorker {
                owner,
                worker_to_owner: worker_to_owner_rx,
                command_sender: worker_command_tx,
                join_handle: Some(join_handle),
            },
        );
        Ok(())
    }

    /// Send a dedicated worker agent the terminate command (the command
    /// half of terminate a worker), looked up in the shared registry.
    pub(crate) fn terminate(&self, worker_id: WorkerId) -> Result<(), String> {
        let Some(command_sender) = self.registry.command_sender(worker_id) else {
            // The worker already closed (its Closed report is queued).
            return Ok(());
        };
        command_sender
            .send(WorkerCommand::Terminate)
            .map_err(|error| format!("failed to terminate worker {worker_id}: {error}"))
    }
}
