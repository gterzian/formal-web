use std::cell::RefCell;
use std::rc::Rc;

use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;

use super::dedicated_worker_agent::{WorkerChannelMessage, WorkerEvent, WorkerMessageQueue};
use super::worker_global_scope::WorkerGlobalScope;
use crate::html::event_loop::Task;
use crate::html::structured_data::safe_passing_of_structured_data::structured_serialize_with_transfer;
use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

/// <https://html.spec.whatwg.org/#dedicatedworkerglobalscope>
#[gc_struct]
pub(crate) struct DedicatedWorkerGlobalScope {
    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    /// The common interface of this global scope: its event target, global
    /// scope, name, url, type, closing flag and location/navigator caches.
    pub(crate) worker_global_scope: WorkerGlobalScope,

    /// <https://html.spec.whatwg.org/#dedicatedworkerglobalscope>
    /// The inside port of this global scope: the channel the Worker
    /// constructor set up at creation entangles it with the Worker object's
    /// outside port, and `postMessage` on this global scope acts on it,
    /// sending the messages the owner fires as message events at the
    /// worker's Worker object.  This global scope is also the inside port's
    /// message event target (run-a-worker step 12.7.1): the messages the
    /// owner posts are the inside port's arrivals (see `inbound`).
    /// Note: The inside port is implemented as a direct channel end instead
    /// of a MessagePort (the worker's implicit port is bypassed; see
    /// dedicated_worker_agent.rs): the worker posts the messages it sends
    /// back, and its lifecycle reports, over its owner's worker inbox — this
    /// field is the owner-inbox sender, set once the agent's event loop is
    /// running, before the worker script is fetched.
    #[ignore_trace]
    pub(crate) inside_port: Rc<RefCell<Option<crossbeam_channel::Sender<WorkerEvent>>>>,

    /// <https://html.spec.whatwg.org/#port-message-queue>
    /// The inside port's message queue: the messages the owner posted to
    /// this worker wait here until the queue is enabled (run-a-worker step
    /// 12.15, or the first onmessage handler on this global scope); each is
    /// then delivered as a message event at this global scope (the inside
    /// port's message event target, run-a-worker step 12.7.1).  Messages
    /// that arrive before the queue is enabled wait here (a port message
    /// queue, as when start() is called or the first onmessage handler is
    /// set).
    #[ignore_trace]
    pub(crate) inbound: Rc<RefCell<WorkerMessageQueue>>,
}

