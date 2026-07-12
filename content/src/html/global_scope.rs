use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    vec::Vec,
};

use super::environment_settings_object::EnvironmentSettingsObject;

use blitz_dom::BaseDocument;
use ipc::IpcSender;
use ipc_messages::content::DocumentId;
use ipc_messages::content::{
    Event as ContentEvent, NavigableId, WindowTimerClearRequest, WindowTimerKey, WindowTimerRequest,
};
use ipc_messages::media::{MediaCommand, VideoPaintId};
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{JsTypes, gc_struct};
use log::{debug, error};

use crate::html::Microtask;
use crate::js::Types;
use crate::webidl::Callback;
use js_engine::Completion;
use js_engine::ExecutionContext;

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

/// The lifecycle state of a pending request.
#[gc_struct]
#[derive(Debug, PartialEq, Eq)]
pub enum PendingState {
    /// Just created, waiting for the content process to submit it.
    Pending,
    /// Sent to the background thread, waiting for completion.
    Processing,
}

/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
///
/// A pending WebAssembly request stored on the GlobalScope during JS execution.
/// The request stays in this Vec throughout its lifecycle — the state field
/// tracks progress.  The content process mutates the state when submitting to
/// the background thread and when resolving/rejecting the promise.
///
/// Note: JS-typed fields (promise, resolvers) are NOT stored here — they live
/// in `GlobalScope.pending_wasm_resolvers` keyed by `request_id`.  This lets
/// domain code in `content/src/wasm/` construct and push `PendingRequest`
/// without importing `boa_engine`.
#[gc_struct]
pub enum PendingRequest {
    /// A WebAssembly module compilation or instantiate-byte request.
    WasmCompile {
        /// Stable copy of the buffer bytes.
        #[ignore_trace]
        bytes: Vec<u8>,
        /// The request id, correlating with the background thread's result.
        #[ignore_trace]
        request_id: u64,
        /// True if this came from `instantiate(bytes, ...)`, false for `compile(bytes, ...)`.
        is_instantiate: bool,
        /// Current lifecycle state.
        ///
        /// PendingState is a simple Copy enum, safe to ignore trace on.
        #[ignore_trace]
        state: PendingState,
    },

    /// <https://www.w3.org/TR/wasm-js-api/#asynchronously-instantiate-a-webassembly-module>
    ///
    /// An instantiate(moduleObject, importObject) request.
    /// The module is already compiled — this only needs instantiation.
    #[cfg(boa_backend)]
    WasmInstantiate {
        /// The previously compiled wasm module.
        #[ignore_trace]
        module: wasmtime::Module,
        /// The request id, correlating with the content-process's processing.
        #[ignore_trace]
        request_id: u64,
        /// Current lifecycle state.
        #[ignore_trace]
        state: PendingState,
    },
}

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Debug, Clone, Copy)]
pub enum GlobalScopeKind {
    Window,
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

#[derive(Clone)]
struct TimerHost {
    document_id: DocumentId,
    event_sender: IpcSender<ContentEvent>,
}

/// <https://html.spec.whatwg.org/#global-object>
#[gc_struct]
pub struct GlobalScope {
    /// <https://html.spec.whatwg.org/#global-object>
    #[ignore_trace]
    pub kind: GlobalScopeKind,

    /// <https://html.spec.whatwg.org/#concept-document-window>
    #[ignore_trace]
    document: Rc<RefCell<BaseDocument>>,

    /// <https://dom.spec.whatwg.org/#interface-document>
    document_object: GcCell<Option<JsObject>>,

    /// <https://html.spec.whatwg.org/#dom-location>
    location_object: GcCell<Option<JsObject>>,

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

    #[ignore_trace]
    timer_host: RefCell<Option<TimerHost>>,

    /// <https://html.spec.whatwg.org/#concept-navigable>
    #[ignore_trace]
    source_navigable_id: Cell<Option<NavigableId>>,

    /// <https://html.spec.whatwg.org/#parent-navigable>
    /// The parent of this document's navigable in the navigable tree.
    /// None indicates a top-level traversable.
    #[ignore_trace]
    parent_traversable_id: Cell<Option<NavigableId>>,

    /// <https://html.spec.whatwg.org/#traversable-navigable>
    /// The top-level traversable for this navigable tree.
    #[ignore_trace]
    top_level_traversable_id: Cell<Option<NavigableId>>,

