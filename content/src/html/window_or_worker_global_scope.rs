use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsString, JsValue,
    builtins::promise::ResolvingFunctions,
    object::{JsObject, builtins::JsPromise},
};
use ipc_messages::content::{
    Event as ContentEvent, FetchRequest as ContentFetchRequest, HeaderList as ContentHeaderList,
};
use url::Url;

use crate::html::{GlobalScope, TimerHandler, Window, header_list_from_value};
use crate::new_document_fetch_id;
use crate::webidl::{Callback, error_to_rejection_reason};

use crate::html::safe_passing_of_structured_data::{self, StructuredCloneOptions};

/// <https://html.spec.whatwg.org/#windoworworkerglobalscope>
pub(crate) trait WindowOrWorkerGlobalScope {
    fn global_scope(&self) -> &GlobalScope;

    /// <https://html.spec.whatwg.org/#dom-structuredclone>
    fn structured_clone(
        &self,
        value: JsValue,
        options: Option<StructuredCloneOptions>,
        context: &mut Context,
    ) -> JsResult<JsValue> {
        safe_passing_of_structured_data::structured_clone(value, options, context)
    }

    /// <https://html.spec.whatwg.org/#dom-settimeout>
    fn set_timeout(
        &self,
        handler: &JsValue,
        timeout: &JsValue,
        arguments: Vec<JsValue>,
        context: &mut Context,
    ) -> JsResult<u32> {
        // Step 1: "Return the result of running the timer initialization steps given this, handler, timeout, arguments, and false."
        self.timer_initialization_steps(
            timer_handler(handler, context)?,
            timeout,
            arguments,
            false,
            None,
            context,
        )
    }

    /// <https://html.spec.whatwg.org/#dom-setinterval>
    fn set_interval(
        &self,
        handler: &JsValue,
        timeout: &JsValue,
        arguments: Vec<JsValue>,
        context: &mut Context,
    ) -> JsResult<u32> {
        // Step 1: "Return the result of running the timer initialization steps given this, handler, timeout, arguments, and true."
        self.timer_initialization_steps(
            timer_handler(handler, context)?,
            timeout,
            arguments,
            true,
            None,
            context,
        )
    }

    /// <https://html.spec.whatwg.org/#dom-cleartimeout>
    fn clear_timeout(&self, timer_id: u32) {
        // Step 1: "Remove handle from this's map of setTimeout and setInterval IDs."
        self.global_scope().clear_timer(timer_id);
    }

    /// <https://html.spec.whatwg.org/#dom-clearinterval>
    fn clear_interval(&self, timer_id: u32) {
        // Step 1: "Remove handle from this's map of setTimeout and setInterval IDs."
        self.global_scope().clear_timer(timer_id);
    }

    /// <https://fetch.spec.whatwg.org/#fetch-method>
    fn fetch(&self, input: &JsValue, init: &JsValue, context: &mut Context) -> JsResult<JsValue> {
        // Step 1: "Let p be a new promise."
        let (promise, resolvers) = JsPromise::new_pending(context);
        let promise_object: JsObject = promise.into();

        if let Err(error) = self.queue_fetch(input, init, &resolvers, context) {
            let reason = error_to_rejection_reason(error, context);
            reject_fetch_promise(&resolvers, reason, context)?;
        }

        // Step 13: "Return p."
        Ok(JsValue::from(promise_object))
    }

