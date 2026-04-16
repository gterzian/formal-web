use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

use blitz_dom::BaseDocument;
use boa_engine::{JsValue, object::JsObject};
use boa_gc::{Finalize, GcRefCell, Trace};
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{Event as ContentEvent, WindowTimerClearRequest, WindowTimerRequest};

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Debug, Clone, Copy)]
pub enum GlobalScopeKind {
    Window,
}

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Trace, Finalize)]
pub struct CachedNodeObject {
    /// <https://dom.spec.whatwg.org/#interface-node>
    #[unsafe_ignore_trace]
    pub node_id: usize,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub object: JsObject,
}

/// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
#[derive(Trace, Finalize)]
pub struct AnimationFrameCallback {
    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[unsafe_ignore_trace]
    pub handle: u32,

    /// <https://webidl.spec.whatwg.org/#idl-callback-function>
    pub callback: JsObject,
}

/// <https://html.spec.whatwg.org/#timers>
#[derive(Trace, Finalize, Clone)]
pub enum TimerHandler {
    Function {
        /// <https://webidl.spec.whatwg.org/#idl-callback-function>
        callback: JsObject,
    },
    String {
        /// <https://html.spec.whatwg.org/#timerhandler>
        #[unsafe_ignore_trace]
        source: String,
    },
}

/// <https://html.spec.whatwg.org/#timers>
#[derive(Trace, Finalize, Clone)]
pub struct WindowTimer {
    /// <https://html.spec.whatwg.org/#map-of-settimeout-and-setinterval-ids>
    #[unsafe_ignore_trace]
    pub id: u32,

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    #[unsafe_ignore_trace]
    pub timer_key: u64,

    /// <https://html.spec.whatwg.org/#timerhandler>
    pub handler: TimerHandler,

    /// <https://html.spec.whatwg.org/#timers>
    pub arguments: Vec<JsValue>,

    /// <https://html.spec.whatwg.org/#timers>
    #[unsafe_ignore_trace]
    pub repeat: bool,

    /// <https://html.spec.whatwg.org/#timers>
    #[unsafe_ignore_trace]
    pub timeout_ms: u32,
}

#[derive(Clone)]
struct TimerHost {
    document_id: u64,
    event_sender: IpcSender<ContentEvent>,
}

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Trace, Finalize)]
pub struct GlobalScope {
    /// <https://html.spec.whatwg.org/#global-object>
    #[unsafe_ignore_trace]
    pub kind: GlobalScopeKind,

    /// <https://html.spec.whatwg.org/#concept-document-window>
    #[unsafe_ignore_trace]
    document: Rc<RefCell<BaseDocument>>,

    /// <https://dom.spec.whatwg.org/#interface-document>
    document_object: GcRefCell<Option<JsObject>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    node_objects: GcRefCell<Vec<CachedNodeObject>>,

    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[unsafe_ignore_trace]
    animation_frame_callback_identifier: Cell<u32>,

    /// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
    animation_frame_callbacks: GcRefCell<Vec<AnimationFrameCallback>>,

    /// <https://html.spec.whatwg.org/#timers>
    #[unsafe_ignore_trace]
    timer_callback_identifier: Cell<u32>,

    /// <https://html.spec.whatwg.org/#timers>
    #[unsafe_ignore_trace]
    timer_key_identifier: Cell<u64>,

    /// <https://html.spec.whatwg.org/#map-of-settimeout-and-setinterval-ids>
    window_timers: GcRefCell<Vec<WindowTimer>>,

    /// <https://html.spec.whatwg.org/#timer-nesting-level>
    #[unsafe_ignore_trace]
    current_timer_nesting_level: Cell<Option<u32>>,

    #[unsafe_ignore_trace]
    timer_host: RefCell<Option<TimerHost>>,
}

