use ipc_messages::content::{Event as ContentEvent, WorkerId, WorkerOwner, WorkerRequest};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;

use super::dedicated_worker_agent::{
    DedicatedWorkerAgentConfig, WorkerEvent, WorkerInbound, run_a_worker,
};
use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::html::structured_data::safe_passing_of_structured_data::structured_serialize_with_transfer;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::syntax_error_value;

type JsValue = <Types as JsTypes>::JsValue;

/// <https://html.spec.whatwg.org/#enumdef-workertype>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerType {
    /// <https://html.spec.whatwg.org/#dom-workertype-classic>
    Classic,
    /// <https://html.spec.whatwg.org/#dom-workertype-module>
    Module,
}

impl WorkerType {
    pub(crate) fn from_idl(value: &str) -> Self {
        if value == "module" {
            WorkerType::Module
        } else {
            WorkerType::Classic
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            WorkerType::Classic => "classic",
            WorkerType::Module => "module",
        }
    }
}

/// <https://html.spec.whatwg.org/#dedicated-workers-and-the-worker-interface>
#[gc_struct]
pub(crate) struct Worker {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub(crate) event_target: EventTarget,

    /// The id under which the worker is known to its owner event loop (the
    /// handle the owner keeps, and the association the worker reports its
    /// posted messages under).
    #[ignore_trace]
    pub(crate) worker_id: WorkerId,

    /// <https://html.spec.whatwg.org/#outside-port>
    /// The owner→worker end of this Worker's channel: the constructor
    /// creates it as its outsidePort (constructor steps 5-7) with its
    /// message event target set to this Worker, so `postMessage` on this
    /// Worker sends through it, and the messages the worker posts back fire
    /// as message events at this Worker.
    /// Note: Implemented as the owner→worker end of a direct crossbeam
    /// channel instead of a MessagePort (the worker's implicit port is
    /// bypassed; see dedicated_worker_agent.rs): the constructor creates the
    /// channel in the owner realm, the dedicated worker agent's event loop
    /// delivers each message as a message event at the worker global scope
    /// (the inside port's role, run-a-worker steps 12.6-12.8), and the
    /// messages the worker posts back travel over the owner's worker inbox
    /// to the owner realm's registered worker channel (see
    /// `GlobalScope::register_owned_worker`).  The same channel end carries
    /// the terminate-a-worker command.
    #[ignore_trace]
    pub(crate) outside_port: crossbeam_channel::Sender<WorkerInbound>,
}

