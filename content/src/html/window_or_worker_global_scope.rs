use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

use crate::html::{GlobalScope, TimerHandler, Window, WorkerGlobalScope};
use crate::webidl::Callback;

use crate::dom::DOMException;
use crate::html::structured_data::safe_passing_of_structured_data::{self, StructuredCloneOptions};
use crate::infra::{forgiving_base64_decode, forgiving_base64_encode};

/// <https://w3c.github.io/webappsec-secure-contexts/#is-origin-trustworthy>
fn is_origin_potentially_trustworthy(origin_url: &url::Url) -> bool {
    // Step 1: "If origin is an opaque origin, return "Not Trustworthy"."
    if origin_url.cannot_be_a_base() {
        return false;
    }

    // Step 2: "Assert: origin is a tuple origin."
    // Step 3: "If origin’s scheme is either "https" or "wss", return "Potentially Trustworthy"."
    if matches!(origin_url.scheme(), "https" | "wss") {
        return true;
    }

    // Step 4: "If origin’s host matches one of the CIDR notations 127.0.0.0/8 or ::1/128 [RFC4632], return "Potentially Trustworthy"."
    // Step 5: "If the user agent conforms to the name resolution rules in [let-localhost-be-localhost] and one of the following is true: origin’s host is "localhost" or "localhost."; origin’s host ends with ".localhost" or ".localhost."; then return "Potentially Trustworthy"."
    match origin_url.host() {
        Some(url::Host::Ipv4(address)) if address.octets()[0] == 127 => return true,
        Some(url::Host::Ipv6(address)) if address.is_loopback() => return true,
        Some(url::Host::Domain(domain)) => {
            let domain = domain.strip_suffix('.').unwrap_or(domain);
            if domain == "localhost" || domain.ends_with(".localhost") {
                return true;
            }
        }
        _ => {}
    }

    // Step 6: "If origin’s scheme is "file", return "Potentially Trustworthy"."
    if origin_url.scheme() == "file" {
        return true;
    }

    // Step 7: "If origin’s scheme component is one which the user agent considers to be authenticated, return "Potentially Trustworthy"."
    // Step 8: "If origin has been configured as a trustworthy origin, return "Potentially Trustworthy"."
    // Note: no scheme is considered authenticated and no origin is configured
    // as trustworthy.
    // Step 9: "Return "Not Trustworthy"."
    false
}

/// <https://html.spec.whatwg.org/#windoworworkerglobalscope>
pub(crate) trait WindowOrWorkerGlobalScope {
    fn global_scope(&self) -> &GlobalScope;

    /// <https://html.spec.whatwg.org/#dom-issecurecontext>
    fn is_secure_context(&self) -> bool {
        // "The isSecureContext getter steps are to return true if this's relevant settings object is a secure context, or false otherwise."
        // Note: the settings object is a secure context when its top-level
        // origin is potentially trustworthy; the creation URL's origin stands
        // in for the top-level origin.
        self.global_scope()
            .creation_url()
            .is_some_and(|creation_url| is_origin_potentially_trustworthy(&creation_url))
    }

    /// <https://html.spec.whatwg.org/#dom-btoa>
    fn btoa(&self, data: &str) -> Result<String, DOMException> {
        // "The btoa(data) method must throw an "InvalidCharacterError" DOMException if data contains any character whose code point is greater than U+00FF."
        let mut bytes = Vec::with_capacity(data.len());
        for code_point in data.chars() {
            let code_point = u32::from(code_point);
            if code_point > 0xFF {
                return Err(DOMException::invalid_character_error());
            }
            bytes.push(code_point as u8);
        }

        // "Otherwise, the user agent must convert data to a byte sequence whose nth byte is the eight-bit representation of the nth code point of data, and then must apply forgiving-base64 encode to that byte sequence and return the result."
        Ok(forgiving_base64_encode(&bytes))
    }

    /// <https://html.spec.whatwg.org/#dom-atob>
    fn atob(&self, data: &str) -> Result<String, DOMException> {
        // Step 1: "Let decodedData be the result of running forgiving-base64 decode on data."
        let decoded_data = forgiving_base64_decode(data);

        // Step 2: "If decodedData is failure, then throw an "InvalidCharacterError" DOMException."
        let Some(decoded_data) = decoded_data else {
            return Err(DOMException::invalid_character_error());
        };

        // Step 3: "Return decodedData."
        Ok(decoded_data.into_iter().map(char::from).collect())
    }

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
    fn clear_timeout(&self, timer_id: u32, ec: &mut dyn ExecutionContext<crate::js::Types>) {
        // Step 1: "Remove handle from this's map of setTimeout and setInterval IDs."
        self.global_scope().clear_timer(timer_id, ec);
    }

    /// <https://html.spec.whatwg.org/#dom-clearinterval>
    fn clear_interval(&self, timer_id: u32, ec: &mut dyn ExecutionContext<crate::js::Types>) {
        // Step 1: "Remove handle from this's map of setTimeout and setInterval IDs."
        self.global_scope().clear_timer(timer_id, ec);
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
                ec,
            )
            .map_err(|message| ec.new_type_error(&message))
    }
}

impl WindowOrWorkerGlobalScope for Window {
    fn global_scope(&self) -> &GlobalScope {
        &self.global_scope
    }
}

impl WindowOrWorkerGlobalScope for WorkerGlobalScope {
    fn global_scope(&self) -> &GlobalScope {
        &self.global_scope
    }
}

fn timer_handler(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<TimerHandler, crate::js::Types> {
    if let Some(object) = <crate::js::Types as JsTypes>::value_as_object(value)
        && ec.is_callable(value)
    {
        return Ok(TimerHandler::Function {
            callback: Callback::from_object(object, ec),
        });
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
