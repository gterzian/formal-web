use std::cell::{Cell, RefCell};
use std::rc::Rc;

use data_url::DataUrl;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use url::Url;

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::html::GlobalScope;
use crate::js::Types;
use crate::webidl::syntax_error_value;

use super::worker::WorkerType;
use super::worker_location::WorkerLocation;
use super::worker_navigator::WorkerNavigator;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
#[gc_struct]
pub(crate) struct WorkerGlobalScope {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub(crate) event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub(crate) global_scope: GlobalScope,

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-name>
    #[ignore_trace]
    pub(crate) name: String,

    /// The worker's type, from the WorkerOptions the constructor was given
    /// (options["type"]): "classic" or "module".  Only "classic" script
    /// evaluation is implemented.
    /// <https://html.spec.whatwg.org/#enumdef-workertype>
    #[ignore_trace]
    pub(crate) worker_type: WorkerType,

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-url>
    #[ignore_trace]
    pub(crate) url: Rc<RefCell<Option<Url>>>,

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-closing>
    #[ignore_trace]
    pub(crate) closing_flag: Rc<Cell<bool>>,

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
            location_object: gc_cell_new(None, ec),
            navigator_object: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-url>
    pub(crate) fn set_url(&self, url: Url) {
        *self.url.borrow_mut() = Some(url);
    }

    /// <https://html.spec.whatwg.org/#concept-workerglobalscope-url>
    pub(crate) fn url(&self) -> Option<Url> {
        self.url.borrow().clone()
    }

    /// <https://html.spec.whatwg.org/#dom-workerglobalscope-self>
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

    /// <https://html.spec.whatwg.org/#workernavigator>
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
