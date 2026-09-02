use std::cell::{Cell, RefCell};
use std::rc::Rc;

use data_url::DataUrl;
use ipc_messages::content::{WorkerId, WorkerOwner, WorkerRequest};
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;
use url::Url;

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::html::dedicated_worker_agent::{
    WorkerChannelMessage, WorkerContentRequest, WorkerMessageQueue, WorkerStartRequest,
};
use crate::html::event_loop::Task;
use crate::html::structured_data::safe_passing_of_structured_data::structured_serialize_with_transfer;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::syntax_error_value;

use super::GlobalScope;
use super::worker_location::WorkerLocation;
use super::worker_navigator::WorkerNavigator;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

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

    /// The id under which the content process's worker table knows this
    /// worker (terminate() reports it to the content process's worker
    /// manager).
    #[ignore_trace]
    pub(crate) worker_id: WorkerId,

    /// <https://html.spec.whatwg.org/#dedicated-workers-and-the-worker-interface>
    /// The owner→worker end of the worker's channel: `postMessage` on this
    /// Worker sends the serialized messages here, and the dedicated worker
    /// agent's event loop delivers them as message events at the worker
    /// global scope.  This replaces the constructor-created outside port of
    /// the port-based model (the worker's implicit port is bypassed; see
    /// dedicated_worker_agent.rs).
    #[ignore_trace]
    pub(crate) owner_to_worker: crossbeam_channel::Sender<WorkerChannelMessage>,
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
        // call; its worker creator channel and creation URL are read from the
        // current realm's GlobalScope below.
        let global_scope = with_global_scope(ec, |global_scope, _ec| Ok(global_scope.clone()))
            .map_err(|error| {
                ec.new_type_error(&format!("worker constructor: {}", error.display()))
            })?;
        let worker_creator = global_scope
            .worker_creator()
            .ok_or_else(|| ec.new_type_error("worker constructor: no worker creator channel"))?;
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
        // the constructor creates the two channels of the worker here, in the
        // owner realm, and registers this Worker object's event target with
        // its worker id in the owner realm's global scope, as the target of
        // the message events the messages the worker posts back fire at (the
        // role the outside port's record played).  The channel ends that
        // travel with the run-a-worker request are handed to the worker
        // agent's event loop and to the owner's event loop.
        let worker_id = WorkerId::new();
        let (owner_to_worker_tx, owner_to_worker_rx) =
            crossbeam_channel::unbounded::<WorkerChannelMessage>();
        let (worker_to_owner_tx, worker_to_owner_rx) =
            crossbeam_channel::unbounded::<WorkerChannelMessage>();
        let worker = Worker {
            event_target: EventTarget::new(ec),
            worker_id,
            owner_to_worker: owner_to_worker_tx,
        };
        global_scope.register_owned_worker(worker_id, worker.event_target.clone(), ec);

        // Step 8: Let worker be this.
        // Note: `worker` above is this.
        // Step 9: Run this step in parallel:
        // Step 9.1: Run a worker given worker, workerURL, outsideSettings,
        //           outsidePort, and options.
        // Note: Dedicated workers are entirely content-process-nested: the
        // constructor reports the request to the content process's worker
        // manager (through the current realm's worker creator channel), which
        // starts the worker's dedicated worker agent (a native thread; see
        // dedicated_worker_agent.rs).  The user agent is not involved.  The
        // owner end of the worker's channel is registered above; the messages
        // the owner posts before the worker realm exists wait in the
        // owner→worker channel (unbounded) and are delivered once the worker
        // enables its message queue.
        let owner = match global_scope.worker_id() {
            Some(parent_worker_id) => WorkerOwner::Worker(parent_worker_id),
            None => WorkerOwner::Document(
                global_scope
                    .document_id()
                    .ok_or_else(|| ec.new_type_error("worker constructor: no owner document"))?,
            ),
        };
        if let Err(send_error) =
            worker_creator.send(WorkerContentRequest::Create(WorkerStartRequest {
                request: WorkerRequest {
                    worker_id,
                    script_url: worker_url.to_string(),
                    name,
                    worker_type: worker_type.as_str().to_owned(),
                    owner,
                },
                owner_to_worker: owner_to_worker_rx,
                worker_to_owner: worker_to_owner_tx,
                worker_to_owner_rx,
            }))
        {
            error!("worker constructor: failed to request worker: {send_error}");
        }
        Ok(worker)
    }

    /// <https://html.spec.whatwg.org/#dom-worker-terminate>
    pub(crate) fn terminate(&self, ec: &mut dyn ExecutionContext<Types>) {
        // The terminate() method steps are to terminate a worker given this's
        // worker.
        // Note: The content process runs the terminate-a-worker steps: the
        // method reports the worker id to the content process's worker
        // manager, which sends the dedicated worker agent a `Terminate`
        // command; the agent sets the closing flag, discards its queued
        // tasks, and tears down the worker realm.
        if let Err(error) = self.notify_termination(ec) {
            error!("worker terminate: failed to request termination: {error}");
        }
    }

    /// Report the worker's termination to the content process through the
    /// current realm's worker creator channel.
    fn notify_termination(&self, ec: &mut dyn ExecutionContext<Types>) -> Result<(), String> {
        let global_scope = with_global_scope(ec, |global_scope, _ec| Ok(global_scope.clone()))
            .map_err(|error| format!("worker terminate: {}", error.display()))?;
        let worker_creator = global_scope
            .worker_creator()
            .ok_or_else(|| String::from("worker terminate: no worker creator channel"))?;
        worker_creator
            .send(WorkerContentRequest::Terminate(self.worker_id))
            .map_err(|error| format!("worker terminate: {error}"))
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
        let _ = self.owner_to_worker.send(serialize_result);
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

/// <https://html.spec.whatwg.org/#dedicated-workerglobalscope>
#[gc_struct]
pub(crate) struct DedicatedWorkerGlobalScope {}

/// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
#[gc_struct]
pub(crate) struct WorkerGlobalScope {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub(crate) event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub(crate) global_scope: GlobalScope,

    /// <https://html.spec.whatwg.org/#dom-worker-name>
    #[ignore_trace]
    pub(crate) name: String,

    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    #[ignore_trace]
    pub(crate) worker_type: WorkerType,

    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    #[ignore_trace]
    pub(crate) url: Rc<RefCell<Option<Url>>>,

    /// <https://html.spec.whatwg.org/#the-worker-s-lifetime>
    #[ignore_trace]
    pub(crate) closing_flag: Rc<Cell<bool>>,

    /// <https://html.spec.whatwg.org/#dedicated-workerglobalscope>
    /// The worker→owner end of the worker's channel: `postMessage` on this
    /// global scope sends the messages the owner fires as message events at
    /// the worker's Worker object.  This replaces the inside port of the
    /// port-based model (the worker's implicit port is bypassed; see
    /// dedicated_worker_agent.rs).  Set once the agent's event loop is
    /// running, before the worker script is fetched.
    #[ignore_trace]
    pub(crate) worker_to_owner:
        Rc<RefCell<Option<crossbeam_channel::Sender<WorkerChannelMessage>>>>,

    /// <https://html.spec.whatwg.org/#dedicated-workerglobalscope>
    /// The worker end of the worker's channel: the messages the owner
    /// posted.  Each is delivered as a message event at this global scope
    /// (its implicit port's message event target) once its message queue is
    /// enabled; messages that arrive before that wait in the queue.
    #[ignore_trace]
    pub(crate) inbound: Rc<RefCell<WorkerMessageQueue>>,

    /// <https://html.spec.whatwg.org/#workerlocation>
    /// The cached WorkerLocation JS object, created on first access.
    pub(crate) location_object: GcCell<Option<JsObject>>,

    /// <https://html.spec.whatwg.org/#workernavigator>
    /// The cached WorkerNavigator JS object, created on first access.
    pub(crate) navigator_object: GcCell<Option<JsObject>>,
}

impl EventTargetAccess for WorkerGlobalScope {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl WorkerGlobalScope {
    pub(crate) fn new(
        global_scope: GlobalScope,
        name: String,
        worker_type: WorkerType,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            event_target: EventTarget::new(ec),
            global_scope,
            name,
            worker_type,
            url: Rc::new(RefCell::new(None)),
            closing_flag: Rc::new(Cell::new(false)),
            worker_to_owner: Rc::new(RefCell::new(None)),
            inbound: Rc::new(RefCell::new(WorkerMessageQueue::default())),
            location_object: gc_cell_new(None, ec),
            navigator_object: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    pub(crate) fn set_url(&self, url: Url) {
        *self.url.borrow_mut() = Some(url);
    }

    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    pub(crate) fn url(&self) -> Option<Url> {
        self.url.borrow().clone()
    }

    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    pub(crate) fn self_value(&self, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
        // The self attribute must return the WorkerGlobalScope object itself.
        // Note: As for the Window getters, the global object is the relevant
        // realm's [[GlobalEnv]].[[GlobalThisValue]].
        // <https://html.spec.whatwg.org/#concept-relevant-realm>
        crate::webidl::relevant_realm_global_this_value(ec)
    }

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-location>
    pub(crate) fn location_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<WorkerLocation, Types> {
        // The location attribute must return the WorkerLocation object whose
        // associated WorkerGlobalScope object is the WorkerGlobalScope object.
        if let Some(location_object) = self.location_object.borrow(ec).clone() {
            let location = ec
                .with_object_any(&location_object)
                .and_then(|data| data.downcast_ref::<WorkerLocation>().cloned())
                .ok_or_else(|| ec.new_type_error("location object is not a WorkerLocation"))?;
            return Ok(location);
        }
        let url = self
            .url()
            .unwrap_or_else(|| Url::parse("about:blank").expect("parse about:blank"));
        let location = WorkerLocation::new(url);
        let object = crate::webidl::bindings::create_interface_instance::<Types, WorkerLocation>(
            location.clone(),
            ec,
        )?;
        self.location_object.set(Some(object), ec);
        Ok(location)
    }

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-navigator>
    pub(crate) fn navigator_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<WorkerNavigator, Types> {
        // The navigator attribute must return the WorkerNavigator object
        // whose associated WorkerGlobalScope object is the WorkerGlobalScope
        // object.
        if let Some(navigator_object) = self.navigator_object.borrow(ec).clone() {
            let navigator = ec
                .with_object_any(&navigator_object)
                .and_then(|data| data.downcast_ref::<WorkerNavigator>().cloned())
                .ok_or_else(|| ec.new_type_error("navigator object is not a WorkerNavigator"))?;
            return Ok(navigator);
        }
        let navigator = WorkerNavigator::new();
        let object = crate::webidl::bindings::create_interface_instance::<Types, WorkerNavigator>(
            navigator.clone(),
            ec,
        )?;
        self.navigator_object.set(Some(object), ec);
        Ok(navigator)
    }

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-importscripts>
    pub(crate) fn import_scripts(
        &self,
        urls: Vec<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let urlStrings be « ».
        // Step 2: For each url of urls:
        // Step 2.1: Append the result of invoking the get trusted type
        //           compliant string algorithm with TrustedScriptURL, this's
        //           relevant global object, url, "WorkerGlobalScope
        //           importScripts", and "script" to urlStrings.
        // Note: Trusted Types are not implemented; the given url strings are
        // used as-is, so urlStrings is urls.
        import_scripts_into_worker_global_scope(self, urls, ec)
    }

    /// <https://html.spec.whatwg.org/#dedicated-workerglobalscope>
    pub(crate) fn name_value(&self) -> String {
        // The name getter steps are to return this's name.
        self.name.clone()
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
        // Note: The worker's implicit port is bypassed (dedicated worker
        // channels are direct crossbeam channels; see dedicated_worker_agent.rs):
        // step 5 of the message port post message steps serializes the
        // message here, in the worker realm, and the owner's event loop runs
        // the message task (steps 7.1-7.7) at this worker's Worker object.
        // There is no target port to consult (steps 1-4 and 6): a message
        // sent to an owner that is gone (its document destroyed or its realm
        // closed) is dropped, like the target-port-null return of step 6.
        let Some(worker_to_owner) = self.worker_to_owner.borrow().clone() else {
            return Ok(());
        };
        // Step 5: Let serializeWithTransferResult be
        //         StructuredSerializeWithTransfer(message, transfer). Rethrow
        //         any exceptions.
        let serialize_result = structured_serialize_with_transfer(&message, transfer, ec)?;
        // A gone owner (its document destroyed or its event loop closed)
        // drops the message, the same expected condition as a reply channel
        // send.
        let _ = worker_to_owner.send(serialize_result);
        Ok(())
    }

    /// Set the worker→owner end of the worker's channel, once the agent's
    /// event loop is running (before the worker script is fetched).
    pub(crate) fn set_worker_to_owner(
        &self,
        worker_to_owner: crossbeam_channel::Sender<WorkerChannelMessage>,
    ) {
        *self.worker_to_owner.borrow_mut() = Some(worker_to_owner);
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
        let Some(worker_id) = self.global_scope.worker_id() else {
            error!("worker global scope has no worker id; dropping inbound message");
            return;
        };
        let Ok(task_sources) = self.global_scope.task_sources() else {
            error!("worker global scope has no task sources; dropping inbound message");
            return;
        };
        task_sources
            .task_queue()
            .queue_a_task(Task::RunWorkerInboundMessage { worker_id, payload });
    }

    /// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-close>
    pub(crate) fn close(&self, ec: &mut dyn ExecutionContext<Types>) {
        // The close() method steps are to close a worker given this.
        let _ = ec;
        close_a_worker(self);
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

/// <https://html.spec.whatwg.org/#import-scripts-into-worker-global-scope>
fn import_scripts_into_worker_global_scope(
    worker_global_scope: &WorkerGlobalScope,
    urls: Vec<String>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: If worker global scope's type is "module", throw a TypeError
    //         exception.
    if worker_global_scope.worker_type == WorkerType::Module {
        return Err(ec.new_type_error("importScripts() is disallowed inside module workers"));
    }
    // Step 2: Let settings object be the current settings object.
    // Note: The current realm is the worker realm.
    // Step 3: If urls is empty, return.
    if urls.is_empty() {
        return Ok(());
    }
    // Step 4: Let urlRecords be « ».
    // Step 5: For each url of urls:
    // Step 5.1: Let urlRecord be the result of encoding-parsing a URL given
    //           url, relative to settings object.
    // Step 5.2: If urlRecord is failure, then throw a "SyntaxError"
    //           DOMException.
    // Step 5.3: Append urlRecord to urlRecords.
    let base_url = worker_global_scope
        .url()
        .unwrap_or_else(|| Url::parse("about:blank").expect("parse about:blank"));
    let mut url_records = Vec::with_capacity(urls.len());
    for url in urls {
        let url_record = base_url.join(&url).map_err(|_| syntax_error_value(ec))?;
        url_records.push(url_record);
    }
    // Step 6: For each urlRecord of urlRecords:
    // Step 6.1: Fetch a classic worker-imported script given urlRecord and
    //           settings object, passing along performFetch if provided. If
    //           this succeeds, let script be the result. Otherwise, rethrow
    //           the exception.
    // Step 6.2: Run the classic script script, with rethrow errors set to
    //           true.
    // Note: Only data: URLs are supported so far: the fetch is local and
    // synchronous, so the script can be run before importScripts returns.
    // Fetches of other URL schemes are asynchronous in this architecture
    // (through the net process) and are not implemented yet; they throw a
    // "NotSupportedError" DOMException.
    for url_record in url_records {
        if url_record.scheme() == "data" {
            let (bytes, _fragment) = match DataUrl::process(url_record.as_str())
                .map_err(|_| syntax_error_value(ec))?
                .decode_to_vec()
            {
                Ok(decoded) => decoded,
                Err(_) => return Err(syntax_error_value(ec)),
            };
            let source = String::from_utf8_lossy(&bytes).into_owned();
            // Step 6.2: Run the classic script script, with rethrow errors
            // set to true.
            // Note: Rethrowing is approximated: a failed parse or an uncaught
            // exception aborts the import (the evaluate result is an error).
            ec.evaluate_script(&source).map(|_| ())?;
        } else {
            return Err(crate::webidl::not_supported_error_value(
                format!("importScripts of non-data URL `{url_record}` is not implemented"),
                ec,
            ));
        }
    }
    Ok(())
}
