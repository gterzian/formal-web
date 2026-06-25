// ── Fetch Standard domain model ────────────────────────────────────────────
//
// These types model the Fetch Standard state machine that the content process
// uses when implementing the Fetch API.  They live in the content crate because
// the Fetch Standard is a content-side concept — the net extension is a thin
// HTTP transport layer.
//
// Some items are marked `#[allow(dead_code)]` because they implement spec
// algorithms (abort, terminate, fetch-group management) that will be wired
// into the Fetch API implementation when that work is unblocked.

use serde::{Deserialize, Serialize};

/// <https://fetch.spec.whatwg.org/#concept-header-list>
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct HeaderList {
    /// <https://fetch.spec.whatwg.org/#concept-header-list>
    headers: Vec<(String, String)>,
}

#[allow(dead_code)]
impl HeaderList {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_content_type(content_type: &str) -> Self {
        // Note: The current content IPC `content_type` convenience field maps to a one-entry
        // header list. Full response headers belong in a later net/content IPC shape.
        if content_type.is_empty() {
            return Self::new();
        }

        Self {
            headers: vec![(String::from("content-type"), content_type.to_owned())],
        }
    }
}

/// <https://fetch.spec.whatwg.org/#concept-request>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct InternalFetchRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request-url>
    pub(crate) url: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-method>
    pub(crate) method: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-header-list>
    pub(crate) header_list: HeaderList,
    /// <https://fetch.spec.whatwg.org/#concept-request-body>
    pub(crate) body: String,
    /// <https://fetch.spec.whatwg.org/#done-flag>
    pub(crate) done: bool,
    /// <https://fetch.spec.whatwg.org/#request-keepalive-flag>
    pub(crate) keepalive: bool,
}

#[allow(dead_code)]
impl InternalFetchRequest {
    pub(crate) fn mark_done(&mut self) {
        self.done = true;
    }
}

/// <https://fetch.spec.whatwg.org/#concept-response>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct InternalFetchResponse {
    /// <https://fetch.spec.whatwg.org/#concept-response-url-list>
    pub(crate) url_list: Vec<String>,
    /// <https://fetch.spec.whatwg.org/#concept-response-status>
    pub(crate) status: u16,
    /// <https://fetch.spec.whatwg.org/#concept-response-header-list>
    pub(crate) header_list: HeaderList,
    // Note: The content IPC exposes `content_type` as a separate convenience
    // field alongside the spec-shaped `header_list`.  Both are preserved to round
    // trip the existing `FetchResponse` transport without changing behavior.
    pub(crate) content_type: String,
    /// <https://fetch.spec.whatwg.org/#concept-response-body>
    pub(crate) body: Vec<u8>,
}

/// <https://fetch.spec.whatwg.org/#fetch-controller-state>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) enum FetchControllerState {
    #[default]
    Ongoing,
    Terminated,
    Aborted,
}

/// <https://fetch.spec.whatwg.org/#fetch-controller>
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct FetchController {
    /// <https://fetch.spec.whatwg.org/#fetch-controller-state>
    pub(crate) state: FetchControllerState,
    /// <https://fetch.spec.whatwg.org/#fetch-controller-serialized-abort-reason>
    pub(crate) serialized_abort_reason: Option<String>,
}

#[allow(dead_code)]
impl FetchController {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// <https://fetch.spec.whatwg.org/#fetch-controller-abort>
    // TODO: Wire this to AbortSignal/controller integration once content can initiate aborts and
    // formal-web can carry structured abort reasons across content, user-agent, net.
    pub(crate) fn abort(&mut self, error: Option<String>) {
        // Step 1. Set controller's state to "aborted".
        self.state = FetchControllerState::Aborted;
        // Step 2. Let fallbackError be an "AbortError" DOMException.
        let fallback_error = String::from("AbortError");
        // Step 3. Set error to fallbackError if it is not given.
        let error = error.unwrap_or_else(|| fallback_error.clone());
        // Step 4. Let serializedError be StructuredSerialize(error).
        // TODO: Replace this placeholder with StructuredSerialize(error).
        // formal-web does not yet expose DOMException or structured clone values across
        // this worker boundary, so the serialized reason is stored as a string placeholder.
        // Step 5. Set controller's serialized abort reason to serializedError.
        self.serialized_abort_reason = Some(error);
    }

    /// <https://fetch.spec.whatwg.org/#fetch-controller-terminate>
    pub(crate) fn terminate(&mut self) {
        // Step 1. Set controller's state to "terminated".
        self.state = FetchControllerState::Terminated;
    }
}

/// <https://fetch.spec.whatwg.org/#fetch-params>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct FetchParams {
    /// <https://fetch.spec.whatwg.org/#fetch-params-request>
    pub(crate) request: InternalFetchRequest,
    /// <https://fetch.spec.whatwg.org/#fetch-params-controller>
    pub(crate) controller: FetchController,
}

#[allow(dead_code)]
impl FetchParams {
    pub(crate) fn new(request: InternalFetchRequest) -> Self {
        Self {
            request,
            controller: FetchController::new(),
        }
    }

    /// <https://fetch.spec.whatwg.org/#fetch-params-aborted>
    pub(crate) fn is_aborted(&self) -> bool {
        self.controller.state == FetchControllerState::Aborted
    }

    /// <https://fetch.spec.whatwg.org/#fetch-params-canceled>
    pub(crate) fn is_canceled(&self) -> bool {
        matches!(
            self.controller.state,
            FetchControllerState::Aborted | FetchControllerState::Terminated
        )
    }
}

/// <https://fetch.spec.whatwg.org/#deferred-fetch-record>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct DeferredFetchRecord;

/// <https://fetch.spec.whatwg.org/#concept-fetch-group>
// Note: A fetch group is associated with an environment settings object per the Fetch Standard.
// That ownership split will be introduced when content exposes environment-scoped Fetch API state.
#[derive(Debug, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct FetchGroup {
    /// <https://fetch.spec.whatwg.org/#concept-fetch-record>
    pub(crate) fetch_records: Vec<FetchRecord>,
    /// <https://fetch.spec.whatwg.org/#fetch-group-deferred-fetch-records>
    pub(crate) deferred_fetch_records: Vec<DeferredFetchRecord>,
}

#[allow(dead_code)]
impl FetchGroup {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push_fetch_record(&mut self, record: FetchRecord) {
        self.fetch_records.push(record);
    }

    /// <https://fetch.spec.whatwg.org/#concept-fetch-group-terminate>
    pub(crate) fn terminate(&mut self) {
        // Step 1. For each fetch record record of fetchGroup's fetch records, if record's
        // controller is non-null and record's request's done flag is unset and keepalive is
        // false, terminate record's controller.
        for record in &mut self.fetch_records {
            if !record.request.done && !record.request.keepalive {
                record.controller.terminate();
            }
        }
        // TODO: Step 2. Process deferred fetches for fetchGroup.
        let _has_deferred_fetch_records = !self.deferred_fetch_records.is_empty();
    }
}

/// <https://fetch.spec.whatwg.org/#fetch-record>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct FetchRecord {
    /// <https://fetch.spec.whatwg.org/#concept-fetch-record-request>
    pub(crate) request: InternalFetchRequest,
    /// <https://fetch.spec.whatwg.org/#concept-fetch-record-fetch>
    pub(crate) controller: FetchController,
}
