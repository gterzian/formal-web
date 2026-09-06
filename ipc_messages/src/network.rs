use crate::content::{Command, DocumentFetchId, EventLoopId, FetchRequest, FetchResponse};
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
    /// <https://fetch.spec.whatwg.org/#concept-request-header-list>
    pub header_list: Vec<(String, String)>,
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
    /// Net sends Response on its persistent net→UA channel (the sender end
    /// of its own bootstrap connection). Modelled on the fetch spec's
    /// parallel queue: the UA channel is where fetch responses are
    /// delivered.
    /// <https://fetch.spec.whatwg.org/#fetch-useparallelqueue>
    UserAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    SetTraceSender(Option<TraceSender>),
    /// The URL schemes the embedder serves itself. A fetch whose URL has
    /// one of these schemes is answered by the embedder over the net→UA
    /// channel instead of by a network backend.
    SetEmbedderSchemes {
        schemes: Vec<String>,
    },
    Fetch {
        /// The network partition key of the fetch: the event loop id of
        /// the similar-origin window agent of the agent cluster (content
        /// process) that initiated it.  A dedicated worker shares its
        /// owner cluster's partition, so the id is the host window agent's
        /// event loop, never the worker agent's own.
        event_loop_id: EventLoopId,
        request_id: Uuid,
        request: FetchRequest,
        reply_to: ResponseRecipient,
    },
    NavigationFetch {
        /// The network partition key of the navigation fetch: the event
        /// loop id of the similar-origin window agent of the agent cluster
        /// (content process) that owns the navigable being navigated.
        event_loop_id: EventLoopId,
        request_id: Uuid,
        request: NavigationFetchRequest,
        reply_to: ResponseRecipient,
    },
    /// The embedder's answer to a [`Response::EmbedderSchemeFetch`]. Net
    /// routes it to the recipient the intercepted request named.
    CompleteEmbedderSchemeFetch {
        request_id: Uuid,
        result: Result<FetchResponse, String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// The outcome of a fetch the user agent started.
    Fetch {
        request_id: Uuid,
        result: Result<FetchResponse, String>,
    },
    /// A fetch whose URL scheme the embedder serves. The user agent asks
    /// the embedder for the response and sends it back as
    /// [`Request::CompleteEmbedderSchemeFetch`].
    EmbedderSchemeFetch {
        /// The event loop of the agent cluster that initiated the fetch.
        event_loop_id: EventLoopId,
        request_id: Uuid,
        request: FetchRequest,
    },
}
