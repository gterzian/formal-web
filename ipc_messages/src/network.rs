use crate::content::{Command, DocumentFetchId, FetchRequest, FetchResponse};
use ipc::IpcSender;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use verification::TraceSender;

/// A navigation fetch request initiated by the user agent.
/// Distinct from content-initiated document fetches (FetchRequest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationFetchRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request-url>
    pub url: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-method>
    pub method: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-body>
    pub body: Option<String>,
    /// <https://fetch.spec.whatwg.org/#concept-request-referrer>
    pub referrer: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-referrer-policy>
    pub referrer_policy: String,
}

/// Specifies how net should route the fetch response back to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseRecipient {
    /// Net sends Command::CompleteDocumentFetch to the content process's command sender.
    ContentProcess {
        content_command_sender: IpcSender<Command>,
        handler_id: DocumentFetchId,
    },
    /// Net sends Response through the bidirectional IPC response channel
    /// (the user agent's event loop receives on this channel).
    UserAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    SetTraceSender(Option<TraceSender>),
    Fetch {
        request_id: Uuid,
        request: FetchRequest,
        reply_to: ResponseRecipient,
    },
    NavigationFetch {
        request_id: Uuid,
        request: NavigationFetchRequest,
        reply_to: ResponseRecipient,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub request_id: Uuid,
    pub result: Result<FetchResponse, String>,
}
