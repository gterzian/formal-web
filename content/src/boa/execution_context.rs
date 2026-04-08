use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

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
use ipc_messages::content::CallbackData;
use url::Url;

use crate::dom::{
    Document, Element, Event, EventDispatchHost, EventTarget, GlobalScope, GlobalScopeKind, Node,
    UIEvent, Window,
};
use crate::webidl::EcmascriptHost;

use super::{
    bindings::install_console_namespace,
    platform_objects::{
        document_object, object_for_existing_node, resolve_element_object, store_document_object,
    },
    runtime_data::RuntimeData,
    task_queue::{PendingCallback, TaskQueue},
};

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
    /// <https://html.spec.whatwg.org/#environment>
    pub execution_context: JsExecutionContext,

    /// <https://html.spec.whatwg.org/#concept-settings-object-origin>
    pub origin: Origin,

    /// <https://html.spec.whatwg.org/#concept-environment-creation-url>
    pub creation_url: Url,

    /// <https://html.spec.whatwg.org/#concept-settings-object-policy-container>
    pub referrer_policy: ReferrerPolicy,
}

/// <https://html.spec.whatwg.org/#environment-settings-object>
pub struct JsState {
    /// <https://html.spec.whatwg.org/#environment-settings-object>
    pub settings: EnvironmentSettingsObject,
}

/// <https://html.spec.whatwg.org/#environment>
pub struct JsExecutionContext {
    /// <https://html.spec.whatwg.org/#realms-settings-objects-global-objects>
    pub context: Context,

    /// <https://dom.spec.whatwg.org/#dom-event-timestamp>
    pub navigation_start: Instant,

    /// <https://html.spec.whatwg.org/#task-queue>
    pub task_queue: TaskQueue,

    /// <https://html.spec.whatwg.org/#queue-a-task>
    pending_callbacks: HashMap<u64, PendingCallback>,

    /// <https://infra.spec.whatwg.org/#ordered-map>
    next_callback_id: u64,
}

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Default)]
struct WindowHostHooks;

pub(crate) struct ContextEventDispatchHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextEventDispatchHost<'a> {
    pub(crate) fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl HostHooks for WindowHostHooks {
    fn create_global_object(&self, intrinsics: &Intrinsics) -> JsObject {
        JsObject::from_proto_and_data(
            intrinsics.constructors().object().prototype(),
            Window {
                event_target: EventTarget::default(),
                global_scope: GlobalScope {
                    kind: GlobalScopeKind::Window,
                },
            },
        )
    }
}

impl JsState {
    pub fn new(document: Rc<RefCell<BaseDocument>>, creation_url: Url) -> Result<Self, String> {
        let execution_context = JsExecutionContext::new(Rc::clone(&document))?;
        Ok(Self {
            settings: EnvironmentSettingsObject {
                execution_context,
                origin: Origin {
                    serialized: creation_url.origin().unicode_serialization(),
                },
                creation_url,
                referrer_policy: ReferrerPolicy::NoReferrerWhenDowngrade,
            },
        })
    }
}

impl EcmascriptHost for ContextEventDispatchHost<'_> {
    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        object.get(JsString::from(property), self.context)
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
        function.call(this_arg, args, self.context)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        self.context.run_jobs()
    }

    fn report_exception(&mut self, error: JsError, _callback: &JsObject) {
        eprintln!("uncaught event listener error: {error}");
    }
}

impl EventDispatchHost for ContextEventDispatchHost<'_> {
    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        Event::from_data(event, self.context)
    }

    fn document_object(&mut self) -> JsResult<JsObject> {
        document_object(self.context)
    }

    fn global_object(&mut self) -> JsObject {
        self.context.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> JsResult<JsObject> {
        resolve_element_object(node_id, self.context)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> JsResult<JsObject> {
        object_for_existing_node(document, node_id, self.context)
    }
}

impl JsExecutionContext {
    pub fn new(document: Rc<RefCell<BaseDocument>>) -> Result<Self, String> {
        let mut context = ContextBuilder::new()
            .host_hooks(Rc::new(WindowHostHooks))
            .job_executor(Rc::new(SimpleJobExecutor::new()))
            .build()
            .map_err(|error| error.to_string())?;

        context.insert_data(RuntimeData::new(Rc::clone(&document)));
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

        let document_object = Document::from_data(Document::new(document), &mut context)
            .map_err(|error| error.to_string())?;
        store_document_object(&context, document_object.clone())
            .map_err(|error| error.to_string())?;
        super::bindings::install_document_property(&mut context)
            .map_err(|error| error.to_string())?;
        install_console_namespace(&mut context).map_err(|error| error.to_string())?;
        context
            .register_global_property(js_string!("window"), global.clone(), Attribute::all())
            .map_err(|error| error.to_string())?;
        context
            .register_global_property(js_string!("self"), global, Attribute::all())
            .map_err(|error| error.to_string())?;

        Ok(Self {
            context,
            navigation_start: Instant::now(),
            task_queue: TaskQueue::default(),
            pending_callbacks: HashMap::new(),
            next_callback_id: 1,
        })
    }

    pub fn evaluate_script(&mut self, source: &str) -> Result<(), String> {
        self.context
            .eval(Source::from_bytes(source))
            .map(|_| ())
            .map_err(|error| error.to_string())?;
        self.perform_a_microtask_checkpoint()
    }

    pub fn enqueue_task(
        &mut self,
        task: impl FnOnce(&mut JsExecutionContext) -> Result<(), String> + 'static,
    ) {
        self.task_queue.push(task);
    }

    pub fn drain_tasks(&mut self) -> Result<(), String> {
        while let Some(task) = self.task_queue.pop_front() {
            task.run(self)?;

            // Note: After each queued task returns to Rust, the content runtime performs <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint> before taking the next task.
            self.perform_a_microtask_checkpoint()?;
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    pub fn perform_a_microtask_checkpoint(&mut self) -> Result<(), String> {
        self.context.run_jobs().map_err(|error| error.to_string())
    }

    pub fn register_callback(
        &mut self,
        callback: impl FnOnce(&mut JsExecutionContext, CallbackData) -> Result<(), String> + 'static,
    ) -> u64 {
        let callback_id = self.next_callback_id;
        self.next_callback_id += 1;
        self.pending_callbacks
            .insert(callback_id, Box::new(callback));
        callback_id
    }

    pub fn resolve_callback(&mut self, callback_id: u64, data: CallbackData) {
        if let Some(callback) = self.pending_callbacks.remove(&callback_id) {
            self.enqueue_task(move |execution_context| callback(execution_context, data));
        }
    }
}

impl EcmascriptHost for JsExecutionContext {
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

impl EventDispatchHost for JsExecutionContext {
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
}
