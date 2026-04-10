use std::{cell::RefCell, rc::Rc, time::Instant};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsString, JsValue, Source,
    class::Class,
    context::{ContextBuilder, HostHooks, intrinsics::Intrinsics},
    job::SimpleJobExecutor,
    js_string,
    object::{JsObject, builtins::JsFunction},
    property::Attribute,
};
use url::Url;

use crate::boa::{
    install_console_namespace,
    install_document_property,
    platform_objects::{
        document_object, object_for_existing_node, resolve_element_object, store_document_object,
        take_animation_frame_callbacks,
    },
};
use crate::dom::{
    Document, Element, Event, EventDispatchHost, EventTarget, GlobalScope, GlobalScopeKind, Node,
    UIEvent, Window,
};
use crate::webidl::{EcmascriptHost, ExceptionBehavior, invoke_callback_function};

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

/// <https://html.spec.whatwg.org/#global-object>
struct WindowHostHooks {
    document: Rc<RefCell<BaseDocument>>,
}

impl WindowHostHooks {
    fn new(document: Rc<RefCell<BaseDocument>>) -> Self {
        Self { document }
    }
}

/// <https://html.spec.whatwg.org/#environment-settings-object>
pub struct EnvironmentSettingsObject {
    /// <https://html.spec.whatwg.org/#realms-settings-objects-global-objects>
    pub context: Context,

    /// <https://html.spec.whatwg.org/#concept-settings-object-origin>
    pub origin: Origin,

    /// <https://html.spec.whatwg.org/#concept-environment-creation-url>
    pub creation_url: Url,

    /// <https://html.spec.whatwg.org/#concept-settings-object-policy-container>
    pub referrer_policy: ReferrerPolicy,

    /// <https://html.spec.whatwg.org/#concept-settings-object-time-origin>
    pub time_origin: Instant,
}

impl HostHooks for WindowHostHooks {
    fn create_global_object(&self, intrinsics: &Intrinsics) -> JsObject {
        JsObject::from_proto_and_data(
            intrinsics.constructors().object().prototype(),
            Window {
                event_target: EventTarget::default(),
                global_scope: GlobalScope::new(GlobalScopeKind::Window, Rc::clone(&self.document)),
            },
        )
    }
}

impl EnvironmentSettingsObject {
    pub fn new(document: Rc<RefCell<BaseDocument>>, creation_url: Url) -> Result<Self, String> {
        let mut context = ContextBuilder::new()
            .host_hooks(Rc::new(WindowHostHooks::new(Rc::clone(&document))))
            .job_executor(Rc::new(SimpleJobExecutor::new()))
            .build()
            .map_err(|error| error.to_string())?;

        context
            .register_global_class::<EventTarget>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<Event>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<UIEvent>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<Node>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<Document>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<Element>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<Window>()
            .map_err(|error| error.to_string())?;

        let global = context.global_object();
        if let Some(window_class) = context.get_global_class::<Window>() {
            global.set_prototype(Some(window_class.prototype()));
        }

        let document_object =
            Document::from_data(Document::new(document), &mut context).map_err(|error| error.to_string())?;
        store_document_object(&context, document_object.clone()).map_err(|error| error.to_string())?;
        install_document_property(&mut context).map_err(|error| error.to_string())?;
        install_console_namespace(&mut context).map_err(|error| error.to_string())?;
        context
            .register_global_property(js_string!("window"), global.clone(), Attribute::all())
            .map_err(|error| error.to_string())?;
        context
            .register_global_property(js_string!("self"), global, Attribute::all())
            .map_err(|error| error.to_string())?;

        Ok(Self {
            context,
            origin: Origin {
                serialized: creation_url.origin().unicode_serialization(),
            },
            creation_url,
            referrer_policy: ReferrerPolicy::NoReferrerWhenDowngrade,
            time_origin: Instant::now(),
        })
    }

    pub(crate) fn current_time_millis(&self) -> f64 {
        self.time_origin.elapsed().as_secs_f64() * 1000.0
    }

    pub fn evaluate_script(&mut self, source: &str) -> Result<(), String> {
        self.context
            .eval(Source::from_bytes(source))
            .map(|_| ())
            .map_err(|error| error.to_string())?;
        self.perform_a_microtask_checkpoint()
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn run_animation_frame_callbacks(&mut self, now: f64) -> Result<(), String> {
        let callbacks = take_animation_frame_callbacks(&self.context).map_err(|error| error.to_string())?;

        for callback in callbacks {
            // Step 3.3: "Invoke callback with « now » and \"report\"."
            invoke_callback_function(
                self,
                &callback,
                &[JsValue::from(now)],
                ExceptionBehavior::Report,
                None,
            )
            .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    pub fn perform_a_microtask_checkpoint(&mut self) -> Result<(), String> {
        self.context.run_jobs().map_err(|error| error.to_string())
    }
}

impl EcmascriptHost for EnvironmentSettingsObject {
    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        object.get(JsString::from(property), &mut self.context)
    }

    fn is_callable(&self, object: &JsObject) -> bool {
        object.is_callable()
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message("callback is not callable"))
        })?;
        function.call(this_arg, args, &mut self.context)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        self.context.run_jobs()
    }

    fn report_exception(&mut self, error: JsError, _callback: &JsObject) {
        eprintln!("uncaught event listener error: {error}");
    }
}

impl EventDispatchHost for EnvironmentSettingsObject {
    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        Event::from_data(event, &mut self.context)
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