    fn queue_fetch(
        &self,
        input: &JsValue,
        init: &JsValue,
        resolvers: &ResolvingFunctions,
        context: &mut Context,
    ) -> JsResult<()> {
        // Step 2: "Let requestObject be the result of invoking the initial value of Request as constructor with input and init as arguments. If this throws an exception, reject p with it and return p."
        // Note: Formal-web does not expose the Request class yet. The helper below constructs the
        // subset of request fields that the existing IPC path can transport.
        let request = fetch_request_from_input_and_init(input, init, self.global_scope(), context)?;

        // TODO: Step 3: "Let request be requestObject’s request."
        // TODO: Step 4: "If requestObject’s signal is aborted, then:"
        // TODO: Step 5: "Let globalObject be request’s client’s global object."
        // TODO: Step 6: "If globalObject is a ServiceWorkerGlobalScope object, then set request’s service-workers mode to \"none\"."
        // TODO: Step 7: "Let responseObject be null."
        // TODO: Step 8: "Let relevantRealm be this’s relevant realm."
        // TODO: Step 9: "Let locallyAborted be false."
        // TODO: Step 10: "Let controller be null."
        // TODO: Step 11: "Add the following abort steps to requestObject’s signal:"
        // TODO: Step 12: "Set controller to the result of calling fetch given request and processResponse given response being these steps:"
        // TODO: Step 12 processResponse 1: "If locallyAborted is true, then abort these steps."
        // TODO: Step 12 processResponse 2: "If response’s aborted flag is set, then:"
        // TODO: Step 12 processResponse 3: "If response is a network error, then reject p with a TypeError and abort these steps."
        // Note: The IPC request below is formal-web callback plumbing. It starts the user-agent
        // fetch worker and resumes the Fetch method's processResponse continuation in the content
        // process when the callback ID completes.
        let handler_id = new_document_fetch_id();
        self.global_scope()
            .store_fetch_resolvers(handler_id, resolvers.clone());

        let Some(event_sender) = self.global_scope().event_sender() else {
            self.global_scope().take_fetch_resolvers(handler_id);
            return Err(type_error(
                "fetch is not available without a content event sender",
            ));
        };

        event_sender
            .send(ContentEvent::DocumentFetchRequested(ContentFetchRequest {
                handler_id,
                url: request.url.to_string(),
                method: request.method,
                header_list: request.header_list,
                body: request.body,
            }))
            .map_err(|error| {
                self.global_scope().take_fetch_resolvers(handler_id);
                type_error(format!("failed to send fetch request: {error}"))
            })
    }

    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    fn timer_initialization_steps(
        &self,
        handler: TimerHandler,
        timeout: &JsValue,
        arguments: Vec<JsValue>,
        repeat: bool,
        previous_id: Option<u32>,
        context: &mut Context,
    ) -> JsResult<u32> {
        // Step 1: "If method context is a Window object, then let thisArg be method context's WindowProxy object; otherwise let thisArg be method context."
        // Note: The callback invocation path always uses the current global object as `this`, so the Window case is implicit in the stored registration.

        // Step 2: "If previousId was given, let id be previousId; otherwise, let id be an implementation-defined integer that is greater than zero and does not already exist in global's map of setTimeout and setInterval IDs."
        // Note: `GlobalScope::timer_initialization_steps` owns the map of setTimeout and setInterval IDs, so it performs the concrete `id` allocation or reuse.

        // Step 3: "If the surrounding agent's event loop's currently running task is a task that was created by this algorithm, then let nesting level be that task's timer nesting level. Otherwise, let nesting level be 0."
        let nesting_level = self
            .global_scope()
            .current_timer_nesting_level()
            .unwrap_or(0);

        // Step 4: "Set timeout to the result of converting timeout to an IDL long."
        let mut timeout_ms = timeout_ms(timeout, context)?;

        // Step 5: "If timeout is less than 0, then set timeout to 0."
        // Note: `timeout_ms` already clamps negative values to zero during the IDL conversion.

        // Step 6: "If nesting level is greater than 5, and timeout is less than 4, then set timeout to 4."
        if nesting_level > 5 && timeout_ms < 4 {
            timeout_ms = 4;
        }

        // Step 7: "Let realm be global's relevant Realm."
        // Note: The current content process executes all timer callbacks inside the owning `EnvironmentSettingsObject` realm, so the realm selection is implicit.

        // Step 8: "Let uniqueHandle be null."
        // Note: The content process reserves the implementation-defined timer key during scheduling so the ID map can update synchronously before the timer worker runs `run steps after a timeout`.

        // Step 9: "Let task be a task that runs the following steps:"
        // Note: The timer worker queues the task on the timer task source and later re-enters the content process with `RunWindowTimer`.

        // Step 10: "Set task's timer nesting level to nesting level + 1."
        let task_nesting_level = nesting_level.saturating_add(1);

        // Step 11: "Let uniqueHandle be the result of running steps after a timeout given global, "setTimeout/setInterval", timeout, completionSteps, and timerKey if repeat is true, and which returns a unique internal value."
        self.global_scope()
            .timer_initialization_steps(
                previous_id,
                handler,
                arguments,
                repeat,
                timeout_ms,
                task_nesting_level,
            )
            .map_err(internal_error)
    }
}