    /// <https://html.spec.whatwg.org/#concept-document>
    /// The document id for the document associated with this global scope.
    #[ignore_trace]
    document_id: RefCell<Option<DocumentId>>,

    /// Sender for content-to-user-agent IPC events (e.g. navigation requests).
    #[ignore_trace]
    event_sender: RefCell<Option<IpcSender<ContentEvent>>>,

    /// Shared registry for newly-created traversable documents (window.open).
    /// Set by `ContentProcess` before running JS that may trigger
    /// `the_rules_for_choosing_a_navigable`. Both GlobalScope (to insert)
    /// and ContentProcess (to retrieve) share the same `Rc`, so no separate
    /// flush step is needed.
    #[ignore_trace]
    new_document_registry: RefCell<
        Option<
            Rc<
                RefCell<
                    HashMap<DocumentId, (EnvironmentSettingsObject, Rc<RefCell<BaseDocument>>)>,
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
    video_paint_registry: RefCell<Option<Rc<RefCell<HashMap<(DocumentId, usize), VideoPaintId>>>>>,

    /// Direct sender to the media extension.
    #[ignore_trace]
    media_extension_sender: RefCell<Option<IpcSender<MediaCommand>>>,

    /// JSC engine context (cloned `JscContext`) for creating shared realms.
    /// On Boa this is `None` (creates a fresh context for each realm).
    /// Stored as `Rc<dyn Any>` to keep the backend-agnostic abstraction.
    /// Uses `Rc` instead of `Box` because the JSC `#[gc_struct]` derive
    /// adds `#[derive(Clone)]`.
    ///
    /// Note: This field is only read on JSC (`#[cfg(not(boa_backend))]`), so
    /// `#[allow(dead_code)]` suppresses the Boa-backend warning.
    #[allow(dead_code)]
    #[ignore_trace]
    engine_context: Option<Rc<dyn core::any::Any + Send>>,

    /// <https://html.spec.whatwg.org/#concept-document-creation-url>
    /// The creation URL of this window's Document.
    #[ignore_trace]
    creation_url: RefCell<Option<url::Url>>,

    /// Generic queue of pending async requests created during JS execution.
    /// Populated by native JS functions (e.g. WebAssembly.compile) and drained
    /// by the content process after JS execution completes.
    pending_requests: GcCell<Vec<PendingRequest>>,

    /// A counter for generating unique request IDs for async wasm operations.
    #[ignore_trace]
    pending_wasm_request_id_counter: std::cell::Cell<u64>,

    /// Map of request_id → (promise, resolvers) for pending wasm operations.
    /// The promise and resolvers are stored here rather than in
    /// `PendingRequest` so that domain code in `content/src/wasm/` can
    /// push pending requests without importing `boa_engine`.
    pending_wasm_resolvers:
        GcCell<Vec<(u64, JsObject, js_engine::records::PromiseResolvers<Types>)>>,

    /// <https://html.spec.whatwg.org/#microtask-queue>
    /// Shared Rc with ContentProcess, which drains directly.
    /// The outer RefCell allows replacing the Rc without &mut self.
    #[ignore_trace]
    microtask_queue: RefCell<Rc<RefCell<Vec<Microtask>>>>,
}

impl GlobalScope {
    pub fn new(kind: GlobalScopeKind, document: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            kind,
            document,
            document_object: gc_cell_new(None),
            location_object: gc_cell_new(None),
            node_objects: gc_cell_new(Vec::new()),
            animation_frame_callback_identifier: Cell::new(0),
            animation_frame_callbacks: gc_cell_new(Vec::new()),
            timer_callback_identifier: Cell::new(0),
            window_timers: gc_cell_new(Vec::new()),
            current_timer_nesting_level: Cell::new(None),
            timer_host: RefCell::new(None),
            source_navigable_id: Cell::new(None),
            parent_traversable_id: Cell::new(None),
            top_level_traversable_id: Cell::new(None),
            document_id: RefCell::new(None),
            event_sender: RefCell::new(None),

            new_document_registry: RefCell::new(None),
            video_paint_registry: RefCell::new(None),
            media_extension_sender: RefCell::new(None),
            engine_context: None,
            creation_url: RefCell::new(None),
            pending_requests: gc_cell_new(Vec::new()),
            pending_wasm_request_id_counter: std::cell::Cell::new(0),
            pending_wasm_resolvers: gc_cell_new(Vec::new()),
            microtask_queue: RefCell::new(Rc::new(RefCell::new(Vec::new()))),
        }
    }

