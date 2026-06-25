use crate::content::{FetchRequest, FetchResponse};
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
    /// Direct-response channel from content process.
    /// Net sends fetch responses directly to content instead of
    /// routing through the user agent.
    SetContentSender {
        response_sender: ipc::IpcSender<FetchResponse>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub request_id: u64,
    pub result: Result<FetchResponse, String>,
}
