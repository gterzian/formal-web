use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    vec::Vec,
};

use super::windowproxy::{WindowProxy, WindowProxyBacking};
use super::{
    ChannelMessaging, Window, create_a_new_browsing_context_and_document,
    environment_settings_object::EnvironmentSettingsObject,
};

use super::timers::TimerRealm;

use super::dedicated_worker_agent::{
    OwnedWorkerChannel, WorkerChannelMessage, WorkerContentRequest, WorkerMessageQueue,
};

use blitz_dom::BaseDocument;
use ipc::IpcSender;
use ipc_messages::content::{
    DocumentId, Event as ContentEvent, NavigableId, WindowTimerKey, WorkerId,
};
use ipc_messages::media::VideoPaintId;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use log::{debug, error};

use super::environment_settings_object::RealmWiring;
use super::event_loop::{EventLoopTaskSources, Task};
use crate::dom::event::EventTarget;
use crate::js::{Engine, Types};
use crate::webidl::Callback;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        debug!("[timer-debug][global] {}", message.as_ref());
    }
}

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Debug, Clone, Copy)]
pub enum GlobalScopeKind {
    /// <https://html.spec.whatwg.org/#window>
    Window,
    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    Worker,
}

/// <https://html.spec.whatwg.org/#global-object>
#[gc_struct]
pub struct CachedNodeObject {
    /// <https://dom.spec.whatwg.org/#interface-node>
    #[ignore_trace]
    pub node_id: usize,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub object: JsObject,
}

/// A cached WindowProxy for a navigable, keyed by navigable id.  The cache
/// lives on the realm's GlobalScope so the same WindowProxy JS object is
/// returned for `event.source`, `iframe.contentWindow`, and `window.open`
/// results referring to the same navigable.  The same entry also records the
/// iframe node whose content navigable the WindowProxy backs (the
/// `contentWindow` binding resolves the navigable from the element's node
/// id).  `window_proxy` is the domain WindowProxy whose backing cell carries
/// the navigable's active Window; `js_object` is the ECMAScript Proxy
/// wrapping the platform object that is handed to JavaScript.
/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
#[gc_struct]
pub struct CachedWindowProxy {
    /// <https://html.spec.whatwg.org/#navigable>
    #[ignore_trace]
    pub navigable_id: ipc_messages::content::NavigableId,

    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    /// The domain WindowProxy (the platform object's data, shared with the
    /// platform object created from it via the backing cell); `None` until
    /// the proxy for the navigable is first created.
    pub window_proxy: Option<WindowProxy>,

    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    /// The JS object handed to JavaScript (the ECMAScript Proxy wrapping the
    /// window proxy's platform object); `None` until the realm first
    /// accesses the window.
    pub js_object: Option<JsObject>,

    /// <https://html.spec.whatwg.org/#content-navigable>
    /// The iframe element's node id in this realm's document when the
    /// navigable is that element's content navigable.
    #[ignore_trace]
    pub iframe_node_id: Option<usize>,
}

/// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
#[gc_struct]
pub struct AnimationFrameCallback {
    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[ignore_trace]
    pub handle: u32,

    /// <https://webidl.spec.whatwg.org/#idl-callback-function>
    pub callback: Callback,
}

/// <https://html.spec.whatwg.org/#timers>
#[gc_struct]
pub enum TimerHandler {
    Function {
        /// <https://webidl.spec.whatwg.org/#idl-callback-function>
        callback: Callback,
    },
    String {
        /// <https://html.spec.whatwg.org/#timerhandler>
        #[ignore_trace]
        source: String,
    },
}

/// <https://html.spec.whatwg.org/#timers>
#[gc_struct]
pub struct WindowTimer {
    /// <https://html.spec.whatwg.org/#map-of-settimeout-and-setinterval-ids>
    #[ignore_trace]
    pub id: u32,

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    #[ignore_trace]
    pub timer_key: WindowTimerKey,

    /// <https://html.spec.whatwg.org/#timerhandler>
    pub handler: TimerHandler,

    /// <https://html.spec.whatwg.org/#timers>
    pub arguments: Vec<JsValue>,

    /// <https://html.spec.whatwg.org/#timers>
    #[ignore_trace]
    pub repeat: bool,

    /// <https://html.spec.whatwg.org/#timers>
    #[ignore_trace]
    pub timeout_ms: u32,
}

/// <https://html.spec.whatwg.org/#global-object>
#[gc_struct]
pub struct GlobalScope {
    /// <https://html.spec.whatwg.org/#global-object>
    #[ignore_trace]
    pub kind: GlobalScopeKind,

    /// <https://html.spec.whatwg.org/#concept-document-window>
    /// The DOM document the global scope resolves nodes against.  The outer
    /// `Rc<RefCell<..>>` slot is shared across every `GlobalScope` clone so a
    /// step-6 window reuse (a new document taking over an existing realm) can
    /// re-point it for all clones.
    #[ignore_trace]
    document: Rc<RefCell<Rc<RefCell<BaseDocument>>>>,

    /// <https://dom.spec.whatwg.org/#interface-document>
    document_object: GcCell<Option<JsObject>>,

    /// <https://html.spec.whatwg.org/#dom-location>
    location_object: GcCell<Option<JsObject>>,

    /// WindowProxy cache entries for navigables (and the iframe node each
    /// navigable is the content navigable of), keyed by navigable id.
    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    window_proxies: GcCell<Vec<CachedWindowProxy>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    node_objects: GcCell<Vec<CachedNodeObject>>,

    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[ignore_trace]
    animation_frame_callback_identifier: Cell<u32>,

    /// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
    animation_frame_callbacks: GcCell<Vec<AnimationFrameCallback>>,

    /// <https://html.spec.whatwg.org/#timers>
    #[ignore_trace]
    timer_callback_identifier: Cell<u32>,

