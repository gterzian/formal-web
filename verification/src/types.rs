use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};

pub type TraceSender = IpcSender<LogEntry>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarUpdate {
    pub variable: String,
    pub path: Vec<String>,
    pub op: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub spec: String,
    pub producer: String,
    pub updates: Vec<VarUpdate>,
    pub event: Option<String>,
    pub event_args: Vec<String>,
    pub source_file: String,
    pub source_line: u32,
}