    fn next_timer_id(&self) -> u32 {
        let timers = self.window_timers.borrow();
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

    fn timer_host(&self) -> Result<TimerHost, String> {
        self.timer_host
            .borrow()
            .clone()
            .ok_or_else(|| String::from("window timer host is not installed"))
    }

    pub(crate) fn document(&self) -> Rc<RefCell<BaseDocument>> {
        Rc::clone(&self.document)
    }

    pub(crate) fn set_navigation_info(
        &self,
        source_navigable_id: NavigableId,
        event_sender: IpcSender<ContentEvent>,
    ) {
        self.source_navigable_id.set(Some(source_navigable_id));
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

    pub(crate) fn document_id(&self) -> Option<DocumentId> {
        *self.document_id.borrow()
    }

    pub(crate) fn event_sender(&self) -> Option<IpcSender<ContentEvent>> {
        self.event_sender.borrow().clone()
    }

    pub(crate) fn set_timer_host(
        &self,
        document_id: DocumentId,
        event_sender: IpcSender<ContentEvent>,
    ) {
        self.document_id.borrow_mut().replace(document_id);
        self.timer_host.borrow_mut().replace(TimerHost {
            document_id,
            event_sender,
        });
    }

    pub(crate) fn document_object(&self) -> Option<JsObject> {
        self.document_object.borrow().clone()
    }

    pub(crate) fn store_document_object(&self, object: JsObject) {
        self.document_object.borrow_mut().replace(object);
    }

    pub(crate) fn location_object(&self) -> Option<JsObject> {
        self.location_object.borrow().clone()
    }

    pub(crate) fn store_location_object(&self, object: JsObject) {
        self.location_object.borrow_mut().replace(object);
    }

    pub(crate) fn cached_node_object(&self, node_id: usize) -> Option<JsObject> {
        self.node_objects
            .borrow()
            .iter()
            .find(|entry| entry.node_id == node_id)
            .map(|entry| entry.object.clone())
    }

    pub(crate) fn cache_node_object(&self, node_id: usize, object: JsObject) {
        self.node_objects
            .borrow_mut()
            .push(CachedNodeObject { node_id, object });
    }

    pub(crate) fn invalidate_cached_node_ids(&self, node_ids: &[usize]) {
        if node_ids.is_empty() {
            return;
        }

        let node_ids = node_ids.iter().copied().collect::<HashSet<_>>();
        self.node_objects
            .borrow_mut()
            .retain(|entry| !node_ids.contains(&entry.node_id));
    }

    /// <https://html.spec.whatwg.org/#dom-animationframeprovider-requestanimationframe>
    pub(crate) fn request_animation_frame(&self, callback: Callback) -> u32 {
        let callbacks = self.animation_frame_callbacks.borrow();
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
            .borrow_mut()
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
    ) -> Result<u32, String> {
        // Note: This helper continues the `timer initialization steps` algorithm at the `GlobalScope`-owned pieces. The mixin implementation already handled the preliminary timeout conversion, clamping, and task setup.

        // Step 2: "If previousId was given, let id be previousId; otherwise, let id be an implementation-defined integer that is greater than zero and does not already exist in global's map of setTimeout and setInterval IDs."
        let timer_id = previous_id.unwrap_or_else(|| self.next_timer_id());

        // Step 11: "Set uniqueHandle to the result of running steps after a timeout given global, \"setTimeout/setInterval\", timeout, and completionStep."
        // Note: The content/embedder boundary forwards this request into the dedicated timer worker, which models `run steps after a timeout`.
        let timer_key = self.next_timer_key()?;
        log_timer_debug(format!(
            "schedule timer id={} key={} timeout_ms={} nesting={} repeat={} previous_id={:?}",
            timer_id, timer_key, timeout_ms, nesting_level, repeat, previous_id
        ));
        let host = self.timer_host()?;
        host.event_sender
            .send(ContentEvent::WindowTimerRequested(WindowTimerRequest {
                document_id: host.document_id,
                timer_id,
                timer_key,
                timeout_ms,
                nesting_level,
            }))
            .map_err(|error| {
                format!("failed to send window timer request to the embedder: {error}")
            })?;

        // Step 12: "Set global's map of setTimeout and setInterval IDs[id] to uniqueHandle."
        let mut timers = self.window_timers.borrow_mut();
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
    pub(crate) fn clear_timer(&self, timer_id: u32) {
        // Note: This is the shared storage helper used by both `clearTimeout()` and `clearInterval()`.

        // Step 1: "Remove this's map of setTimeout and setInterval IDs[id]."
        let removed_timer = {
            let mut timers = self.window_timers.borrow_mut();
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
        let Ok(host) = self.timer_host() else {
            return;
        };

        // Note: The embedder-facing clear mirrors the map removal into the timer worker's active-timer state.
        if let Err(error) =
            host.event_sender
                .send(ContentEvent::WindowTimerCleared(WindowTimerClearRequest {
                    document_id: host.document_id,
                    timer_key: removed_timer.timer_key,
                }))
        {
            error!("failed to send window timer clear to the embedder: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn window_timer(
        &self,
        timer_id: u32,
        timer_key: WindowTimerKey,
    ) -> Option<WindowTimer> {
        // Note: This model-local lookup exposes the stored `(id, uniqueHandle)` registration so the queued timer task can check whether the timer still exists and still maps to the same handle before running the handler.
        self.window_timers
            .borrow()
            .iter()
            .find(|entry| entry.id == timer_id && entry.timer_key == timer_key)
            .cloned()
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn complete_window_timer(
        &self,
        timer_id: u32,
        timer_key: WindowTimerKey,
    ) -> Result<(), String> {
        // Note: This helper continues the queued timer task after the handler and the stale-handle checks have already run inside `EnvironmentSettingsObject::run_window_timer`.
        let timer = self.window_timer(timer_id, timer_key);
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
                .borrow_mut()
                .retain(|entry| !(entry.id == timer_id && entry.timer_key == timer_key));
            return Ok(());
        }

        // Step 11: "If repeat is true, then perform the timer initialization steps again, given global, handler, timeout, arguments, true, and id."
        let next_nesting_level = self
            .current_timer_nesting_level()
            .unwrap_or(0)
            .saturating_add(1);
        let host = self.timer_host()?;
        let next_timer_key = self.next_timer_key()?;
        log_timer_debug(format!(
            "reschedule interval id={} old_key={} new_key={} timeout_ms={} nesting={}",
            timer_id, timer_key, next_timer_key, timer.timeout_ms, next_nesting_level
        ));
        host.event_sender
            .send(ContentEvent::WindowTimerRequested(WindowTimerRequest {
                document_id: host.document_id,
                timer_id,
                timer_key: next_timer_key,
                timeout_ms: timer.timeout_ms,
                nesting_level: next_nesting_level,
            }))
            .map_err(|error| {
                format!("failed to reschedule window timer with the embedder: {error}")
            })?;

        let mut timers = self.window_timers.borrow_mut();
        let Some(entry) = timers
            .iter_mut()
            .find(|entry| entry.id == timer_id && entry.timer_key == timer_key)
        else {
            return Ok(());
        };
        entry.timer_key = next_timer_key;
        Ok(())
    }

    pub(crate) fn clear_all_timers(&self) {
        let cleared_timers = {
            let mut timers = self.window_timers.borrow_mut();
            std::mem::take(&mut *timers)
        };
        let Ok(host) = self.timer_host() else {
            return;
        };
        for timer in cleared_timers {
            if let Err(error) =
                host.event_sender
                    .send(ContentEvent::WindowTimerCleared(WindowTimerClearRequest {
                        document_id: host.document_id,
                        timer_key: timer.timer_key,
                    }))
            {
                error!("failed to clear window timer during teardown: {error}");
                break;
            }
        }
    }

    /// Like `create_document`, but passes the stored engine context so the
    /// new realm shares the same JS context / GC heap on JSC.
    /// Used for `window.open` with opener (auxiliary BC).
    pub(crate) fn create_document_in_realm(
        &self,
        new_traversable_id: NavigableId,
        new_document_id: DocumentId,
    ) -> Result<
        (
            JsObject,
            super::environment_settings_object::EnvironmentSettingsObject,
            Rc<RefCell<BaseDocument>>,
        ),
        String,
    > {
        let event_sender = self.event_sender();
        let event_sender = event_sender
            .as_ref()
            .ok_or_else(|| String::from("GlobalScope has no event sender"))?;
        // Build a temporary parent engine from the stored engine context.
        // `create_a_new_realm` will use this to create a shared realm,
        // ensuring the new window's objects live in the same GC heap on JSC.
        let mut temp_engine = self.build_temp_parent_engine()?;
        let parent: Option<&mut crate::js::Engine> = temp_engine.as_mut();
        crate::html::create_a_new_realm(parent, event_sender, new_traversable_id, new_document_id)
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
    pub(crate) fn set_media_extension_sender(&self, sender: IpcSender<MediaCommand>) {
        self.media_extension_sender.borrow_mut().replace(sender);
    }

    pub(crate) fn media_extension_sender(&self) -> Option<IpcSender<MediaCommand>> {
        self.media_extension_sender.borrow().clone()
    }

    pub(crate) fn allocate_media_pipeline_id(&self) -> ipc_messages::media::MediaPipelineId {
        ipc_messages::media::MediaPipelineId(uuid::Uuid::new_v4())
    }

    /// Store the engine context so new realms can share the same JS engine
    /// (same GC heap on JSC).  Called during engine setup, before any JS
    /// execution that might trigger `window.open`.
    /// Note: Only used on JSC backend (Boa creates fresh contexts).
    #[allow(dead_code)]
    pub(crate) fn set_engine_context(&mut self, context: Box<dyn core::any::Any + Send>) {
        self.engine_context = Some(Rc::from(context));
    }

    /// Build a temporary parent engine from the stored engine context.
    /// On JSC, creates an engine sharing the same JSC context (same GC heap).
    /// On Boa, returns `None` (the caller's `build_realm` creates a fresh context).
    fn build_temp_parent_engine(&self) -> Result<Option<crate::js::Engine>, String> {
        #[cfg(not(boa_backend))]
        {
            use js_engine::jsc::{JscContext, JscEngine};
            let ctx = self
                .engine_context
                .as_ref()
                .and_then(|c| c.downcast_ref::<JscContext>())
                .ok_or_else(|| "no engine context available for shared realm".to_string())?;
            Ok(Some(JscEngine::new_from_context(ctx.clone())))
        }
        #[cfg(boa_backend)]
        {
            // Boa: `build_realm` ignores the parent engine, so returning None
            // creates a fresh context (which is the expected behavior).
            Ok(None)
        }
    }

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

    pub(crate) fn cancel_animation_frame(&self, handle: u32) {
        self.animation_frame_callbacks
            .borrow_mut()
            .retain(|entry| entry.handle != handle);
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn take_animation_frame_callbacks(&self) -> Vec<Callback> {
        let callback_handles: Vec<u32> = self
            .animation_frame_callbacks
            .borrow()
            .iter()
            .map(|entry| entry.handle)
            .collect();

        let mut callbacks = self.animation_frame_callbacks.borrow_mut();
        let mut taken = Vec::with_capacity(callback_handles.len());
        for handle in callback_handles {
            let Some(index) = callbacks.iter().position(|entry| entry.handle == handle) else {
                continue;
            };
            taken.push(callbacks.remove(index).callback.clone());
        }
        taken
    }

    /// Push a pending async request onto this document's queue.
    ///
    /// Called by native JS functions (e.g. `WebAssembly.compile()`) during JS
    /// execution.  The content process drains these requests after each command.
    pub(crate) fn push_pending_request(&self, request: PendingRequest) {
        self.pending_requests.borrow_mut().push(request);
    }

    /// Allocate a unique request ID for a pending wasm operation.
    pub(crate) fn next_wasm_request_id(&self) -> u64 {
        let id = self.pending_wasm_request_id_counter.get();
        self.pending_wasm_request_id_counter.set(id.wrapping_add(1));
        id
    }

    /// Mark all compile-type pending wasm requests as Processing and return
    /// their bytes + request_ids.  Called by the content process.
    pub(crate) fn take_pending_wasm_batches(&self) -> Vec<(u64, Vec<u8>)> {
        let mut requests = self.pending_requests.borrow_mut();
        let mut batches = Vec::new();
        for request in requests.iter_mut() {
            if let PendingRequest::WasmCompile {
                bytes,
                request_id,
                state,
                ..
            } = request
            {
                if *state == PendingState::Pending {
                    *state = PendingState::Processing;
                    batches.push((*request_id, bytes.clone()));
                }
            }
        }
        batches
    }

    /// Mark all instantiate-type pending wasm requests as Processing and
    /// return their module + request_id.  Called by the content process.
    #[cfg(boa_backend)]
    pub(crate) fn take_pending_wasm_instantiates(&self) -> Vec<(u64, wasmtime::Module)> {
        let mut requests = self.pending_requests.borrow_mut();
        let mut instantiates = Vec::new();
        for request in requests.iter_mut() {
            if let PendingRequest::WasmInstantiate {
                module,
                request_id,
                state,
            } = request
            {
                if *state == PendingState::Pending {
                    *state = PendingState::Processing;
                    instantiates.push((*request_id, module.clone()));
                }
            }
        }
        instantiates
    }

    /// Store the promise and resolving functions for a pending wasm request.
    /// Called by the bindings layer after creating the promise.
    pub(crate) fn store_wasm_resolver(
        &self,
        request_id: u64,
        promise: JsObject,
        resolvers: js_engine::records::PromiseResolvers<Types>,
    ) {
        self.pending_wasm_resolvers
            .borrow_mut()
            .push((request_id, promise, resolvers));
    }

    /// <https://html.spec.whatwg.org/#queue-a-microtask>
    /// <https://html.spec.whatwg.org/#queue-a-microtask>
    /// Pushes to the shared domain queue, which is drained by
    /// ContentProcess::perform_microtask_checkpoint.
    pub(crate) fn queue_microtask(&self, microtask: Microtask) {
        let rc = self.microtask_queue.borrow().clone();
        rc.borrow_mut().push(microtask);
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    #[allow(dead_code)]
    pub(crate) fn drain_microtasks(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let rc = self.microtask_queue.borrow().clone();
        let tasks = std::mem::take(&mut *rc.borrow_mut());
        for task in &tasks {
            task.call(ec)?;
        }
        Ok(())
    }

    /// Share the ContentProcess's microtask queue.  After this call,
    /// `queue_microtask` pushes directly to the shared queue.
    pub(crate) fn set_shared_microtask_queue(&self, shared: Rc<RefCell<Vec<Microtask>>>) {
        // Transfer any tasks queued before the shared queue was set.
        let old_rc = self.microtask_queue.borrow();
        let pending = std::mem::take(&mut *old_rc.clone().borrow_mut());
        drop(old_rc);
        if !pending.is_empty() {
            shared.borrow_mut().extend(pending);
        }
        *self.microtask_queue.borrow_mut() = shared;
    }

    pub(crate) fn microtask_queue_ref(&self) -> Rc<RefCell<Vec<Microtask>>> {
        self.microtask_queue.borrow().clone()
    }

    /// Check if the microtask queue is empty, for debugging/assertions.
    #[allow(dead_code)]
    pub(crate) fn microtask_queue_is_empty(&self) -> bool {
        self.microtask_queue.borrow().borrow().is_empty()
    }

    /// Remove and return the promise + resolvers for a completed request.
    pub(crate) fn consume_wasm_request(
        &self,
        request_id: u64,
    ) -> Option<(JsObject, js_engine::records::PromiseResolvers<Types>)> {
        // Remove the PendingRequest from the request queue.
        {
            let mut requests = self.pending_requests.borrow_mut();
            let idx = requests.iter().position(|r| match r {
                PendingRequest::WasmCompile {
                    request_id: rid, ..
                } => *rid == request_id,
                #[cfg(boa_backend)]
                PendingRequest::WasmInstantiate {
                    request_id: rid, ..
                } => *rid == request_id,
            });
            if let Some(idx) = idx {
                requests.swap_remove(idx);
            }
        }
        // Look up the promise/resolvers in the separate store.
        let mut resolvers = self.pending_wasm_resolvers.borrow_mut();
        let idx = resolvers
            .iter()
            .position(|(rid, _, _)| *rid == request_id)?;
        let (_rid, promise, res) = resolvers.swap_remove(idx);
        Some((promise, res))
    }
}
