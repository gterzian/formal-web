use std::cell::{Cell, RefCell};
use std::rc::Rc;

use data_url::DataUrl;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;
use url::Url;

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::html::dedicated_worker_agent::{WorkerChannelMessage, WorkerMessageQueue};
use crate::html::event_loop::Task;
use crate::html::structured_data::safe_passing_of_structured_data::structured_serialize_with_transfer;
use crate::js::Types;
use crate::webidl::syntax_error_value;

use super::GlobalScope;
use super::worker::WorkerType;
use super::worker_location::WorkerLocation;
use super::worker_navigator::WorkerNavigator;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
#[gc_struct]
pub(crate) struct WorkerGlobalScope {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub(crate) event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub(crate) global_scope: GlobalScope,

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-name>
    #[ignore_trace]
    pub(crate) name: String,

    /// The worker's type, from the WorkerOptions the constructor was given
    /// (options["type"]): "classic" or "module".  Only "classic" script
    /// evaluation is implemented.
    /// <https://html.spec.whatwg.org/#enumdef-workertype>
    #[ignore_trace]
    pub(crate) worker_type: WorkerType,

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-url>
    #[ignore_trace]
    pub(crate) url: Rc<RefCell<Option<Url>>>,

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-closing>
    #[ignore_trace]
    pub(crate) closing_flag: Rc<Cell<bool>>,

    /// <https://html.spec.whatwg.org/#inside-port>
    /// This global scope's inside port: the channel the Worker constructor
    /// set up at creation entangles it with the Worker object's outside
    /// port, and `postMessage` on this global scope acts on it, sending the
    /// messages the owner fires as message events at the worker's Worker
    /// object.  This global scope is also the inside port's message event
    /// target (run-a-worker step 12.7.1): the messages the owner posts are
    /// the inside port's arrivals (see `inbound`).
    /// Note: The inside port is implemented as a direct crossbeam channel
    /// end instead of a MessagePort (the worker's implicit port is bypassed;
    /// see dedicated_worker_agent.rs): this field is the worker→owner sender
    /// the global scope posts on.  Set once the agent's event loop is
    /// running, before the worker script is fetched.
    #[ignore_trace]
    pub(crate) inside_port: Rc<RefCell<Option<crossbeam_channel::Sender<WorkerChannelMessage>>>>,

    /// <https://html.spec.whatwg.org/#inside-port>
    /// The messages the owner posted to this worker, waiting here until the
    /// inside port's message queue is enabled: each is then delivered as a
    /// message event at this global scope (its inside port's message event
    /// target, run-a-worker step 12.7.1).  Messages that arrive before the
    /// queue is enabled wait here (a port message queue, as when start() is
    /// called or the first onmessage handler is set).
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
            inside_port: Rc::new(RefCell::new(None)),
            inbound: Rc::new(RefCell::new(WorkerMessageQueue::default())),
            location_object: gc_cell_new(None, ec),
            navigator_object: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-url>
    pub(crate) fn set_url(&self, url: Url) {
        *self.url.borrow_mut() = Some(url);
    }

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-url>
    pub(crate) fn url(&self) -> Option<Url> {
        self.url.borrow().clone()
    }

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-self>
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

    /// <https://html.spec.whatwg.org/#workernavigator>
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

    /// <https://html.spec.whatwg.org/#dom-dedicatedworkerglobalscope-name>
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
        let Some(inside_port) = self.inside_port.borrow().clone() else {
            return Ok(());
        };
        // Step 5: Let serializeWithTransferResult be
        //         StructuredSerializeWithTransfer(message, transfer). Rethrow
        //         any exceptions.
        let serialize_result = structured_serialize_with_transfer(&message, transfer, ec)?;
        // A gone owner (its document destroyed or its event loop closed)
        // drops the message, the same expected condition as a reply channel
        // send.
        let _ = inside_port.send(serialize_result);
        Ok(())
    }

    /// Set the worker→owner end of the worker's channel, once the agent's
    /// event loop is running (before the worker script is fetched).
    pub(crate) fn set_inside_port(
        &self,
        inside_port: crossbeam_channel::Sender<WorkerChannelMessage>,
    ) {
        *self.inside_port.borrow_mut() = Some(inside_port);
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
    pub(crate) fn close(&self) {
        // The close() method steps are to close a worker given this.
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