    /// <https://html.spec.whatwg.org/#map-of-settimeout-and-setinterval-ids>
    window_timers: GcCell<Vec<WindowTimer>>,

    /// <https://html.spec.whatwg.org/#timer-nesting-level>
    #[ignore_trace]
    current_timer_nesting_level: Cell<Option<u32>>,

    /// <https://html.spec.whatwg.org/#task-source>
    #[ignore_trace]
    task_sources: Rc<RefCell<Option<EventLoopTaskSources>>>,

    /// <https://html.spec.whatwg.org/#concept-navigable>
    #[ignore_trace]
    source_navigable_id: Rc<Cell<Option<NavigableId>>>,

    /// <https://html.spec.whatwg.org/#responsible-event-loop>
    /// The id of this global object's responsible event loop (its relevant
    /// agent's event loop), set by the content process at document creation.
    #[ignore_trace]
    event_loop_id: Rc<Cell<Option<ipc_messages::content::EventLoopId>>>,

    /// The id of the worker this global scope belongs to, when the global
    /// object is a worker global scope.  The timer machinery uses it to
    /// schedule and route the realm's timers (a worker realm has no
    /// document).
    #[ignore_trace]
    worker_id: Rc<Cell<Option<WorkerId>>>,

    /// Per-realm channel messaging state (ports, message queues, transfer
    /// state), created lazily on first port use.
    channel_messaging: GcCell<Option<ChannelMessaging>>,

    /// TLA trace sender for the MessagePort spec, set by the content process
    /// at document creation (mirrors the `event_sender` wiring).
    #[ignore_trace]
    trace_sender: Rc<RefCell<Option<verification::TraceSender>>>,

    /// <https://html.spec.whatwg.org/#parent-navigable>
    /// The parent of this document's navigable in the navigable tree.
    /// None indicates a top-level traversable.
    #[ignore_trace]
    parent_traversable_id: Rc<Cell<Option<NavigableId>>>,

    /// <https://html.spec.whatwg.org/#traversable-navigable>
    /// The top-level traversable for this navigable tree.
    #[ignore_trace]
    top_level_traversable_id: Rc<Cell<Option<NavigableId>>>,

    /// <https://html.spec.whatwg.org/#concept-document>
    /// The document id for the document associated with this global scope.
    #[ignore_trace]
    document_id: Rc<RefCell<Option<DocumentId>>>,

    /// Sender for content-to-user-agent IPC events (e.g. navigation requests).
    #[ignore_trace]
    event_sender: Rc<RefCell<Option<IpcSender<ContentEvent>>>>,

    /// Channel to the content process's worker manager: the `Worker`
    /// constructor reports its creation request here, and `terminate()` its
    /// termination request.  Dedicated workers are entirely
    /// content-process-nested, so worker creation and termination never
    /// involve the user agent.
    #[ignore_trace]
    worker_creator: Rc<RefCell<Option<crossbeam_channel::Sender<WorkerContentRequest>>>>,

    /// The dedicated workers this realm owns (it created their Worker
    /// platform objects): the owner-side delivery state of each worker's
    /// channel (its Worker object's event target and the message queue the
    /// worker's posts flow through).  This replaces the outside port
    /// records of the port-based model (the worker's implicit port is
    /// bypassed; see dedicated_worker_agent.rs).  A GcCell of a Vec, not a
    /// HashMap, because the GC trace impls cover Vec but not HashMap.
    owned_workers: GcCell<Vec<OwnedWorkerChannel>>,

    /// Shared registry for newly-created traversable documents (window.open).
    /// Set by `ContentProcess` before running JS that may trigger
    /// `the_rules_for_choosing_a_navigable`. Both GlobalScope (to insert)
    /// and ContentProcess (to retrieve) share the same `Rc`, so no separate
    /// flush step is needed.
    #[ignore_trace]
    new_document_registry: Rc<
        RefCell<
            Option<
                Rc<
                    RefCell<
                        HashMap<DocumentId, (EnvironmentSettingsObject, Rc<RefCell<BaseDocument>>)>,
                    >,
                >,
            >,
        >,
    >,

    /// Shared registry mapping (document_id, node_id) → VideoPaintId.
    /// Set by `ContentProcess` during document creation so that both
    /// `resource_selection_algorithm` (to insert) and
    /// `ContentProcess::build_frame_composition_metadata` (to read) share
    /// the same `Rc`.
    #[ignore_trace]
    video_paint_registry:
        Rc<RefCell<Option<Rc<RefCell<HashMap<(DocumentId, usize), VideoPaintId>>>>>>,

    /// Direct sender to the graphics process (composition + media).
    #[ignore_trace]
    graphics_sender: Rc<RefCell<Option<IpcSender<ipc_messages::graphics::GraphicsCommand>>>>,

    /// <https://html.spec.whatwg.org/#concept-document-creation-url>
    /// The creation URL of this window's Document.
    #[ignore_trace]
    creation_url: Rc<RefCell<Option<url::Url>>>,

    /// Consolidated wasm state (pending requests, resolvers, counter).
    #[cfg(all(boa_backend, feature = "wasm"))]
    wasm_state: Option<crate::wasm::WasmState>,
}

