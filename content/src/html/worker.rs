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
use crate::html::messageport::MessagePort;
use crate::html::worker_thread::WorkerContentRequest;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::syntax_error_value;

use super::GlobalScope;

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
    /// The constructor-created outside port: part of the channel set up when
    /// the worker is created, entangled with the worker's inside port once
    /// run-a-worker reaches step 12.8.  Its message event target is this
    /// Worker object.
    pub(crate) outside_port: GcCell<Option<MessagePort>>,
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
        let worker = Worker {
            event_target: EventTarget::new(ec),
            worker_id: WorkerId::new(),
            outside_port: gc_cell_new(None, ec),
        };
        let mut outside_port = MessagePort::new_port(ec)?;
        // Step 6: Set outsidePort's message event target to this.
        outside_port.set_message_event_target(worker.event_target.clone());
        // Step 7: Set this's outside port to outsidePort.
        worker.outside_port.set(Some(outside_port.clone()), ec);

        // Step 8: Let worker be this.
        // Note: `worker` above is this.
        // Step 9: Run this step in parallel:
        // Step 9.1: Run a worker given worker, workerURL, outsideSettings,
        //           outsidePort, and options.
        // Note: Dedicated workers are entirely content-process-nested: the
        // constructor reports the request to the content process's worker
        // manager (through the current realm's worker creator channel), which
        // starts the worker's dedicated worker agent (a native thread; see
        // worker_thread.rs).  The user agent is not involved.  The outside
        // port's record is registered unentangled here; run-a-worker step
        // 12.8 entangles it with the inside port once the worker realm
        // exists.  Messages posted before that entanglement are dropped by
        // the message port post message steps (targetPort is null), per the
        // spec.
        if let Some(messaging) = global_scope.channel_messaging(ec) {
            messaging.register_port(outside_port.clone(), ec);
        }
        let owner = match global_scope.worker_id() {
            Some(parent_worker_id) => WorkerOwner::Worker(parent_worker_id),
            None => WorkerOwner::Document(
                global_scope
                    .document_id()
                    .ok_or_else(|| ec.new_type_error("worker constructor: no owner document"))?,
            ),
        };
        if let Err(send_error) = worker_creator.send(WorkerContentRequest::Create(WorkerRequest {
            worker_id: worker.worker_id,
            script_url: worker_url.to_string(),
            name,
            worker_type: worker_type.as_str().to_owned(),
            owner,
            outside_port: outside_port.port_id,
        })) {
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
        // manager, which sends the worker's agent thread a `Terminate`
        // command; the thread sets the closing flag, discards its queued
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
        let Some(outside_port) = self.outside_port.borrow(ec).clone() else {
            return Ok(());
        };
        outside_port.post_message(message, transfer, ec)
    }

    /// <https://html.spec.whatwg.org/#messageeventtarget>
    pub(crate) fn enable_outside_port_queue(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Enable the outside port's message queue, as when start() is called
        // or the first onmessage handler is set on this Worker.
        let Some(outside_port) = self.outside_port.borrow(ec).clone() else {
            return;
        };
        outside_port.enable_queue(ec);
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
    /// The inside port: the port of the channel set up when the worker is
    /// created, whose message event target is this global scope.
    pub(crate) inside_port: GcCell<Option<MessagePort>>,

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
            inside_port: gc_cell_new(None, ec),
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
        let Some(inside_port) = self.inside_port.borrow(ec).clone() else {
            return Ok(());
        };
        inside_port.post_message(message, transfer, ec)
    }

    /// <https://html.spec.whatwg.org/#run-a-worker>
    pub(crate) fn set_inside_port(
        &self,
        inside_port: MessagePort,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 12.7.2: "Set worker global scope's inside port to inside port."
        // Note: Step 12.7.1 (the inside port's message event target is the
        // worker global scope) was applied when the port was created, sharing
        // this global scope's event target.
        self.inside_port.set(Some(inside_port), ec);
    }

    /// <https://html.spec.whatwg.org/#messageeventtarget>
    pub(crate) fn enable_inside_port_queue(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Enable the inside port's message queue, as when start() is called or
        // the first onmessage handler is set on this global scope.
        let Some(inside_port) = self.inside_port.borrow(ec).clone() else {
            return;
        };
        inside_port.enable_queue(ec);
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
    // Note: The closing flag makes the worker's agent-thread event loop exit
    // (see worker_thread.rs), dropping the tasks queued on its task queue;
    // the thread then reports its teardown to the content process, which
    // empties and disentangles the outside port in the owner realm.
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

/// <https://html.spec.whatwg.org/#workerlocation>
#[gc_struct]
pub(crate) struct WorkerLocation {
    /// <https://html.spec.whatwg.org/#concept-url>
    #[ignore_trace]
    url: Url,
}

impl WorkerLocation {
    pub(crate) fn new(url: Url) -> Self {
        Self { url }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-href>
    pub(crate) fn href(&self) -> String {
        // The href getter steps are to return this's WorkerGlobalScope
        // object's url, serialized.
        self.url.to_string()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-origin>
    pub(crate) fn origin(&self) -> String {
        // The origin getter steps are to return the serialization of this's
        // WorkerGlobalScope object's url's origin.
        self.url.origin().unicode_serialization()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-protocol>
    pub(crate) fn protocol(&self) -> String {
        // The protocol getter steps are to return this's WorkerGlobalScope
        // object's url's scheme, followed by ":".
        format!("{}:", self.url.scheme())
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-host>
    pub(crate) fn host(&self) -> String {
        // Step 1: Let url be this's WorkerGlobalScope object's url.
        // Step 2: If url's host is null, return the empty string.
        let Some(host) = self.url.host_str() else {
            return String::new();
        };
        // Step 3: If url's port is null, return url's host, serialized.
        // Step 4: Return url's host, serialized, followed by ":" and url's
        //         port, serialized.
        match self.url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-hostname>
    pub(crate) fn hostname(&self) -> String {
        // Step 1: Let host be this's WorkerGlobalScope object's url's host.
        // Step 2: If host is null, return the empty string.
        // Step 3: Return host, serialized.
        self.url.host_str().unwrap_or("").to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-port>
    pub(crate) fn port(&self) -> String {
        // Step 1: Let port be this's WorkerGlobalScope object's url's port.
        // Step 2: If port is null, return the empty string.
        // Step 3: Return port, serialized.
        match self.url.port() {
            Some(port) => port.to_string(),
            None => String::new(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-pathname>
    pub(crate) fn pathname(&self) -> String {
        // The pathname getter steps are to return the result of URL path
        // serializing this's WorkerGlobalScope object's url.
        self.url.path().to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-search>
    pub(crate) fn search(&self) -> String {
        // Step 1: Let query be this's WorkerGlobalScope object's url's query.
        // Step 2: If query is either null or the empty string, return the
        //         empty string.
        // Step 3: Return "?", followed by query.
        match self.url.query() {
            Some(query) if !query.is_empty() => format!("?{query}"),
            _ => String::new(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-hash>
    pub(crate) fn hash(&self) -> String {
        // Step 1: Let fragment be this's WorkerGlobalScope object's url's
        //         fragment.
        // Step 2: If fragment is either null or the empty string, return the
        //         empty string.
        // Step 3: Return "#", followed by fragment.
        match self.url.fragment() {
            Some(fragment) if !fragment.is_empty() => format!("#{fragment}"),
            _ => String::new(),
        }
    }
}

/// <https://html.spec.whatwg.org/#the-workernavigator-object>
#[gc_struct]
pub(crate) struct WorkerNavigator {}

impl WorkerNavigator {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-useragent>
    pub(crate) fn user_agent(&self) -> String {
        // The userAgent getter steps are to return this's user agent.
        // Note: The user agent string is reported by the embedder for the
        // window navigator; the worker returns the same value.
        crate::webidl::navigator_user_agent()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-platform>
    pub(crate) fn platform(&self) -> String {
        // The platform getter steps are to return this's platform.
        crate::webidl::navigator_platform()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-language>
    pub(crate) fn language(&self) -> String {
        // The language getter steps are to return this's languages[0].
        crate::webidl::navigator_language()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-online>
    pub(crate) fn on_line(&self) -> bool {
        // The onLine getter steps are to return this's online status.
        true
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-hardwareconcurrency>
    pub(crate) fn hardware_concurrency(&self) -> u64 {
        // The hardwareConcurrency getter steps are to return this's
        // hardware concurrency.
        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get() as u64)
            .unwrap_or(1)
    }
}
