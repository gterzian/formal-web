use std::{cell::RefCell, rc::Rc, time::Instant};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, JsError, JsResult, JsValue, Source,
    class::Class,
    object::JsObject,
};
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId, WindowTimerKey};
use url::Url;

use crate::boa::bindings::html::{build_boa_context, install_global_properties};
use crate::boa::platform_objects::{
    document_object, object_for_existing_node, resolve_element_object, take_animation_frame_callbacks,
};
use crate::dom::{
    Document, EventDispatchHost,
};
use crate::html::{
    GlobalScope, TimerHandler, Window,
};
use crate::webidl::{
    Callback, ContextCallbackHost, EcmascriptHost, ExceptionBehavior,
    invoke_callback_function,
};

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        eprintln!("[timer-debug][settings] {}", message.as_ref());
    }
}

/// <https://html.spec.whatwg.org/#concept-settings-object-origin>
#[derive(Debug, Clone)]
pub struct Origin {
    /// <https://html.spec.whatwg.org/#ascii-serialisation-of-an-origin>
    pub serialized: String,
}

/// <https://html.spec.whatwg.org/#concept-referrer-policy>
#[derive(Debug, Clone, Copy, Default)]
pub enum ReferrerPolicy {
    #[default]
    NoReferrerWhenDowngrade,
}

/// <https://html.spec.whatwg.org/#environment-settings-object>
pub struct EnvironmentSettingsObject {
    /// <https://html.spec.whatwg.org/#realms-settings-objects-global-objects>
    pub context: Context,

    /// <https://dom.spec.whatwg.org/#concept-document>
    pub document: Rc<RefCell<BaseDocument>>,

    /// <https://html.spec.whatwg.org/#concept-settings-object-origin>
    pub origin: Origin,

    /// <https://html.spec.whatwg.org/#concept-environment-creation-url>
    pub creation_url: Url,

    /// <https://html.spec.whatwg.org/#concept-settings-object-policy-container>
    pub referrer_policy: ReferrerPolicy,

    /// <https://html.spec.whatwg.org/#concept-settings-object-time-origin>
    pub time_origin: Instant,

    /// Content-to-user-agent IPC sender for navigation and other content events.
    pub event_sender: Option<IpcSender<ContentEvent>>,

    /// <https://html.spec.whatwg.org/#concept-navigable>
    pub source_navigable_id: Option<NavigableId>,

    /// <https://html.spec.whatwg.org/#dom-document>
    pub document_id: Option<DocumentId>,

    /// Raw pointer to the `GlobalScope` owned by the `Window` inside the boa Context.
    /// The Window lives inside the Context's realm as the global object, and the Context
    /// is owned by this struct, so the pointer is valid for the entire lifecycle.
    /// This exists so HTML algorithms can access window-global state without traversing
    /// boa's GC heap. The GlobalScope IS traced through the Window — the raw pointer
    /// is purely an access optimization for the native side.
    global_scope_ptr: *const GlobalScope,
}