impl GlobalScope {
    pub fn new(
        kind: GlobalScopeKind,
        document: Rc<RefCell<BaseDocument>>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            kind,
            document: Rc::new(RefCell::new(document)),
            document_object: gc_cell_new(None, ec),
            location_object: gc_cell_new(None, ec),
            window_proxies: gc_cell_new(Vec::new(), ec),
            node_objects: gc_cell_new(Vec::new(), ec),
            animation_frame_callback_identifier: Cell::new(0),
            animation_frame_callbacks: gc_cell_new(Vec::new(), ec),
            timer_callback_identifier: Cell::new(0),
            window_timers: gc_cell_new(Vec::new(), ec),
            current_timer_nesting_level: Cell::new(None),
            task_sources: Rc::new(RefCell::new(None)),
            source_navigable_id: Rc::new(Cell::new(None)),
            event_loop_id: Rc::new(Cell::new(None)),
            worker_id: Rc::new(Cell::new(None)),
            channel_messaging: gc_cell_new(None, ec),
            trace_sender: Rc::new(RefCell::new(None)),
            parent_traversable_id: Rc::new(Cell::new(None)),
            top_level_traversable_id: Rc::new(Cell::new(None)),
            document_id: Rc::new(RefCell::new(None)),
            event_sender: Rc::new(RefCell::new(None)),
            worker_creator: Rc::new(RefCell::new(None)),
            owned_workers: gc_cell_new(Vec::new(), ec),

            new_document_registry: Rc::new(RefCell::new(None)),
            video_paint_registry: Rc::new(RefCell::new(None)),
            graphics_sender: Rc::new(RefCell::new(None)),

            creation_url: Rc::new(RefCell::new(None)),
            #[cfg(all(boa_backend, feature = "wasm"))]
            wasm_state: Some(crate::wasm::WasmState::new(ec)),
        }
    }

    fn next_timer_id(&self, ec: &mut dyn ExecutionContext<Types>) -> u32 {
        let timers = self.window_timers.borrow(ec);
        let mut handle = self.timer_callback_identifier.get();

        loop {
            handle = handle.wrapping_add(1);
            if handle == 0 {
                continue;
            }
            if timers.iter().all(|entry| entry.id != handle) {
                break;
            }
        }

        drop(timers);
        self.timer_callback_identifier.set(handle);
        handle
    }

    fn next_timer_key(&self) -> Result<WindowTimerKey, String> {
        Ok(WindowTimerKey::new())
    }

    pub(crate) fn document(&self) -> Rc<RefCell<BaseDocument>> {
        self.document.borrow().clone()
    }

    /// <https://html.spec.whatwg.org/#initialise-the-document-object> step 6
    /// Re-point this realm's associated Document, origin and creation URL at a
    /// new document that is taking over the (reused) Window: the DOM document
    /// the global scope resolves nodes against, the stored platform Document
    /// object, the document-scoped bookkeeping (document id, creation URL), and
    /// the per-document caches (node wrappers, location) so the new document's
    /// nodes resolve to fresh wrappers and `location` reflects the new URL.
    pub(crate) fn repoint_document(
        &self,
        document: Rc<RefCell<BaseDocument>>,
        document_object: JsObject,
        document_id: DocumentId,
        creation_url: url::Url,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        *self.document.borrow_mut() = document;
        self.document_object.borrow_mut(ec).replace(document_object);
        self.node_objects.borrow_mut(ec).clear();
        self.location_object.borrow_mut(ec).take();
        self.document_id.borrow_mut().replace(document_id);
        self.creation_url.borrow_mut().replace(creation_url);
    }

    pub(crate) fn set_navigation_info(
        &self,
        source_navigable_id: NavigableId,
        event_sender: IpcSender<ContentEvent>,
    ) {
        self.source_navigable_id.set(Some(source_navigable_id));
        self.event_sender.borrow_mut().replace(event_sender);
    }

    /// Set the content-to-user-agent event sender of a worker realm's global
    /// scope (a worker has no navigable, so no source navigable id).
    pub(crate) fn set_event_sender(&self, event_sender: IpcSender<ContentEvent>) {
        self.event_sender.borrow_mut().replace(event_sender);
    }

    pub(crate) fn set_navigable_hierarchy(
        &self,
        parent_traversable_id: Option<NavigableId>,
        top_level_traversable_id: NavigableId,
    ) {
        self.parent_traversable_id.set(parent_traversable_id);
        self.top_level_traversable_id
            .set(Some(top_level_traversable_id));
    }

    pub(crate) fn parent_traversable_id(&self) -> Option<NavigableId> {
        self.parent_traversable_id.get()
    }

    pub(crate) fn top_level_traversable_id(&self) -> Option<NavigableId> {
        self.top_level_traversable_id.get()
    }

    pub(crate) fn source_navigable_id(&self) -> Option<NavigableId> {
        self.source_navigable_id.get()
    }

    pub(crate) fn set_event_loop_id(&self, event_loop_id: ipc_messages::content::EventLoopId) {
        self.event_loop_id.set(Some(event_loop_id));
    }

    pub(crate) fn event_loop_id(&self) -> Option<ipc_messages::content::EventLoopId> {
        self.event_loop_id.get()
    }

    pub(crate) fn worker_id(&self) -> Option<WorkerId> {
        self.worker_id.get()
    }

    /// Set the TLA trace sender for the MessagePort spec.
    pub(crate) fn set_trace_sender(&self, sender: Option<verification::TraceSender>) {
        *self.trace_sender.borrow_mut() = sender;
    }

    pub(crate) fn trace_sender(&self) -> Option<verification::TraceSender> {
        self.trace_sender.borrow().clone()
    }

    /// The per-realm channel messaging state, created on first use.
    pub(crate) fn channel_messaging(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<ChannelMessaging> {
        let event_loop_id = self.event_loop_id()?;
        let existing = self.channel_messaging.borrow(ec).clone();
        if let Some(existing) = existing {
            return Some(existing);
        }
        let created = ChannelMessaging::new(
            event_loop_id,
            self.trace_sender(),
            self.task_sources().ok()?.task_queue(),
            ec,
        );
        self.channel_messaging.set(Some(created.clone()), ec);
        Some(created)
    }

    /// <https://html.spec.whatwg.org/#task-source>
    pub(crate) fn set_task_sources(
        &self,
        document_id: DocumentId,
        task_sources: EventLoopTaskSources,
    ) {
        self.document_id.borrow_mut().replace(document_id);
        *self.task_sources.borrow_mut() = Some(task_sources);
    }

    /// Wire a worker realm's global scope to its own agent's event loop:
    /// its task sources (the worker's own task queue and timer map) and
    /// worker id (a worker realm has no document).
    /// <https://html.spec.whatwg.org/#task-source>
    pub(crate) fn set_worker_task_sources(
        &self,
        worker_id: WorkerId,
        task_sources: EventLoopTaskSources,
    ) {
        self.worker_id.set(Some(worker_id));
        *self.task_sources.borrow_mut() = Some(task_sources);
    }

    pub(crate) fn document_id(&self) -> Option<DocumentId> {
        *self.document_id.borrow()
    }

    pub(crate) fn event_sender(&self) -> Option<IpcSender<ContentEvent>> {
        self.event_sender.borrow().clone()
    }

    /// Set the channel to the content process's worker manager.
    pub(crate) fn set_worker_creator(
        &self,
        sender: crossbeam_channel::Sender<WorkerContentRequest>,
    ) {
        *self.worker_creator.borrow_mut() = Some(sender);
    }

    /// The channel to the content process's worker manager, if set.
    pub(crate) fn worker_creator(&self) -> Option<crossbeam_channel::Sender<WorkerContentRequest>> {
        self.worker_creator.borrow().clone()
    }

    /// <https://html.spec.whatwg.org/#dedicated-workers-and-the-worker-interface>
    /// The Worker constructor registers the worker this realm owns: its
    /// Worker object's event target is the target of the message events the
    /// messages the worker posts back fire at, and its message queue starts
    /// disabled (a port message queue can be enabled, and is initially
    /// disabled).
    pub(crate) fn register_owned_worker(
        &self,
        worker_id: WorkerId,
        worker_target: EventTarget,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut owned_workers = self.owned_workers.borrow_mut(ec);
        if owned_workers
            .iter()
            .any(|record| record.worker_id == worker_id)
        {
            return;
        }
        owned_workers.push(OwnedWorkerChannel {
            worker_target,
            worker_id,
            queue: Rc::new(RefCell::new(WorkerMessageQueue::default())),
        });
    }

    /// The Worker object's event target of a worker this realm owns, if
    /// any: the target the message and error events of that worker fire at.
    pub(crate) fn owned_worker_event_target(
        &self,
        worker_id: WorkerId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<EventTarget> {
        self.owned_workers
            .borrow(ec)
            .iter()
            .find(|record| record.worker_id == worker_id)
            .map(|record| record.worker_target.clone())
    }

    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    /// The owner's event loop received a message a worker this realm owns
    /// posted: when the worker's message queue is enabled the message is
    /// queued as a message task (firing a message event at the worker's
    /// Worker object); otherwise it waits in the queue until the queue is
    /// enabled (run-a-worker step 12.14, or the first onmessage handler on
    /// the Worker object).  A worker this realm no longer owns drops the
    /// message.
    pub(crate) fn handle_worker_posted_message(
        &self,
        worker_id: WorkerId,
        payload: WorkerChannelMessage,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let queue_task = {
            let mut owned_workers = self.owned_workers.borrow_mut(ec);
            let Some(record) = owned_workers
                .iter_mut()
                .find(|record| record.worker_id == worker_id)
            else {
                return;
            };
            let mut queue = record.queue.borrow_mut();
            if queue.enabled {
                Some(payload)
            } else {
                queue.pending.push_back(payload);
                None
            }
        };
        if let Some(payload) = queue_task {
            self.queue_worker_message_task(worker_id, payload);
        }
    }

    /// <https://html.spec.whatwg.org/#messageeventtarget>
    /// Enable the message queue of a worker this realm owns (run-a-worker
    /// step 12.14, or the first onmessage handler on its Worker object): the
    /// messages the worker posted while the queue was disabled now fire as
    /// message tasks, in order.
    pub(crate) fn enable_owned_worker_messages(
        &self,
        worker_id: WorkerId,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let pending: Vec<WorkerChannelMessage> = {
            let mut owned_workers = self.owned_workers.borrow_mut(ec);
            let Some(record) = owned_workers
                .iter_mut()
                .find(|record| record.worker_id == worker_id)
            else {
                return;
            };
            let mut queue = record.queue.borrow_mut();
            queue.enabled = true;
            queue.pending.drain(..).collect()
        };
        for payload in pending {
            self.queue_worker_message_task(worker_id, payload);
        }
    }

    /// Queue one message a worker this realm owns posted as a message task
    /// on this realm's event loop, firing a message event at the worker's
    /// Worker object.
    fn queue_worker_message_task(&self, worker_id: WorkerId, payload: WorkerChannelMessage) {
        let Ok(task_sources) = self.task_sources() else {
            error!("realm has no task sources; dropping worker message");
            return;
        };
        task_sources
            .task_queue()
            .queue_a_task(Task::RunWorkerOutboundMessage { worker_id, payload });
    }

    /// <https://html.spec.whatwg.org/#terminate-a-worker>
    /// A worker this realm owns closed: drop its channel registry entry,
    /// discarding the messages that had not yet fired (terminate-a-worker
    /// step 4 empties the queue of the port the worker's implicit port is
    /// entangled with; run-a-worker step 12.20 disentangles the worker's
    /// ports).
    pub(crate) fn discard_owned_worker(
        &self,
        worker_id: WorkerId,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut owned_workers = self.owned_workers.borrow_mut(ec);
        owned_workers.retain(|record| record.worker_id != worker_id);
    }

    /// <https://html.spec.whatwg.org/#event-loop-processing-model>
    pub(crate) fn task_sources(&self) -> Result<EventLoopTaskSources, String> {
        self.task_sources
            .borrow()
            .clone()
            .ok_or_else(|| String::from("event loop task sources are not installed"))
    }

    pub(crate) fn document_object(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<JsObject> {
        self.document_object.borrow(ec).clone()
    }

    pub(crate) fn store_document_object(
        &self,
        object: JsObject,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.document_object.borrow_mut(ec).replace(object);
    }

    pub(crate) fn location_object(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<JsObject> {
        self.location_object.borrow(ec).clone()
    }

    pub(crate) fn store_location_object(
        &self,
        object: JsObject,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.location_object.borrow_mut(ec).replace(object);
    }

    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    /// Returns the (domain WindowProxy, JS object) pair cached for the
    /// navigable, if any.
    pub(crate) fn cached_window_proxy_state(
        &self,
        navigable_id: ipc_messages::content::NavigableId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> (Option<WindowProxy>, Option<JsObject>) {
        let mut state = (None, None);
        for entry in self.window_proxies.borrow(ec).iter() {
            if entry.navigable_id == navigable_id {
                state = (entry.window_proxy.clone(), entry.js_object.clone());
            }
        }
        state
    }

    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    pub(crate) fn cache_window_proxy(
        &self,
        navigable_id: ipc_messages::content::NavigableId,
        window_proxy: WindowProxy,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut entries = self.window_proxies.borrow_mut(ec);
        match entries
            .iter_mut()
            .find(|entry| entry.navigable_id == navigable_id)
        {
            Some(entry) => {
                if entry.window_proxy.is_none() {
                    entry.window_proxy = Some(window_proxy);
                }
            }
            None => entries.push(CachedWindowProxy {
                navigable_id,
                window_proxy: Some(window_proxy),
                js_object: None,
                iframe_node_id: None,
            }),
        }
    }

    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    pub(crate) fn cache_window_proxy_object(
        &self,
        navigable_id: ipc_messages::content::NavigableId,
        js_object: JsObject,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut entries = self.window_proxies.borrow_mut(ec);
        match entries
            .iter_mut()
            .find(|entry| entry.navigable_id == navigable_id)
        {
            Some(entry) => {
                if entry.js_object.is_none() {
                    entry.js_object = Some(js_object);
                }
            }
            None => entries.push(CachedWindowProxy {
                navigable_id,
                window_proxy: None,
                js_object: Some(js_object),
                iframe_node_id: None,
            }),
        }
    }

    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    /// Re-point the backing of every cached WindowProxy for a navigable.
    /// `create_window_proxy` seeds the backing when a same-process Window
    /// becomes known; navigation commit calls this with
    /// `WindowProxyBacking::CrossContentProcess` when the navigable's active
    /// document was created in another content process, or with
    /// `WindowProxyBacking::SameContentProcess` when the navigation stays in
    /// this process.  The backing cell is shared with the platform objects
    /// created from the cached clones, so the traps read the new backing.
    pub(crate) fn set_window_proxy_backing(
        &self,
        navigable_id: ipc_messages::content::NavigableId,
        backing: WindowProxyBacking,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let proxies: Vec<WindowProxy> = self
            .window_proxies
            .borrow(ec)
            .iter()
            .filter(|entry| entry.navigable_id == navigable_id)
            .filter_map(|entry| entry.window_proxy.clone())
            .collect();
        for proxy in proxies {
            proxy.set_backing(backing.clone(), ec);
        }
    }

    /// <https://html.spec.whatwg.org/#content-navigable>
    /// Look up the content navigable registered for an iframe node id in
    /// this realm's document.
    pub(crate) fn content_navigable_for_iframe(
        &self,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<ipc_messages::content::NavigableId> {
        self.window_proxies
            .borrow(ec)
            .iter()
            .find(|entry| entry.iframe_node_id == Some(node_id))
            .map(|entry| entry.navigable_id)
    }

    /// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
    /// Registers the iframe node -> content navigable mapping used by the
    /// `contentWindow` getter, and caches the navigable's WindowProxy with
    /// the child document's Window (created in this process) as its backing,
    /// so the `contentWindow` binding hands out a locally-backed WindowProxy.
    pub(crate) fn register_iframe_content_navigable(
        &self,
        node_id: usize,
        navigable_id: ipc_messages::content::NavigableId,
        local_window: Option<(Window, JsObject)>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Clone the current entry state out of the cell before creating the
        // window proxy (which allocates and must not run under the cell
        // borrow).
        let existing = {
            let entries = self.window_proxies.borrow(ec);
            entries
                .iter()
                .find(|entry| entry.navigable_id == navigable_id)
                .cloned()
        };

        // A node can only be the content navigable of one element.
        {
            let mut entries = self.window_proxies.borrow_mut(ec);
            for entry in entries.iter_mut() {
                if entry.iframe_node_id == Some(node_id) {
                    entry.iframe_node_id = None;
                }
            }
        }

        let window_proxy = if existing
            .as_ref()
            .and_then(|entry| entry.window_proxy.as_ref())
            .is_some()
        {
            None
        } else {
            let backing = match local_window {
                Some((window, js_object)) => {
                    WindowProxyBacking::SameContentProcess { window, js_object }
                }
                None => WindowProxyBacking::CrossContentProcess,
            };
            Some(WindowProxy::new(navigable_id, backing, ec))
        };

        let mut entries = self.window_proxies.borrow_mut(ec);
        match entries
            .iter_mut()
            .find(|entry| entry.navigable_id == navigable_id)
        {
            Some(entry) => {
                entry.iframe_node_id = Some(node_id);
                if entry.window_proxy.is_none() {
                    entry.window_proxy = window_proxy;
                }
            }
            None => entries.push(CachedWindowProxy {
                navigable_id,
                window_proxy,
                js_object: None,
                iframe_node_id: Some(node_id),
            }),
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#destroy-a-child-navigable>
    /// Unregisters the iframe node -> content navigable mapping.
    pub(crate) fn unregister_iframe_content_navigable(
        &self,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut entries = self.window_proxies.borrow_mut(ec);
        for entry in entries.iter_mut() {
            if entry.iframe_node_id == Some(node_id) {
                entry.iframe_node_id = None;
            }
        }
    }

    pub(crate) fn cached_node_object(
        &self,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        self.node_objects
            .borrow(ec)
            .iter()
            .find(|entry| entry.node_id == node_id)
            .map(|entry| entry.object.clone())
    }

    pub(crate) fn cache_node_object(
        &self,
        node_id: usize,
        object: JsObject,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.node_objects
            .borrow_mut(ec)
            .push(CachedNodeObject { node_id, object });
    }

    pub(crate) fn invalidate_cached_node_ids(
        &self,
        node_ids: &[usize],
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        if node_ids.is_empty() {
            return;
        }

        let node_ids = node_ids.iter().copied().collect::<HashSet<_>>();
        self.node_objects
            .borrow_mut(ec)
            .retain(|entry| !node_ids.contains(&entry.node_id));
    }

    /// Clone every cached node wrapper out of the cache, for teardown walks
    /// that must clear per-node state without holding the cell borrow.
    pub(crate) fn cached_node_objects(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<JsObject> {
        self.node_objects
            .borrow(ec)
            .iter()
            .map(|entry| entry.object.clone())
            .collect()
    }

    /// <https://html.spec.whatwg.org/#dom-animationframeprovider-requestanimationframe>
    pub(crate) fn request_animation_frame(
        &self,
        callback: Callback,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> u32 {
        let callbacks = self.animation_frame_callbacks.borrow(ec);
        let mut handle = self.animation_frame_callback_identifier.get();

        loop {
            handle = handle.wrapping_add(1);
            if handle == 0 {
                continue;
            }
            if callbacks.iter().all(|entry| entry.handle != handle) {
                break;
            }
        }

        drop(callbacks);
        self.animation_frame_callback_identifier.set(handle);
        self.animation_frame_callbacks
            .borrow_mut(ec)
            .push(AnimationFrameCallback { handle, callback });
        handle
    }

    /// <https://html.spec.whatwg.org/#timer-nesting-level>
    pub(crate) fn current_timer_nesting_level(&self) -> Option<u32> {
        self.current_timer_nesting_level.get()
    }

    /// <https://html.spec.whatwg.org/#timer-nesting-level>
    pub(crate) fn set_current_timer_nesting_level(
        &self,
        nesting_level: Option<u32>,
    ) -> Option<u32> {
        let previous = self.current_timer_nesting_level.get();
        self.current_timer_nesting_level.set(nesting_level);
        previous
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn timer_initialization_steps(
        &self,
        previous_id: Option<u32>,
        handler: TimerHandler,
        arguments: Vec<JsValue>,
        repeat: bool,
        timeout_ms: u32,
        nesting_level: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<u32, String> {
        // Note: This helper continues the `timer initialization steps` algorithm at the `GlobalScope`-owned pieces. The mixin implementation already handled the preliminary timeout conversion, clamping, and task setup.

        // Step 2: "If previousId was given, let id be previousId; otherwise, let id be an implementation-defined integer that is greater than zero and does not already exist in global's map of setTimeout and setInterval IDs."
        let timer_id = previous_id.unwrap_or_else(|| self.next_timer_id(ec));

        // Step 11: "Set uniqueHandle to the result of running steps after a timeout given global, \"setTimeout/setInterval\", timeout, and completionStep."
        // Note: The content process main thread runs the in-parallel steps of "run steps
        // after a timeout"; recording the expiry time here is what its main loop waits on.
        let timer_key = self.next_timer_key()?;
        log_timer_debug(format!(
            "schedule timer id={} key={} timeout_ms={} nesting={} repeat={} previous_id={:?}",
            timer_id, timer_key, timeout_ms, nesting_level, repeat, previous_id
        ));
        let realm = match self.worker_id() {
            Some(worker_id) => TimerRealm::Worker(worker_id),
            None => TimerRealm::Document(self.document_id().ok_or_else(|| {
                String::from("window timer scheduled without an associated document")
            })?),
        };
        self.task_sources()?.run_steps_after_a_timeout(
            realm,
            timer_key,
            timeout_ms,
            timer_id,
            nesting_level,
        );

        // Step 12: "Set global's map of setTimeout and setInterval IDs[id] to uniqueHandle."
        let mut timers = self.window_timers.borrow_mut(ec);
        if let Some(index) = timers.iter().position(|entry| entry.id == timer_id) {
            timers.remove(index);
        }
        timers.push(WindowTimer {
            id: timer_id,
            timer_key,
            handler,
            arguments,
            repeat,
            timeout_ms,
        });

        // Step 13: "Return id."
        Ok(timer_id)
    }

    /// <https://html.spec.whatwg.org/#dom-cleartimeout>
    pub(crate) fn clear_timer(&self, timer_id: u32, ec: &mut dyn ExecutionContext<Types>) {
        // Note: This is the shared storage helper used by both `clearTimeout()` and `clearInterval()`.

        // Step 1: "Remove this's map of setTimeout and setInterval IDs[id]."
        let removed_timer = {
            let mut timers = self.window_timers.borrow_mut(ec);
            timers
                .iter()
                .position(|entry| entry.id == timer_id)
                .map(|index| timers.remove(index))
        };
        let Some(removed_timer) = removed_timer else {
            return;
        };
        log_timer_debug(format!(
            "clear timer id={} key={}",
            removed_timer.id, removed_timer.timer_key
        ));
        let Ok(task_sources) = self.task_sources() else {
            return;
        };
        task_sources.remove_active_timer(removed_timer.timer_key);
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn window_timer(
        &self,
        timer_id: u32,
        timer_key: WindowTimerKey,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<WindowTimer> {
        // Note: This model-local lookup exposes the stored `(id, uniqueHandle)` registration so the queued timer task can check whether the timer still exists and still maps to the same handle before running the handler.
        self.window_timers
            .borrow(ec)
            .iter()
            .find(|entry| entry.id == timer_id && entry.timer_key == timer_key)
            .cloned()
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn complete_window_timer(
        &self,
        timer_id: u32,
        timer_key: WindowTimerKey,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<(), String> {
        // Note: This helper continues the queued timer task after the handler and the stale-handle checks have already run inside `EnvironmentSettingsObject::run_window_timer`.
        let timer = self.window_timer(timer_id, timer_key, ec);
        let Some(timer) = timer else {
            log_timer_debug(format!(
                "complete timer id={} key={} skipped_missing",
                timer_id, timer_key
            ));
            return Ok(());
        };

        log_timer_debug(format!(
            "complete timer id={} key={} repeat={}",
            timer_id, timer_key, timer.repeat
        ));

        // Step 12: "Otherwise, remove global's map of setTimeout and setInterval IDs[id]."
        if !timer.repeat {
            self.window_timers
                .borrow_mut(ec)
                .retain(|entry| !(entry.id == timer_id && entry.timer_key == timer_key));
            return Ok(());
        }

        // Step 11: "If repeat is true, then perform the timer initialization steps again, given global, handler, timeout, arguments, true, and id."
        let next_nesting_level = self
            .current_timer_nesting_level()
            .unwrap_or(0)
            .saturating_add(1);
        let task_sources = self.task_sources()?;
        let realm = match self.worker_id() {
            Some(worker_id) => TimerRealm::Worker(worker_id),
            None => TimerRealm::Document(self.document_id().ok_or_else(|| {
                String::from("window timer rescheduled without an associated document")
            })?),
        };
        let next_timer_key = self.next_timer_key()?;
        log_timer_debug(format!(
            "reschedule interval id={} old_key={} new_key={} timeout_ms={} nesting={}",
            timer_id, timer_key, next_timer_key, timer.timeout_ms, next_nesting_level
        ));
        task_sources.run_steps_after_a_timeout(
            realm,
            next_timer_key,
            timer.timeout_ms,
            timer_id,
            next_nesting_level,
        );

        let mut timers = self.window_timers.borrow_mut(ec);
        let Some(entry) = timers
            .iter_mut()
            .find(|entry| entry.id == timer_id && entry.timer_key == timer_key)
        else {
            return Ok(());
        };
        entry.timer_key = next_timer_key;
        Ok(())
    }

    pub(crate) fn clear_all_timers(&self, ec: &mut dyn ExecutionContext<Types>) {
        let cleared_timers = {
            let mut timers = self.window_timers.borrow_mut(ec);
            std::mem::take(&mut *timers)
        };
        let Ok(task_sources) = self.task_sources() else {
            return;
        };
        for timer in cleared_timers {
            task_sources.remove_active_timer(timer.timer_key);
        }
    }

    /// <https://html.spec.whatwg.org/#creating-a-new-auxiliary-browsing-context>
    pub(crate) fn create_auxiliary_context_document(
        &self,
        parent_engine: Option<&mut Engine>,
        new_traversable_id: NavigableId,
        new_document_id: DocumentId,
    ) -> Result<
        (
            JsObject,
            Window,
            super::environment_settings_object::EnvironmentSettingsObject,
            Rc<RefCell<BaseDocument>>,
        ),
        String,
    > {
        let event_sender = self
            .event_sender()
            .ok_or_else(|| String::from("GlobalScope has no event sender"))?;
        // Step 7 of "creating a new browsing context and document": "Let origin be the
        // result of determining the origin given about:blank, sandboxFlags, and
        // creatorOrigin."
        // Note: The about:blank document inherits the opener's origin (the creator
        // origin of this window.open), so the initial about:blank Window can be reused for
        // a later same-origin navigation (step 6 of `initialise-the-document-object`).
        let creator_origin = self
            .creation_url()
            .map(|url| url.origin())
            .filter(|origin| !matches!(origin, url::Origin::Opaque(_)))
            .map(|origin| super::environment_settings_object::Origin {
                serialized: origin.unicode_serialization(),
            });
        // Step 4: Let browsingContext and document be the result of creating a
        // new browsing context and document with opener's active document, null,
        // and group.
        // Note: Content-process portion of this step: the document, realm,
        // Window and environment settings object are created by
        // `create_a_new_browsing_context_and_document` (steps 10, 13, 15, 22 of
        // "creating a new browsing context and document").  The browsing
        // context, group membership and agent are allocated by the user agent
        // when it handles the `new_traversable_info` on NavigateRequest, and
        // the opener relationship is set by `setup_opener_for_window_open`
        // (see `UserAgent::creating_a_new_top_level_traversable`).
        create_a_new_browsing_context_and_document(
            parent_engine,
            creator_origin,
            RealmWiring {
                source_navigable_id: new_traversable_id,
                document_id: new_document_id,
                event_sender,
                task_sources: self.task_sources()?,
            },
        )
    }

    /// Set the shared new-document registry that both GlobalScope and
    /// ContentProcess access.  ContentProcess sets this before running JS
    /// that may trigger `the_rules_for_choosing_a_navigable`.
    pub(crate) fn set_new_document_registry(
        &self,
        registry: Rc<
            RefCell<HashMap<DocumentId, (EnvironmentSettingsObject, Rc<RefCell<BaseDocument>>)>>,
        >,
    ) {
        *self.new_document_registry.borrow_mut() = Some(registry);
    }

    /// Clear the shared registry after JS execution completes.
    pub(crate) fn clear_new_document_registry(&self) {
        *self.new_document_registry.borrow_mut() = None;
    }

    /// Register a newly-created traversable document in the shared registry.
    /// Returns an error if no registry has been set (caller error).
    pub(crate) fn register_new_traversable_document(
        &self,
        document_id: DocumentId,
        settings: EnvironmentSettingsObject,
        document: Rc<RefCell<BaseDocument>>,
    ) -> Result<(), String> {
        let registry = self
            .new_document_registry
            .borrow()
            .clone()
            .ok_or_else(|| String::from("no new_document_registry set on GlobalScope"))?;
        registry
            .borrow_mut()
            .insert(document_id, (settings, document));
        Ok(())
    }

    /// Set the shared video-paint registry that both GlobalScope and
    /// ContentProcess access.  ContentProcess sets this during document
    /// creation so that `resource_selection_algorithm` can register
    /// paint IDs during JS execution.
    pub(crate) fn set_graphics_sender(
        &self,
        sender: IpcSender<ipc_messages::graphics::GraphicsCommand>,
    ) {
        self.graphics_sender.borrow_mut().replace(sender);
    }

    pub(crate) fn graphics_sender(
        &self,
    ) -> Option<IpcSender<ipc_messages::graphics::GraphicsCommand>> {
        self.graphics_sender.borrow().clone()
    }

    pub(crate) fn allocate_media_pipeline_id(&self) -> ipc_messages::media::MediaPipelineId {
        ipc_messages::media::MediaPipelineId(uuid::Uuid::new_v4())
    }

    /// Store the engine context so new realms can share the same JS engine
    /// (same GC heap on JSC).  Called during engine setup, before any JS
    /// execution that might trigger `window.open`.
    /// Note: Only used on JSC backend (Boa creates fresh contexts).
    #[allow(dead_code)]
    pub(crate) fn set_video_paint_registry(
        &self,
        registry: Rc<RefCell<HashMap<(DocumentId, usize), VideoPaintId>>>,
    ) {
        *self.video_paint_registry.borrow_mut() = Some(registry);
    }

    /// Register a VideoPaintId for a (document_id, node_id) pair.
    /// Returns the existing paint ID if one is already registered, or
    /// inserts and returns the given one.
    pub(crate) fn register_video_paint_id(
        &self,
        document_id: DocumentId,
        node_id: usize,
        paint_id: VideoPaintId,
    ) {
        if let Some(registry) = self.video_paint_registry.borrow().as_ref() {
            registry
                .borrow_mut()
                .entry((document_id, node_id))
                .or_insert(paint_id);
        }
    }

    pub(crate) fn set_creation_url(&self, url: url::Url) {
        self.creation_url.borrow_mut().replace(url);
    }

    pub(crate) fn creation_url(&self) -> Option<url::Url> {
        self.creation_url.borrow().clone()
    }

    pub(crate) fn cancel_animation_frame(&self, handle: u32, ec: &mut dyn ExecutionContext<Types>) {
        self.animation_frame_callbacks
            .borrow_mut(ec)
            .retain(|entry| entry.handle != handle);
    }

    /// Whether any animation frame callbacks are queued (a pending skeleton
    /// or rAF that will be run at the next rendering opportunity). Used by
    /// `update_the_rendering` to decide whether a render must run even when
    /// the document is otherwise clean — a script-driven animation loop keeps
    /// re-registering callbacks right after they run.
    pub(crate) fn has_pending_animation_frame_callbacks(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> bool {
        !self.animation_frame_callbacks.borrow(ec).is_empty()
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn take_animation_frame_callbacks(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<Callback> {
        let callback_handles: Vec<u32> = self
            .animation_frame_callbacks
            .borrow(ec)
            .iter()
            .map(|entry| entry.handle)
            .collect();

        let mut callbacks = self.animation_frame_callbacks.borrow_mut(ec);
        let mut taken = Vec::with_capacity(callback_handles.len());
        for handle in callback_handles {
            let Some(index) = callbacks.iter().position(|entry| entry.handle == handle) else {
                continue;
            };
            taken.push(callbacks.remove(index).callback.clone());
        }
        taken
    }

    /// Delegation methods to WasmState.
    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn next_wasm_request_id(&self) -> u64 {
        self.wasm_state
            .as_ref()
            .map(|state| state.next_request_id())
            .unwrap_or(0)
    }

    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn push_pending_request(
        &self,
        request: crate::wasm::PendingRequest,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        if let Some(state) = &self.wasm_state {
            state.push_pending_request(request, ec);
        }
    }

    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn store_wasm_resolver(
        &self,
        request_id: u64,
        promise: JsObject,
        resolvers: js_engine::records::PromiseResolvers<Types>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        if let Some(state) = &self.wasm_state {
            state.store_wasm_resolver(request_id, promise, resolvers, ec);
        }
    }

    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn take_pending_wasm_batches(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<(u64, Vec<u8>)> {
        self.wasm_state
            .as_ref()
            .map(|state| state.take_pending_wasm_batches(ec))
            .unwrap_or_default()
    }

    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn take_pending_wasm_instantiates(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<(u64, wasmtime::Module)> {
        self.wasm_state
            .as_ref()
            .map(|state| state.take_pending_wasm_instantiates(ec))
            .unwrap_or_default()
    }

    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn consume_wasm_request(
        &self,
        request_id: u64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<(JsObject, js_engine::records::PromiseResolvers<Types>)> {
        self.wasm_state
            .as_ref()
            .and_then(|state| state.consume_wasm_request(request_id, ec))
    }
}
