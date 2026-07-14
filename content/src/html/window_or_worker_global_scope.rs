use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

use crate::html::{GlobalScope, TimerHandler, Window};
use crate::webidl::Callback;

use crate::html::safe_passing_of_structured_data::{self, StructuredCloneOptions};

/// <https://html.spec.whatwg.org/#windoworworkerglobalscope>
pub(crate) trait WindowOrWorkerGlobalScope {
    fn global_scope(&self) -> &GlobalScope;

    /// <https://html.spec.whatwg.org/#dom-structuredclone>
    fn structured_clone(
        &self,
        value: JsValue,
        options: Option<StructuredCloneOptions>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        safe_passing_of_structured_data::structured_clone(value, options, ec)
    }

    /// <https://html.spec.whatwg.org/#dom-settimeout>
    fn set_timeout(
        &self,
        handler: &JsValue,
        timeout: &JsValue,
        arguments: Vec<JsValue>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<u32, crate::js::Types> {
        // Step 1: "Return the result of running the timer initialization steps given this, handler, timeout, arguments, and false."
        let handler = timer_handler(handler, ec)?;
        self.timer_initialization_steps(handler, timeout, arguments, false, None, ec)
    }

    /// <https://html.spec.whatwg.org/#dom-setinterval>
    fn set_interval(
        &self,
        handler: &JsValue,
        timeout: &JsValue,
        arguments: Vec<JsValue>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<u32, crate::js::Types> {
        // Step 1: "Return the result of running the timer initialization steps given this, handler, timeout, arguments, and true."
        let handler = timer_handler(handler, ec)?;
        self.timer_initialization_steps(handler, timeout, arguments, true, None, ec)
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
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<u32, crate::js::Types> {
        // Step 1-3: thisArg, id allocation, nesting level.
        let nesting_level = self
            .global_scope()
            .current_timer_nesting_level()
            .unwrap_or(0);

        // Step 4: "Set timeout to the result of converting timeout to an IDL long."
        let mut timeout_ms = timeout_ms(timeout, ec)?;

        // Step 5-6: clamp and nesting-level adjustments.
        if nesting_level > 5 && timeout_ms < 4 {
            timeout_ms = 4;
        }

        // Step 7-9: realm, uniqueHandle, task (handled by global_scope).

        // Step 10: "Set task's timer nesting level to nesting level + 1."
        let task_nesting_level = nesting_level.saturating_add(1);

        // Step 11: scheduling.
        self.global_scope()
            .timer_initialization_steps(
                previous_id,
                handler,
                arguments,
                repeat,
                timeout_ms,
                task_nesting_level,
            )
            .map_err(|message| ec.new_type_error(&message))
    }
}

impl WindowOrWorkerGlobalScope for Window {
    fn global_scope(&self) -> &GlobalScope {
        &self.global_scope
    }
}

fn timer_handler(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<TimerHandler, crate::js::Types> {
    if let Some(object) = <crate::js::Types as JsTypes>::value_as_object(value) {
        if ec.is_callable(value) {
            return Ok(TimerHandler::Function {
                callback: Callback::from_object(object, ec),
            });
        }
    }

    let source = ec.to_rust_string(value.clone())?;
    Ok(TimerHandler::String { source })
}

fn timeout_ms(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<u32, crate::js::Types> {
    let timeout = ec.to_number(value.clone())?;
    if !timeout.is_finite() || timeout <= 0.0 {
        return Ok(0);
    }
    Ok(timeout.floor().min(i32::MAX as f64) as u32)
}
