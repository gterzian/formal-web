use crate::content::{FetchRequest, FetchResponse};
use serde::{Deserialize, Serialize};
use verification::TraceSender;

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
