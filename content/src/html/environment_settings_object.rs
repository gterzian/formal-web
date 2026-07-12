use log::{debug, error};
use std::{cell::RefCell, rc::Rc, time::Instant};

use blitz_dom::BaseDocument;
use ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId, WindowTimerKey};
use url::Url;

use crate::dom::{Document, Event, EventDispatchHost};
use crate::html::{TimerHandler, Window};
use crate::js::build_context::{build_context, build_realm};
use crate::js::platform_objects::with_global_scope;
use crate::js::{
    Engine, Types, install_console_namespace, install_css_namespace, install_document_property,
};
use crate::webidl::bindings::{create_interface_instance, get_registry_prototype};
use js_engine::{Completion, EcmascriptHost, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
type JsString = <Types as JsTypes>::JsString;

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
        Self::new_in_realm(
            None,
            document,
            creation_url,
            event_sender,
            source_navigable_id,
            document_id,
        )
    }

    /// Like `new`, but creates the realm within an existing engine (sharing
    /// the same JS context / GC heap).  Used by `window.open`.
    pub fn new_in_realm(
        parent: Option<&mut Engine>,
        document: Rc<RefCell<BaseDocument>>,
        creation_url: Url,
        event_sender: Option<IpcSender<ContentEvent>>,
        source_navigable_id: Option<NavigableId>,
        document_id: Option<DocumentId>,
    ) -> Result<Self, String> {
        // Build the engine (fresh or child realm).
        let mut engine = match parent {
            Some(parent) => build_realm(parent, Rc::clone(&document))?,
            None => build_context(Rc::clone(&document))?,
        };

        // Set up timer host and navigation info on the GlobalScope through
        // the EC trait's realm_global_object + with_object_any.
        if let (Some(event_sender), Some(document_id)) = (&event_sender, document_id) {
            with_global_scope(&mut engine, |global_scope| {
                global_scope.set_timer_host(document_id, event_sender.clone());
                Ok(())
            })
            .map_err(|error| {
                engine
                    .to_rust_string(error)
                    .unwrap_or_else(|_| "unknown error".to_string())
            })?;
        }
        if let Some(navigable_id) = source_navigable_id {
            if let Some(event_sender) = &event_sender {
                with_global_scope(&mut engine, |global_scope| {
                    global_scope.set_navigation_info(navigable_id, event_sender.clone());
                    global_scope.set_creation_url(creation_url.clone());
                    Ok(())
                })
                .map_err(|error| {
                    engine
                        .to_rust_string(error)
                        .unwrap_or_else(|_| "unknown error".to_string())
                })?;
            }
        }

        let document_object = create_interface_instance::<crate::js::Types, Document>(
            Document::new(document.clone(), creation_url.clone()),
            &mut engine,
        )
        .map_err(|error| {
            engine
                .to_rust_string(error)
                .unwrap_or_else(|_| "unknown error".to_string())
        })?;

        with_global_scope(&mut engine, |global_scope| {
            global_scope.store_document_object(document_object);
            Ok(())
        })
        .map_err(|error| {
            engine
                .to_rust_string(error)
                .unwrap_or_else(|_| "unknown error".to_string())
        })?;
        install_document_property(&mut engine).map_err(|error| {
            engine
                .to_rust_string(error)
                .unwrap_or_else(|_| "unknown error".to_string())
        })?;
        install_console_namespace(&mut engine)
            .map_err(|error| format!("failed to install console: {error:?}"))?;
        install_css_namespace(&mut engine)
            .map_err(|error| format!("failed to install CSS namespace: {error:?}"))?;

        let global = engine.realm_global_object();
        let global_value = <Types as JsTypes>::value_from_object(global.clone());
        if let Some(window_proto) = get_registry_prototype::<crate::js::Types, Window>(&engine) {
            engine
                .set_prototype(global.clone(), Some(window_proto))
                .map_err(|error| {
                    engine
                        .to_rust_string(error)
                        .unwrap_or_else(|_| "failed to set prototype".to_string())
                })?;
        }
        engine
            .create_data_property(
                engine.realm_global_object(),
                engine.property_key_from_str("window"),
                global_value.clone(),
            )
            .map_err(|error| {
                engine
                    .to_rust_string(error)
                    .unwrap_or_else(|_| "failed to register window property".to_string())
            })?;
        engine
            .create_data_property(
                engine.realm_global_object(),
                engine.property_key_from_str("self"),
                global_value,
            )
            .map_err(|error| {
                engine
                    .to_rust_string(error)
                    .unwrap_or_else(|_| "failed to register self property".to_string())
            })?;

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

    /// Access the execution context for generic ECMA-262 operations.
    pub fn ec(&mut self) -> &mut dyn ExecutionContext<crate::js::Types> {
        &mut self.engine
    }

    /// Convert a JsValue error (Completion error) to a displayable String.
    fn error_to_string(&mut self, error: <Types as JsTypes>::JsValue) -> String {
        self.engine
            .to_rust_string(error)
            .unwrap_or_else(|_| "unknown error".to_string())
    }

    pub(crate) fn current_time_millis(&self) -> f64 {
        self.time_origin.elapsed().as_secs_f64() * 1000.0
    }

    pub fn clear_all_window_timers(&mut self) -> Result<(), String> {
        with_global_scope(&mut self.engine, |global_scope| {
            global_scope.clear_all_timers();
            Ok(())
        })
        .map_err(|error| self.error_to_string(error))
    }

    pub fn evaluate_script(&mut self, source: &str) -> Result<(), String> {
        self.evaluate_script_without_microtask_checkpoint(source)?;
        self.perform_a_microtask_checkpoint()?;
        Ok(())
    }

    fn evaluate_script_without_microtask_checkpoint(&mut self, source: &str) -> Result<(), String> {
        let result = self
            .engine
            .evaluate_script(source)
            .map(|_| ())
            .map_err(|error| self.error_to_string(error));
        result
    }

    pub fn evaluate_script_to_json(&mut self, source: &str) -> Result<serde_json::Value, String> {
        let value = self
            .engine
            .evaluate_script(source)
            .map_err(|error| self.error_to_string(error))?;

        self.perform_a_microtask_checkpoint()?;

        let json_string = self
            .engine
            .json_stringify(value)
            .map_err(|error| self.error_to_string(error))?;
        serde_json::from_str(&json_string).map_err(|error| format!("failed to parse JSON: {error}"))
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn run_animation_frame_callbacks(&mut self, now: f64) -> Result<(), String> {
        let callbacks =
            crate::js::platform_objects::take_animation_frame_callbacks(&mut self.engine)
                .map_err(|error| self.error_to_string(error))?;

        for callback in callbacks {
            // Step 3.3: "Invoke callback with « now » and \"report\"."
            let now_value = self.engine.value_from_number(now);
            if let Err(error) = crate::webidl::invoke_callback_function(
                &mut self.engine as &mut dyn EcmascriptHost<crate::js::Types>,
                &callback,
                &[now_value],
                crate::webidl::ExceptionBehavior::Report,
                None,
            ) {
                error!("callback error: {error:?}");
            }
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

        let previous_nesting_level = with_global_scope(&mut self.engine, |global_scope| {
            Ok(global_scope.set_current_timer_nesting_level(Some(nesting_level)))
        })
        .map_err(|error| self.error_to_string(error))?;

        let timer = with_global_scope(&mut self.engine, |global_scope| {
            Ok(global_scope.window_timer(timer_id, timer_key))
        })
        .map_err(|error| self.error_to_string(error))?;

        let Some(timer) = timer else {
            log_timer_debug(format!(
                "run timer id={} key={} missing_registration",
                timer_id, timer_key
            ));
            if let Err(error) = with_global_scope(&mut self.engine, |global_scope| {
                global_scope.set_current_timer_nesting_level(previous_nesting_level);
                Ok(())
            }) {
                error!(
                    "[timers] failed to reset timer nesting level: {}",
                    self.error_to_string(error)
                );
            }
            return Ok(());
        };

        match &timer.handler {
            TimerHandler::Function { callback } => {
                log_timer_debug(format!(
                    "invoke timer callback id={} key={} function",
                    timer_id, timer_key
                ));
                let global =
                    <Types as JsTypes>::value_from_object(self.engine.realm_global_object());
                if let Err(error) = crate::webidl::invoke_callback_function(
                    &mut self.engine as &mut dyn EcmascriptHost<crate::js::Types>,
                    callback,
                    &timer.arguments,
                    crate::webidl::ExceptionBehavior::Report,
                    Some(&global),
                ) {
                    error!("content error: {error:?}");
                }
            }
            TimerHandler::String { source } => {
                log_timer_debug(format!(
                    "invoke timer callback id={} key={} string_source_len={}",
                    timer_id,
                    timer_key,
                    source.len()
                ));
                if let Err(error) = self.engine.evaluate_script(source.as_str()).map(|_| ()) {
                    error!("content error: {error:?}");
                }
            }
        }

        if let Err(error) = with_global_scope(&mut self.engine, |global_scope| {
            if let Err(error) = global_scope.complete_window_timer(timer_id, timer_key) {
                error!("failed to complete window timer (id={timer_id} key={timer_key}): {error}");
            }
            Ok(())
        }) {
            error!(
                "failed to access global scope for timer completion: {}",
                self.error_to_string(error)
            );
        }
        if let Err(error) = with_global_scope(&mut self.engine, |global_scope| {
            global_scope.set_current_timer_nesting_level(previous_nesting_level);
            Ok(())
        }) {
            error!(
                "failed to access global scope for timer nesting level: {}",
                self.error_to_string(error)
            );
        }

        if let Err(error) = self.perform_a_microtask_checkpoint() {
            error!("[timer microtask] content error: {error}");
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    pub fn perform_a_microtask_checkpoint(&mut self) -> Result<(), String> {
        self.engine
            .perform_a_microtask_checkpoint()
            .map_err(|error| self.error_to_string(error))
    }

    /// Take all pending wasm batches (bytes + request_id) from the GlobalScope.
    /// Marks them as Processing.
    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn take_pending_wasm_batches(&self) -> Vec<(u64, Vec<u8>)> {
        let global = self.engine.realm_global_object();
        if let Some(window) = self
            .engine
            .with_object_any(&global)
            .and_then(|data| data.downcast_ref::<Window>())
        {
            window.global_scope.take_pending_wasm_batches()
        } else {
            Vec::new()
        }
    }

    /// Take all pending wasm instantiate requests (module + request_id)
    /// from the GlobalScope.  Marks them as Processing.
    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn take_pending_wasm_instantiates(&self) -> Vec<(u64, wasmtime::Module)> {
        let global = self.engine.realm_global_object();
        if let Some(window) = self
            .engine
            .with_object_any(&global)
            .and_then(|data| data.downcast_ref::<Window>())
        {
            window.global_scope.take_pending_wasm_instantiates()
        } else {
            Vec::new()
        }
    }

    /// Remove and return the promise + resolvers for a completed wasm request.
    #[cfg(all(boa_backend, feature = "wasm"))]
    pub(crate) fn consume_wasm_request(
        &self,
        request_id: u64,
    ) -> Option<(JsObject, js_engine::records::PromiseResolvers<Types>)> {
        let global = self.engine.realm_global_object();
        let window = self
            .engine
            .with_object_any(&global)
            .and_then(|data| data.downcast_ref::<Window>())?;
        window.global_scope.consume_wasm_request(request_id)
    }
}

impl js_engine::EcmascriptHost<crate::js::Types> for EnvironmentSettingsObject {
    fn get(
        &mut self,
        object: &JsObject,
        property: &str,
    ) -> js_engine::Completion<JsValue, crate::js::Types> {
        js_engine::EcmascriptHost::get(&mut self.engine, object, property)
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        self.engine.is_callable(value)
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> js_engine::Completion<JsValue, crate::js::Types> {
        self.engine.call(callable, this_arg, args)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> js_engine::Completion<(), crate::js::Types> {
        self.engine.perform_a_microtask_checkpoint()
    }

    fn report_exception(&mut self, error: JsValue) {
        self.engine.report_exception(error)
    }

    fn gc(&mut self) {
        self.engine.gc()
    }

    fn value_undefined(&mut self) -> JsValue {
        self.engine.value_undefined()
    }
    fn value_null(&mut self) -> JsValue {
        self.engine.value_null()
    }
    fn value_from_bool(&mut self, b: bool) -> JsValue {
        self.engine.value_from_bool(b)
    }
    fn value_from_number(&mut self, n: f64) -> JsValue {
        self.engine.value_from_number(n)
    }
    fn value_from_string(&mut self, s: JsString) -> JsValue {
        self.engine.value_from_string(s)
    }
    fn js_string_from_str(&self, s: &str) -> JsString {
        self.engine.js_string_from_str(s)
    }
}

impl EventDispatchHost for EnvironmentSettingsObject {
    fn ec(&mut self) -> &mut dyn ExecutionContext<crate::js::Types> {
        &mut self.engine
    }

    fn create_event_object(
        &mut self,
        event: crate::dom::Event,
    ) -> Completion<JsObject, crate::js::Types> {
        create_interface_instance::<crate::js::Types, Event>(event, &mut self.engine)
    }

    fn document_object(&mut self) -> Completion<JsObject, crate::js::Types> {
        crate::js::platform_objects::document_object(&mut self.engine)
    }

    fn global_object(&mut self) -> JsObject {
        self.engine.realm_global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> Completion<JsObject, crate::js::Types> {
        crate::js::platform_objects::resolve_element_object(node_id, &mut self.engine)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> Completion<JsObject, crate::js::Types> {
        crate::js::platform_objects::object_for_existing_node(document, node_id, &mut self.engine)
    }

    fn current_time_millis(&self) -> f64 {
        EnvironmentSettingsObject::current_time_millis(self)
    }
}