impl WindowOrWorkerGlobalScope for Window {
    fn global_scope(&self) -> &GlobalScope {
        &self.global_scope
    }
}

struct WindowFetchRequest {
    url: Url,
    method: String,
    header_list: ContentHeaderList,
    body: String,
}

fn fetch_request_from_input_and_init(
    input: &JsValue,
    init: &JsValue,
    global_scope: &GlobalScope,
    context: &mut Context,
) -> JsResult<WindowFetchRequest> {
    let input = input.to_string(context)?.to_std_string_escaped();
    let base_url = global_scope
        .creation_url()
        .ok_or_else(|| type_error("fetch is not available without a creation URL"))?;
    let url = base_url
        .join(&input)
        .map_err(|error| type_error(format!("failed to parse fetch URL `{input}`: {error}")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(type_error("fetch URL must not include credentials"));
    }
    if global_scope.document_id().is_none() {
        return Err(type_error("fetch is not available without a document"));
    }

    let init = fetch_request_init(init, context)?;
    Ok(WindowFetchRequest {
        url,
        method: init.method,
        header_list: init.header_list,
        body: init.body,
    })
}

struct WindowFetchRequestInit {
    method: String,
    header_list: ContentHeaderList,
    body: String,
}

fn fetch_request_init(value: &JsValue, context: &mut Context) -> JsResult<WindowFetchRequestInit> {
    let mut init = WindowFetchRequestInit {
        method: String::from("GET"),
        header_list: ContentHeaderList::default(),
        body: String::new(),
    };

    if value.is_null_or_undefined() {
        return Ok(init);
    }

    let Some(object) = value.as_object() else {
        return Err(type_error("fetch init must be an object"));
    };

    let method = object.get(js_string("method"), context)?;
    if !method.is_null_or_undefined() {
        init.method = method.to_string(context)?.to_std_string_escaped();
    }

    let headers = object.get(js_string("headers"), context)?;
    if !headers.is_null_or_undefined() {
        init.header_list = header_list_from_value(&headers, context)?;
    }

    let body = object.get(js_string("body"), context)?;
    if !body.is_null_or_undefined() {
        init.body = body.to_string(context)?.to_std_string_escaped();
    }

    Ok(init)
}

fn reject_fetch_promise(
    resolvers: &ResolvingFunctions,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    resolvers
        .reject
        .call(&JsValue::undefined(), &[reason], context)
        .map(|_| ())
}

fn js_string(value: &'static str) -> JsString {
    JsString::from(value)
}

fn timer_handler(value: &JsValue, context: &mut Context) -> JsResult<TimerHandler> {
    if let Some(object) = value.as_object() {
        if object.is_callable() {
            return Ok(TimerHandler::Function {
                callback: Callback::from_object(object.clone()),
            });
        }
    }

    Ok(TimerHandler::String {
        source: value.to_string(context)?.to_std_string_escaped(),
    })
}

fn timeout_ms(value: &JsValue, context: &mut Context) -> JsResult<u32> {
    let timeout = value.to_number(context)?;
    if !timeout.is_finite() || timeout <= 0.0 {
        return Ok(0);
    }
    Ok(timeout.floor().min(i32::MAX as f64) as u32)
}

fn internal_error(message: String) -> JsError {
    JsError::from(JsNativeError::typ().with_message(message))
}

fn type_error(message: impl Into<String>) -> JsError {
    JsError::from(JsNativeError::typ().with_message(message.into()))
}