impl GlobalScope {
    pub fn new(kind: GlobalScopeKind, document: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            kind,
            document,
            document_object: GcRefCell::new(None),
            node_objects: GcRefCell::new(Vec::new()),
            animation_frame_callback_identifier: Cell::new(0),
            animation_frame_callbacks: GcRefCell::new(Vec::new()),
            timer_callback_identifier: Cell::new(0),
            timer_key_identifier: Cell::new(0),
            window_timers: GcRefCell::new(Vec::new()),
            current_timer_nesting_level: Cell::new(None),
            timer_host: RefCell::new(None),
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

    fn next_timer_key(&self) -> u64 {
        let mut timer_key = self.timer_key_identifier.get();

        loop {
            timer_key = timer_key.wrapping_add(1);
            if timer_key != 0 {
                break;
            }
        }

        self.timer_key_identifier.set(timer_key);
        timer_key
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

    pub(crate) fn install_timer_host(&self, document_id: u64, event_sender: IpcSender<ContentEvent>) {
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
    pub(crate) fn request_animation_frame(&self, callback: JsObject) -> u32 {
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
    pub(crate) fn set_current_timer_nesting_level(&self, nesting_level: Option<u32>) -> Option<u32> {
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
        // Note: This helper continues the `timer initialization steps` algorithm at the `GlobalScope`-owned pieces. The mixin carrier already handled the preliminary timeout conversion, clamping, and task setup.

        // Step 2: "If previousId was given, let id be previousId; otherwise, let id be an implementation-defined integer that is greater than zero and does not already exist in global's map of setTimeout and setInterval IDs."
        let timer_id = previous_id.unwrap_or_else(|| self.next_timer_id());

        // Step 11: "Set uniqueHandle to the result of running steps after a timeout given global, \"setTimeout/setInterval\", timeout, and completionStep."
        // Note: The content/embedder boundary forwards this request into the Lean timer worker, which models `run steps after a timeout`.
        let timer_key = self.next_timer_key();
        let host = self.timer_host()?;
        host.event_sender
            .send(ContentEvent::WindowTimerRequested(WindowTimerRequest {
                document_id: host.document_id,
                timer_id,
                timer_key,
                timeout_ms,
                nesting_level,
            }))
            .map_err(|error| format!("failed to send window timer request to the embedder: {error}"))?;

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
        let Ok(host) = self.timer_host() else {
            return;
        };

        // Note: The embedder-facing clear mirrors the map removal into the Lean timer worker's active-timer state.
        if let Err(error) = host
            .event_sender
            .send(ContentEvent::WindowTimerCleared(WindowTimerClearRequest {
                document_id: host.document_id,
                timer_key: removed_timer.timer_key,
            }))
        {
            eprintln!("failed to send window timer clear to the embedder: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn window_timer(&self, timer_id: u32, timer_key: u64) -> Option<WindowTimer> {
        // Note: This model-local lookup exposes the stored `(id, uniqueHandle)` registration so the queued timer task can check whether the timer still exists and still maps to the same handle before running the handler.
        self.window_timers
            .borrow()
            .iter()
            .find(|entry| entry.id == timer_id && entry.timer_key == timer_key)
            .cloned()
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    pub(crate) fn complete_window_timer(&self, timer_id: u32, timer_key: u64) -> Result<(), String> {
        // Note: This helper continues the queued timer task after the handler and the stale-handle checks have already run inside `EnvironmentSettingsObject::run_window_timer`.
        let timer = self.window_timer(timer_id, timer_key);
        let Some(timer) = timer else {
            return Ok(());
        };

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
        let next_timer_key = self.next_timer_key();
        host.event_sender
            .send(ContentEvent::WindowTimerRequested(WindowTimerRequest {
                document_id: host.document_id,
                timer_id,
                timer_key: next_timer_key,
                timeout_ms: timer.timeout_ms,
                nesting_level: next_nesting_level,
            }))
            .map_err(|error| format!("failed to reschedule window timer with the embedder: {error}"))?;

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
            if let Err(error) = host
                .event_sender
                .send(ContentEvent::WindowTimerCleared(WindowTimerClearRequest {
                    document_id: host.document_id,
                    timer_key: timer.timer_key,
                }))
            {
                eprintln!("failed to clear window timer during teardown: {error}");
                break;
            }
        }
    }

    /// <https://html.spec.whatwg.org/#animationframeprovider-cancelanimationframe>
    pub(crate) fn cancel_animation_frame(&self, handle: u32) {
        self.animation_frame_callbacks
            .borrow_mut()
            .retain(|entry| entry.handle != handle);
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn take_animation_frame_callbacks(&self) -> Vec<JsObject> {
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
}