impl DedicatedWorkerGlobalScope {
    pub(crate) fn new(
        global_scope: crate::html::GlobalScope,
        name: String,
        worker_type: super::worker::WorkerType,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            worker_global_scope: WorkerGlobalScope::new(global_scope, name, worker_type, ec),
            inside_port: Rc::new(RefCell::new(None)),
            inbound: Rc::new(RefCell::new(WorkerMessageQueue::default())),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-name>
    pub(crate) fn name_value(&self) -> String {
        // The name getter steps are to return this's name.
        // Note: The name is the common interface's associated name (a
        // WorkerGlobalScope object has an associated name, set during
        // creation), exposed only by the DedicatedWorkerGlobalScope
        // interface.
        self.worker_global_scope.name.clone()
    }

    /// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-postmessage>
    pub(crate) fn post_message(
        &self,
        message: JsValue,
        transfer: Vec<JsValue>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // The postMessage(message, transfer) and postMessage(message, options)
        // methods on DedicatedWorkerGlobalScope objects act as if, when
        // invoked, they immediately invoked the respective postMessage(message,
        // transfer) and postMessage(message, options) on the port, with the
        // same arguments, and returned the same return value.
        // Note: The port is this's associated inside port, realized by the
        // owner's worker inbox (the worker's implicit port is bypassed; see
        // dedicated_worker_agent.rs): step 5 of the message port post message
        // steps serializes the message here, in the worker realm, and the
        // owner's event loop runs the message task (steps 7.1-7.7) at this
        // worker's Worker object.  There is no target port to consult (steps
        // 1-4 and 6): a message sent to an owner that is gone (its document
        // destroyed or its realm closed) is dropped, like the
        // target-port-null return of step 6.
        let Some(inside_port) = self.inside_port.borrow().clone() else {
            return Ok(());
        };
        // Step 5: Let serializeWithTransferResult be
        //         StructuredSerializeWithTransfer(message, transfer). Rethrow
        //         any exceptions.
        let serialize_result = structured_serialize_with_transfer(&message, transfer, ec)?;
        // The message event target of the owner's end of the channel is the
        // worker's Worker object; the owner's event loop delivers the message
        // by worker id (the owner-side record the constructor registered).
        // A gone owner (its document destroyed or its event loop closed)
        // drops the message, the same expected condition as a reply channel
        // send.
        let Some(worker_id) = self.worker_global_scope.global_scope.worker_id() else {
            return Ok(());
        };
        let _ = inside_port.send(WorkerEvent::Message {
            worker_id,
            payload: serialize_result,
        });
        Ok(())
    }

    /// Set the owner-inbox sender the worker posts its messages and
    /// lifecycle reports back on, once the agent's event loop is running
    /// (before the worker script is fetched).
    pub(crate) fn set_inside_port(&self, inside_port: crossbeam_channel::Sender<WorkerEvent>) {
        *self.inside_port.borrow_mut() = Some(inside_port);
    }

    /// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-close>
    pub(crate) fn close(&self) {
        // The close() method steps are to close a worker given this.
        close_a_worker(&self.worker_global_scope);
    }

    /// The worker end of the worker's channel received a message the owner
    /// posted: queue the message as a message task (the worker's message
    /// queue is enabled), or let it wait in the queue until the queue is
    /// enabled (run-a-worker step 12.15, or the first onmessage handler).
    /// Runs on the agent's event-loop select when the owner→worker channel
    /// fires.
    pub(crate) fn enqueue_inbound_message(&self, payload: WorkerChannelMessage) {
        let queue_task = {
            let mut inbound = self.inbound.borrow_mut();
            if !inbound.enabled {
                inbound.pending.push_back(payload);
                None
            } else {
                Some(payload)
            }
        };
        if let Some(payload) = queue_task {
            self.queue_inbound_message_task(payload);
        }
    }

    /// <https://html.spec.whatwg.org/#messageeventtarget>
    pub(crate) fn enable_inbound_messages(&self) {
        // Enable the worker's message queue, as when start() is called, the
        // first onmessage handler is set on this global scope, or run-a-worker
        // step 12.15 runs: the messages that arrived while the queue was
        // disabled now fire as message tasks, in order.
        let pending: Vec<WorkerChannelMessage> = {
            let mut inbound = self.inbound.borrow_mut();
            inbound.enabled = true;
            inbound.pending.drain(..).collect()
        };
        for payload in pending {
            self.queue_inbound_message_task(payload);
        }
    }

    /// Queue one inbound message as a message task on this worker's event
    /// loop, firing a message event at this global scope.
    fn queue_inbound_message_task(&self, payload: WorkerChannelMessage) {
        let Some(worker_id) = self.worker_global_scope.global_scope.worker_id() else {
            error!("worker global scope has no worker id; dropping inbound message");
            return;
        };
        let Ok(task_sources) = self.worker_global_scope.global_scope.task_sources() else {
            error!("worker global scope has no task sources; dropping inbound message");
            return;
        };
        task_sources
            .task_queue()
            .queue_a_task(Task::RunWorkerInboundMessage { worker_id, payload });
    }
}

/// <https://html.spec.whatwg.org/#close-a-worker>
fn close_a_worker(worker_global_scope: &WorkerGlobalScope) {
    // Step 1: Discard any tasks that have been added to workerGlobal's
    //         relevant agent's event loop's task queues.
    // Step 2: Set workerGlobal's closing flag to true. (This prevents any
    //         further tasks from being queued.)
    // Note: The closing flag makes the dedicated worker agent's event loop
    // exit (see dedicated_worker_agent.rs), dropping the tasks queued on its
    // task queue; the agent then reports its teardown to the content process,
    // which drops the owner end of the worker's channel in the owner realm.
    worker_global_scope.closing_flag.set(true);
}
