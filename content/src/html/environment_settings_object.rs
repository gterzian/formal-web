use log::{debug, error};
use std::{cell::RefCell, rc::Rc, time::Instant};

use blitz_dom::BaseDocument;
use boa_engine::{
    js_string, object::JsObject, property::Attribute, Context, JsResult, JsValue, Source,
};
use ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId, WindowTimerKey};
use url::Url;

use crate::dom::{Document, Event, EventDispatchHost};
use crate::html::{TimerHandler, Window};
use crate::js::bindings::html::build_boa_engine;
use crate::js::platform_objects::{store_document_object, with_global_scope};
use crate::js::{
    install_console_namespace, install_css_namespace, install_document_property, Engine,
};
use crate::webidl::bindings::{create_interface_instance, get_registry_prototype};

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        debug!("[timer-debug][settings] {}", message.as_ref());
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
    ///
    /// The engine wraps a `boa_engine::Context` and implements
    /// `JsEngine<BoaTypes>`.  Access the underlying context via
    /// `self.context()` for Boa-specific operations that are not yet
    /// abstracted through `JsEngine`.
    pub engine: Engine,

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
}

impl EnvironmentSettingsObject {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        creation_url: Url,
        event_sender: Option<IpcSender<ContentEvent>>,
        source_navigable_id: Option<NavigableId>,
        document_id: Option<DocumentId>,
    ) -> Result<Self, String> {
        // Build the engine. WindowHostHooks creates the Window and its
        // GlobalScope during build().
        let mut engine = build_boa_engine(Rc::clone(&document))?;

        // Install timer host and navigation info on the GlobalScope through the
        // safe boa API (with_global_scope — traverses the GC heap to reach the
        // Window's GlobalScope).
        if let (Some(event_sender), Some(document_id)) = (&event_sender, document_id) {
            with_global_scope(engine.context_ref(), |global_scope| {
                global_scope.set_timer_host(document_id, event_sender.clone());
                Ok(())
            })
            .map_err(|error| error.to_string())?;
        }
        if let Some(navigable_id) = source_navigable_id {
            if let Some(event_sender) = &event_sender {
                with_global_scope(engine.context_ref(), |global_scope| {
                    global_scope.set_navigation_info(navigable_id, event_sender.clone());
                    global_scope.set_creation_url(creation_url.clone());
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            }
        }

        let document_object = create_interface_instance::<Document>(
            Document::new(document.clone(), creation_url.clone()),
            engine.context(),
        )
        .map_err(|error| error.to_string())?;

        store_document_object(engine.context_ref(), document_object)
            .map_err(|error| error.to_string())?;
        install_document_property(engine.context())
            .map_err(|error| error.to_string())?;
        install_console_namespace(engine.context())
            .map_err(|error: boa_engine::JsError| error.to_string())?;
        install_css_namespace(engine.context())
            .map_err(|error| error.to_string())?;

        let global = engine.context().global_object();
        if let Some(window_proto) = get_registry_prototype::<Window>(engine.context_ref()) {
            global.set_prototype(Some(window_proto));
        }
        engine
            .context()
            .register_global_property(js_string!("window"), global.clone(), Attribute::all())
            .map_err(|error| error.to_string())?;
        engine
            .context()
            .register_global_property(js_string!("self"), global, Attribute::all())
            .map_err(|error| error.to_string())?;

        Ok(Self {
            engine,
            document,
            origin: Origin {
                serialized: creation_url.origin().unicode_serialization(),
            },
            creation_url,
            referrer_policy: ReferrerPolicy::NoReferrerWhenDowngrade,
            time_origin: Instant::now(),
        })
    }

    /// Access the underlying Boa context (mutable).
    ///
    /// Temporary compatibility shim. Prefer using `self.engine` directly
    /// and calling `JsEngine` trait methods.
    pub fn context(&mut self) -> &mut Context {
        self.engine.context()
    }

    /// Access the underlying Boa context (immutable).
    ///
    /// Needed for functions that take `&Context` (e.g. `with_global_scope`).
    pub fn context_ref(&self) -> &Context {
        self.engine.context_ref()
    }

    pub(crate) fn current_time_millis(&self) -> f64 {
        self.time_origin.elapsed().as_secs_f64() * 1000.0
    }

    pub fn clear_all_window_timers(&self) -> Result<(), String> {
        with_global_scope(self.context_ref(), |global_scope| {
            global_scope.clear_all_timers();
            Ok(())
        })
        .map_err(|error| error.to_string())
    }

    pub fn evaluate_script(&mut self, source: &str) -> Result<(), String> {
        self.evaluate_script_without_microtask_checkpoint(source)?;
        self.perform_a_microtask_checkpoint()
    }

    fn evaluate_script_without_microtask_checkpoint(&mut self, source: &str) -> Result<(), String> {
        self.context()
            .eval(Source::from_bytes(source))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn evaluate_script_to_json(&mut self, source: &str) -> Result<serde_json::Value, String> {
        let value = self
            .context()
            .eval(Source::from_bytes(source))
            .map_err(|error| error.to_string())?;

        self.perform_a_microtask_checkpoint()?;

        value
            .to_json(self.context())
            .map(|value| value.unwrap_or(serde_json::Value::Null))
            .map_err(|error| error.to_string())
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn run_animation_frame_callbacks(&mut self, now: f64) -> Result<(), String> {
        let callbacks = crate::js::platform_objects::take_animation_frame_callbacks(self.context())
            .map_err(|error| error.to_string())?;

        for callback in callbacks {
            // Step 3.3: "Invoke callback with « now » and \"report\"."
            crate::webidl::invoke_callback_function(
                &mut self.engine,
                &callback,
                &[JsValue::from(now)],
                crate::webidl::ExceptionBehavior::Report,
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

        let previous_nesting_level = with_global_scope(self.context_ref(), |global_scope| {
            Ok(global_scope.set_current_timer_nesting_level(Some(nesting_level)))
        })
        .map_err(|error| error.to_string())?;

        let timer = with_global_scope(self.context_ref(), |global_scope| {
            Ok(global_scope.window_timer(timer_id, timer_key))
        })
        .map_err(|error| error.to_string())?;

        let Some(timer) = timer else {
            log_timer_debug(format!(
                "run timer id={} key={} missing_registration",
                timer_id, timer_key
            ));
            if let Err(error) = with_global_scope(self.context_ref(), |global_scope| {
                global_scope.set_current_timer_nesting_level(previous_nesting_level);
                Ok(())
            }) {
                error!("[timers] failed to reset timer nesting level: {error}");
            }
            return Ok(());
        };

        match &timer.handler {
            TimerHandler::Function { callback } => {
                log_timer_debug(format!(
                    "invoke timer callback id={} key={} function",
                    timer_id, timer_key
                ));
                let global = JsValue::from(self.context().global_object());
                let callback_result = crate::webidl::invoke_callback_function(
                    &mut self.engine,
                    callback,
                    &timer.arguments,
                    crate::webidl::ExceptionBehavior::Report,
                    Some(&global),
                );
                if let Err(error) = callback_result {
                    error!("content error: {error}");
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
                    .context()
                    .eval(Source::from_bytes(source.as_str()))
                    .map(|_| ())
                {
                    error!("content error: {error}");
                }
            }
        }

        if let Err(error) = with_global_scope(self.context_ref(), |global_scope| {
            if let Err(error) = global_scope.complete_window_timer(timer_id, timer_key) {
                error!("failed to complete window timer (id={timer_id} key={timer_key}): {error}");
            }
            Ok(())
        }) {
            error!("failed to access global scope for timer completion: {error}");
        }
        if let Err(error) = with_global_scope(self.context_ref(), |global_scope| {
            global_scope.set_current_timer_nesting_level(previous_nesting_level);
            Ok(())
        }) {
            error!("failed to access global scope for timer nesting level: {error}");
        }

        if let Err(error) = self.perform_a_microtask_checkpoint() {
            error!("content error: {error}");
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    pub fn perform_a_microtask_checkpoint(&mut self) -> Result<(), String> {
        self.context().run_jobs().map_err(|error| error.to_string())
    }

    /// Take all pending wasm batches (bytes + request_id) from the GlobalScope.
    /// Marks them as Processing.
    pub(crate) fn take_pending_wasm_batches(&self) -> Vec<(u64, Vec<u8>)> {
        let global = self.context_ref().global_object();
        if let Some(window) = global.downcast_ref::<Window>() {
            window.global_scope.take_pending_wasm_batches()
        } else {
            Vec::new()
        }
    }

    /// Take all pending wasm instantiate requests (module + request_id)
    /// from the GlobalScope.  Marks them as Processing.
    pub(crate) fn take_pending_wasm_instantiates(&self) -> Vec<(u64, wasmtime::Module)> {
        let global = self.context_ref().global_object();
        if let Some(window) = global.downcast_ref::<Window>() {
            window.global_scope.take_pending_wasm_instantiates()
        } else {
            Vec::new()
        }
    }

    /// Remove and return the promise + resolvers for a completed wasm request.
    pub(crate) fn consume_wasm_request(
        &self,
        request_id: u64,
    ) -> Option<(
        boa_engine::object::JsObject,
        boa_engine::builtins::promise::ResolvingFunctions,
    )> {
        let global = self.context_ref().global_object();
        let window = global.downcast_ref::<Window>()?;
        window.global_scope.consume_wasm_request(request_id)
    }
}

impl js_engine::EcmascriptHost<js_engine::boa::BoaTypes> for EnvironmentSettingsObject {
    fn get(
        &mut self,
        object: &JsObject,
        property: &str,
    ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes> {
        self.engine.get(object, property)
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        self.engine.is_callable(value)
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes> {
        self.engine.call(callable, this_arg, args)
    }

    fn perform_a_microtask_checkpoint(
        &mut self,
    ) -> js_engine::Completion<(), js_engine::boa::BoaTypes> {
        self.engine.perform_a_microtask_checkpoint()
    }

    fn report_exception(&mut self, error: JsValue) {
        self.engine.report_exception(error)
    }

    fn value_undefined(&mut self) -> JsValue { self.engine.value_undefined() }
    fn value_null(&mut self) -> JsValue { self.engine.value_null() }
    fn value_from_bool(&mut self, b: bool) -> JsValue { self.engine.value_from_bool(b) }
    fn value_from_number(&mut self, n: f64) -> JsValue { self.engine.value_from_number(n) }
    fn value_from_string(&mut self, s: boa_engine::JsString) -> JsValue { self.engine.value_from_string(s) }
}

impl EventDispatchHost for EnvironmentSettingsObject {
    fn context(&mut self) -> &mut boa_engine::Context {
        self.context()
    }

    fn create_event_object(&mut self, event: crate::dom::Event) -> JsResult<JsObject> {
        create_interface_instance::<Event>(event, self.context())
    }

    fn document_object(&mut self) -> JsResult<JsObject> {
        crate::js::platform_objects::document_object(self.context())
    }

    fn global_object(&mut self) -> JsObject {
        self.context().global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> JsResult<JsObject> {
        crate::js::platform_objects::resolve_element_object(node_id, self.context())
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> JsResult<JsObject> {
        crate::js::platform_objects::object_for_existing_node(document, node_id, self.context())
    }

    fn current_time_millis(&self) -> f64 {
        EnvironmentSettingsObject::current_time_millis(self)
    }
}