impl EventTargetAccess for Worker {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl Worker {
    /// <https://html.spec.whatwg.org/#dom-worker>
    pub(crate) fn constructor(
        script_url: &str,
        name: String,
        worker_type: WorkerType,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let compliantScriptURL be the result of invoking the get
        //         trusted type compliant string algorithm with TrustedScriptURL,
        //         this's relevant global object, scriptURL, "Worker constructor",
        //         and "script".
        // Note: Trusted Types are not implemented; scriptURL is used as-is.
        // Step 2: Let outsideSettings be this's relevant settings object.
        // Note: The current realm's settings object backs the constructor
        // call; its global scope holds the ambient channels the agent thread
        // needs (its owner's worker inbox, the content process's event and
        // network senders and the network partition key) and the creation
        // URL the script URL resolves against.
        let global_scope = with_global_scope(ec, |global_scope, _ec| Ok(global_scope.clone()))
            .map_err(|error| {
                ec.new_type_error(&format!("worker constructor: {}", error.display()))
            })?;
        let worker_inbox = global_scope
            .worker_inbox()
            .ok_or_else(|| ec.new_type_error("worker constructor: no owner worker inbox"))?;
        let event_sender = global_scope
            .event_sender()
            .ok_or_else(|| ec.new_type_error("worker constructor: no event sender"))?;
        let network_extension_sender = global_scope
            .network_extension_sender()
            .ok_or_else(|| ec.new_type_error("worker constructor: no network sender"))?;
        let network_partition_event_loop_id = global_scope
            .network_partition_event_loop_id()
            .ok_or_else(|| ec.new_type_error("worker constructor: no network partition key"))?;
        let trace_sender = global_scope.trace_sender();
        let creation_url = global_scope
            .creation_url()
            .ok_or_else(|| ec.new_type_error("worker constructor: no creation URL"))?;

        // Step 3: Let workerURL be the result of encoding-parsing a URL given
        //         compliantScriptURL, relative to outsideSettings.
        // Step 4: If workerURL is failure, then throw a "SyntaxError" DOMException.
        let worker_url = creation_url
            .join(script_url)
            .map_err(|_| syntax_error_value(ec))?;

        // Step 5: Let outsidePort be a new MessagePort in outsideSettings's
        //         realm.
        // Step 6: Set outsidePort's message event target to this.
        // Step 7: Set this's outside port to outsidePort.
        // Note: The worker's implicit port is bypassed (dedicated worker
        // channels are direct crossbeam channels; see dedicated_worker_agent.rs):
        // the constructor creates the owner→worker channel of the worker
        // here, in the owner realm, and registers this Worker object's event
        // target with its worker id in the owner realm's global scope, as the
        // target of the message events the messages the worker posts back
        // fire at (the role the outside port's record played).
        let worker_id = WorkerId::new();
        let (owner_to_worker_tx, owner_to_worker_rx) =
            crossbeam_channel::unbounded::<WorkerInbound>();
        let worker = Worker {
            event_target: EventTarget::new(ec),
            worker_id,
            outside_port: owner_to_worker_tx.clone(),
        };
        global_scope.register_owned_worker(worker_id, worker.event_target.clone(), ec);

        // Step 8: Let worker be this.
        // Note: `worker` above is this.
        // Step 9: Run this step in parallel:
        // Step 9.1: Run a worker given worker, workerURL, outsideSettings,
        //           outsidePort, and options.
        // Note: The "in parallel" hop is for shared workers (which run over
        // the user agent); for a dedicated worker the start of run a worker
        // runs here, on the event loop that will own the worker.  The
        // dedicated start runs synchronously below: run-a-worker steps 1-3
        // (is shared is false; owner is the relevant owner, computed next;
        // unsafeWorkerCreationTime is not implemented), then step 4's
        // "obtain a dedicated/shared worker agent" — the dedicated path,
        // creating the agent right here (the agent's native thread and its
        // worker event loop).  The agent then runs the rest of run a worker
        // (steps 5-12.21; see dedicated_worker_agent.rs::run_a_worker).
        // The owner end of the worker's channel is registered above; the
        // messages the owner posts before the worker realm exists wait in
        // the owner→worker channel (unbounded) and are delivered once the
        // worker enables its message queue.
        let owner = match global_scope.worker_id() {
            Some(parent_worker_id) => WorkerOwner::Worker(parent_worker_id),
            None => WorkerOwner::Document(
                global_scope
                    .document_id()
                    .ok_or_else(|| ec.new_type_error("worker constructor: no owner document"))?,
            ),
        };
        let request = WorkerRequest {
            worker_id,
            script_url: worker_url.to_string(),
            name,
            worker_type: worker_type.as_str().to_owned(),
            owner,
        };
        // The dedicated worker agent's thread.  The agent reports itself to
        // the user agent (DedicatedWorkerAgentObtained, with its own worker
        // event loop id and UA command channel) as its first act and runs the
        // rest of run a worker; when it exits — also on failure or panic — it
        // reports its close to the user agent and its owner event loop, so
        // the owner joins the thread.
        // <https://html.spec.whatwg.org/#run-a-worker>
        let config = DedicatedWorkerAgentConfig {
            request,
            owner_to_worker: owner_to_worker_rx,
            owner_inbox: worker_inbox.clone(),
            network_partition_event_loop_id,
            event_sender: event_sender.clone(),
            network_extension_sender,
            trace_sender,
        };
        let thread_event_sender = event_sender;
        let thread_inbox = worker_inbox.clone();
        let thread_worker_id = worker_id;
        let join_handle = match std::thread::Builder::new()
            .name(format!("formal-web:worker-{worker_id}"))
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_a_worker(config)));
                match result {
                    Ok(Err(error)) => error!("worker thread {worker_id} failed: {error}"),
                    Ok(Ok(())) => {}
                    Err(panic) => error!("worker thread {worker_id} panicked: {panic:?}"),
                }
                // The agent thread reports its teardown on every path: the
                // user agent drops the agent record it created on
                // DedicatedWorkerAgentObtained, and the owner event loop
                // joins the thread and runs the owner-side cleanup.
                let _ = thread_event_sender.send(ContentEvent::DedicatedWorkerAgentClosed {
                    worker_id: thread_worker_id,
                });
                let _ = thread_inbox.send(WorkerEvent::Closed {
                    worker_id: thread_worker_id,
                });
            }) {
            Ok(join_handle) => join_handle,
            Err(spawn_error) => {
                error!("worker constructor: failed to spawn worker thread: {spawn_error}");
                return Ok(worker);
            }
        };
        // Register the worker with its owner event loop (the loop that
        // delivers its posted messages and joins its thread): the owner
        // keeps the worker's handle (the owner→worker channel end, used to
        // terminate the worker when its owner goes away) and the thread.
        if let Err(error) = worker_inbox.send(WorkerEvent::NewWorker {
            worker_id,
            owner,
            owner_to_worker: owner_to_worker_tx,
            join_handle,
        }) {
            error!("worker constructor: failed to register worker {worker_id}: {error}");
        }
        Ok(worker)
    }

    /// <https://html.spec.whatwg.org/#dom-worker-terminate>
    pub(crate) fn terminate(&self) {
        // The terminate() method steps are to terminate a worker given this's
        // worker.
        // Note: The terminate-a-worker command travels over the worker's
        // owner→worker channel (the same channel its postMessage uses): the
        // agent sets its closing flag and discards its queued tasks, then
        // tears down the worker realm and its own nested workers.  The
        // worker's teardown report then reaches its owner event loop, which
        // joins its thread and runs the owner-side cleanup.  A worker whose
        // agent already exited drops the command (its channel end is gone).
        let _ = self.outside_port.send(WorkerInbound::Terminate);
    }

    /// <https://html.spec.whatwg.org/#dom-worker-postmessage>
    pub(crate) fn post_message(
        &self,
        message: JsValue,
        transfer: Vec<JsValue>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // The postMessage(message, transfer) and postMessage(message, options)
        // methods on Worker objects act as if, when invoked, they immediately
        // invoked the respective postMessage(message, transfer) and
        // postMessage(message, options) on this's outside port, with the same
        // arguments, and returned the same return value.
        // Note: The outside port is realized by the direct owner→worker
        // channel (the worker's implicit port is bypassed; see
        // dedicated_worker_agent.rs).  Step 5 of the message port post
        // message steps serializes the message here, in the owner realm; the
        // worker agent's event loop runs the message task (steps 7.1-7.7) at
        // the worker global scope.  There is no target port to consult (steps
        // 1-4 and 6): a message sent to a closed worker is dropped, like the
        // target-port-null return of step 6.
        // Step 5: Let serializeWithTransferResult be
        //         StructuredSerializeWithTransfer(message, transfer). Rethrow
        //         any exceptions.
        let serialize_result = structured_serialize_with_transfer(&message, transfer, ec)?;
        // A closed worker (its agent exited and dropped its channel end, or
        // the owner document went away) drops the message, the same expected
        // condition as a reply channel send.
        let _ = self
            .outside_port
            .send(WorkerInbound::Message(serialize_result));
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#messageeventtarget>
    pub(crate) fn enable_message_delivery(&self, ec: &mut dyn ExecutionContext<Types>) {
        // The first time a Worker object's onmessage IDL attribute is set, the
        // port message queue of the worker's outside port must be enabled, as
        // if the start() method had been called.
        // Note: The owner end of the worker's channel (the messages the
        // worker posts back) lives in the owner realm's global scope; enabling
        // it here flushes the messages that arrived while the queue was
        // disabled.
        if let Err(error) = with_global_scope(ec, |global_scope, ec| {
            global_scope.enable_owned_worker_messages(self.worker_id, ec);
            Ok(())
        }) {
            error!(
                "worker {}: failed to enable message delivery: {}",
                self.worker_id,
                error.display()
            );
        }
    }
}
