use crate::content::{FetchRequest, FetchResponse};
use ipc_channel::ipc::{IpcReceiver, IpcSender};
use serde::{Deserialize, Serialize};
use verification::TraceSender;

#[derive(Debug, Serialize, Deserialize)]
pub struct Bootstrap {
    pub request_sender: IpcSender<Request>,
    pub response_receiver: IpcReceiver<Response>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    SetTraceSender(Option<TraceSender>),
    Fetch {
        request_id: u64,
        request: FetchRequest,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub request_id: u64,
    pub result: Result<FetchResponse, String>,
}