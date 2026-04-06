use std::{collections::HashMap, rc::Rc, time::Instant};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, Source,
    class::Class,
    context::{ContextBuilder, HostHooks, intrinsics::Intrinsics},
    job::SimpleJobExecutor,
    js_string,
    object::JsObject,
    property::Attribute,
};
use url::Url;

use crate::dom::{Document, GlobalScope, GlobalScopeKind, Node, UIEvent, Window};

use super::{bindings, runtime_data::RuntimeData, task_queue::{PendingCallback, TaskQueue}};

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

impl HostHooks for WindowHostHooks {
    fn create_global_object(&self, intrinsics: &Intrinsics) -> JsObject {
        JsObject::from_proto_and_data(
            intrinsics.constructors().object().prototype(),
            Window {
                event_target: crate::dom::EventTarget::default(),
                global_scope: GlobalScope {
                    kind: GlobalScopeKind::Window,
                },
            },
        )
    }
}

impl JsState {
    pub fn new(document: Rc<std::cell::RefCell<BaseDocument>>, creation_url: Url) -> Result<Self, String> {
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

impl JsExecutionContext {
    pub fn new(document: Rc<std::cell::RefCell<BaseDocument>>) -> Result<Self, String> {
        let mut context = ContextBuilder::new()
            .host_hooks(Rc::new(WindowHostHooks))
            .job_executor(Rc::new(SimpleJobExecutor::new()))
            .build()
            .map_err(|error| error.to_string())?;

        context.insert_data(RuntimeData::new(Rc::clone(&document)));
        context
            .register_global_class::<crate::dom::EventTarget>()
            .map_err(|error| error.to_string())?;
        context
            .register_global_class::<crate::dom::Event>()
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
            .register_global_class::<crate::dom::Element>()
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
        bindings::store_document_object(&context, document_object.clone())
            .map_err(|error| error.to_string())?;
        bindings::install_document_property(&mut context).map_err(|error| error.to_string())?;
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
        self.run_microtasks()
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
            self.run_microtasks()?;
        }
        Ok(())
    }

    pub fn run_microtasks(&mut self) -> Result<(), String> {
        self.context.run_jobs().map_err(|error| error.to_string())
    }

    pub fn register_callback(
        &mut self,
        callback: impl FnOnce(&mut JsExecutionContext, ipc_messages::content::CallbackData) -> Result<(), String> + 'static,
    ) -> u64 {
        let callback_id = self.next_callback_id;
        self.next_callback_id += 1;
        self.pending_callbacks.insert(callback_id, Box::new(callback));
        callback_id
    }

    pub fn resolve_callback(
        &mut self,
        callback_id: u64,
        data: ipc_messages::content::CallbackData,
    ) {
        if let Some(callback) = self.pending_callbacks.remove(&callback_id) {
            self.enqueue_task(move |execution_context| callback(execution_context, data));
        }
    }
}