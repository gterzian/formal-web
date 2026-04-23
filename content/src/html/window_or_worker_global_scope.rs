use boa_engine::{Context, JsError, JsNativeError, JsResult, JsValue};

use crate::html::{GlobalScope, TimerHandler, Window};

/// <https://html.spec.whatwg.org/#windoworworkerglobalscope>
pub(crate) trait WindowOrWorkerGlobalScope {
    fn global_scope(&self) -> &GlobalScope;

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
        // Note: The current content runtime executes all timer callbacks inside the owning `EnvironmentSettingsObject` realm, so the realm selection is implicit.

        // Step 8: "Let uniqueHandle be null."
        // Note: The content runtime reserves the implementation-defined timer key during scheduling so the ID map can update synchronously before Lean runs `run steps after a timeout`.

        // Step 9: "Let task be a task that runs the following steps:"
        // Note: Lean queues the task on the timer task source and later re-enters the content runtime with `RunWindowTimer`.

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

fn timer_handler(value: &JsValue, context: &mut Context) -> JsResult<TimerHandler> {
    if let Some(object) = value.as_object() {
        if object.is_callable() {
            return Ok(TimerHandler::Function {
                callback: object.clone(),
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