impl EnvironmentSettingsObject {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        creation_url: Url,
        event_sender: Option<IpcSender<ContentEvent>>,
        source_navigable_id: Option<NavigableId>,
        document_id: Option<DocumentId>,
    ) -> Result<Self, String> {
        // Build the boa Context. WindowHostHooks creates the Window and its
        // GlobalScope during build().
        let mut context = build_boa_context(Rc::clone(&document))?;

        // Capture a raw pointer to the GlobalScope inside the Window for direct
        // access by HTML algorithms without traversing boa's GC heap.
        // Safety: The Window lives inside the Context's realm as the global object,
        // and the Context is owned by this struct, so the pointer is valid for the
        // entire lifecycle of EnvironmentSettingsObject.
        // Keep the global object JsObject alive so the downcast_ref borrow is valid.
        let global_object = context.global_object();
        let window = global_object
            .downcast_ref::<Window>()
            .ok_or_else(|| String::from("global object is not a Window"))?;
        let global_scope_ptr: *const GlobalScope = &window.global_scope as *const GlobalScope;

        // Install timer host and navigation info on the GlobalScope now that we
        // have direct access to it.
        if let (Some(event_sender), Some(document_id)) = (&event_sender, document_id) {
            // Safety: global_scope_ptr is valid because the Context (which owns
            // the Window that owns the GlobalScope) is alive.
            unsafe {
                (*global_scope_ptr).set_timer_host(document_id, event_sender.clone());
            }
        }
        if let Some(navigable_id) = source_navigable_id {
            if let Some(event_sender) = &event_sender {
                unsafe {
                    (*global_scope_ptr).set_navigation_info(navigable_id, event_sender.clone());
                }
            }
        }

        let document_object =
            Class::from_data(Document::new(document.clone(), creation_url.clone()), &mut context)
                .map_err(|error| error.to_string())?;

        install_global_properties(&mut context, document_object)?;

        Ok(Self {
            context,
            document,
            origin: Origin {
                serialized: creation_url.origin().unicode_serialization(),
            },
            creation_url,
            referrer_policy: ReferrerPolicy::NoReferrerWhenDowngrade,
            time_origin: Instant::now(),
            event_sender,
            source_navigable_id,
            document_id,
            global_scope_ptr,
        })
    }

    /// Access the GlobalScope directly without traversing the boa GC heap.
    pub(crate) fn global_scope(&self) -> &GlobalScope {
        // Safety: global_scope_ptr points to a GlobalScope owned by the Window
        // inside self.context. Since self owns self.context, the pointer is valid
        // as long as self exists.
        unsafe { &*self.global_scope_ptr }
    }

    pub(crate) fn current_time_millis(&self) -> f64 {
        self.time_origin.elapsed().as_secs_f64() * 1000.0
    }

    pub fn clear_all_window_timers(&self) {
        self.global_scope().clear_all_timers();
    }

    pub fn evaluate_script(&mut self, source: &str) -> Result<(), String> {
        self.evaluate_script_without_microtask_checkpoint(source)?;
        self.perform_a_microtask_checkpoint()
    }

    fn evaluate_script_without_microtask_checkpoint(&mut self, source: &str) -> Result<(), String> {
        self.context
            .eval(Source::from_bytes(source))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn evaluate_script_to_json(&mut self, source: &str) -> Result<serde_json::Value, String> {
        let value = self
            .context
            .eval(Source::from_bytes(source))
            .map_err(|error| error.to_string())?;

        self.perform_a_microtask_checkpoint()?;

        value
            .to_json(&mut self.context)
            .map(|value| value.unwrap_or(serde_json::Value::Null))
            .map_err(|error| error.to_string())
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn run_animation_frame_callbacks(&mut self, now: f64) -> Result<(), String> {
        let callbacks =
            take_animation_frame_callbacks(&self.context).map_err(|error| error.to_string())?;

        for callback in callbacks {
            // Step 3.3: "Invoke callback with « now » and \"report\"."
            let mut host = ContextCallbackHost::new(&mut self.context, "animation frame callback");
            invoke_callback_function(
                &mut host,
                &callback,
                &[JsValue::from(now)],
                ExceptionBehavior::Report,
                None,
            )
            .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    /// <https://html.spec.whatwg.org/#timers>
    pub(crate) fn run_window_timer(
        &mut self,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    ) -> Result<(), String> {
        log_timer_debug(format!(
            "run timer id={} key={} nesting={}",
            timer_id, timer_key, nesting_level
        ));
        let previous_nesting_level =
            self.global_scope().set_current_timer_nesting_level(Some(nesting_level));

        let timer = self.global_scope().window_timer(timer_id, timer_key);

        let Some(timer) = timer else {
            log_timer_debug(format!(
                "run timer id={} key={} missing_registration",
                timer_id, timer_key
            ));
            self.global_scope().set_current_timer_nesting_level(previous_nesting_level);
            return Ok(());
        };

        match &timer.handler {
            TimerHandler::Function { callback } => {
                log_timer_debug(format!(
                    "invoke timer callback id={} key={} function",
                    timer_id, timer_key
                ));
                let global = JsValue::from(self.context.global_object());
                let mut host = ContextCallbackHost::new(&mut self.context, "timer callback");
                let callback_result = invoke_callback_function(
                    &mut host,
                    callback,
                    &timer.arguments,
                    ExceptionBehavior::Report,
                    Some(&global),
                );
                if let Err(error) = callback_result {
                    eprintln!("content error: {error}");
                }
            }
            TimerHandler::String { source } => {
                log_timer_debug(format!(
                    "invoke timer callback id={} key={} string_source_len={}",
                    timer_id,
                    timer_key,
                    source.len()
                ));
                if let Err(error) = self
                    .context
                    .eval(Source::from_bytes(source.as_str()))
                    .map(|_| ())
                {
                    eprintln!("content error: {error}");
                }
            }
        }

        let completion_result = self
            .global_scope()
            .complete_window_timer(timer_id, timer_key);
        self.global_scope().set_current_timer_nesting_level(previous_nesting_level);
        if let Err(error) = self.perform_a_microtask_checkpoint() {
            eprintln!("content error: {error}");
        }
        completion_result
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    pub fn perform_a_microtask_checkpoint(&mut self) -> Result<(), String> {
        self.context.run_jobs().map_err(|error| error.to_string())
    }
}

impl EcmascriptHost for EnvironmentSettingsObject {
    fn context(&mut self) -> &mut boa_engine::Context {
        &mut self.context
    }

    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        ContextCallbackHost::new(&mut self.context, "event listener").get(object, property)
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        match value.as_object() {
            Some(object) => object.is_callable(),
            None => false,
        }
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue> {
        ContextCallbackHost::new(&mut self.context, "event listener")
            .call(callable, this_arg, args)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        ContextCallbackHost::new(&mut self.context, "event listener")
            .perform_a_microtask_checkpoint()
    }

    fn report_exception(&mut self, error: JsError, callback: &Callback) {
        ContextCallbackHost::new(&mut self.context, "event listener")
            .report_exception(error, callback)
    }
}

impl EventDispatchHost for EnvironmentSettingsObject {
    fn create_event_object(&mut self, event: crate::dom::Event) -> JsResult<JsObject> {
        Class::from_data(event, &mut self.context)
    }

    fn document_object(&mut self) -> JsResult<JsObject> {
        document_object(&self.context)
    }

    fn global_object(&mut self) -> JsObject {
        self.context.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> JsResult<JsObject> {
        resolve_element_object(node_id, &mut self.context)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> JsResult<JsObject> {
        object_for_existing_node(document, node_id, &mut self.context)
    }

    fn current_time_millis(&self) -> f64 {
        EnvironmentSettingsObject::current_time_millis(self)
    }
}
