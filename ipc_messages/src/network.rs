use crate::content::{Command, DocumentFetchId, FetchRequest, FetchResponse};
use ipc::IpcSender;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use verification::TraceSender;

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
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub request_id: Uuid,
    pub result: Result<FetchResponse, String>,
}
