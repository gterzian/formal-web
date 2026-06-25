use crate::content::{Command, FetchRequest, FetchResponse};
use ipc::IpcSender;
use serde::{Deserialize, Serialize};
use verification::TraceSender;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    SetTraceSender(Option<TraceSender>),
    Fetch {
        request_id: u64,
        request: FetchRequest,
    },
    /// Sender for sending commands directly to a content process.
    /// Net sends Command::CompleteDocumentFetch directly to content.
    SetContentSender {
        sender: ipc::IpcSender<Command>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub request_id: u64,
    pub result: Result<FetchResponse, String>,
}
