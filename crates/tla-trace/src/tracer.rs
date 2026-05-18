use ipc_channel::ipc::IpcSender;
use std::mem;

use crate::{LogEntry, VarUpdate};

#[derive(Debug, Clone)]
pub struct TLATracer {
    spec: String,
    producer: String,
    sender: Option<IpcSender<LogEntry>>,
    pending_updates: Vec<VarUpdate>,
}

impl TLATracer {
    pub fn new(
        spec: impl Into<String>,
        producer: impl Into<String>,
        sender: Option<IpcSender<LogEntry>>,
    ) -> Self {
        Self {
            spec: spec.into(),
            producer: producer.into(),
            sender,
            pending_updates: Vec::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.sender.is_some()
    }

    pub fn notify_change<P, A, PS, AS>(
        &mut self,
        variable: impl Into<String>,
        path: P,
        op: impl Into<String>,
        args: A,
    ) where
        P: IntoIterator<Item = PS>,
        A: IntoIterator<Item = AS>,
        PS: ToString,
        AS: ToString,
    {
        if self.sender.is_none() {
            return;
        }

        self.pending_updates.push(VarUpdate {
            variable: variable.into(),
            path: path.into_iter().map(|component| component.to_string()).collect(),
            op: op.into(),
            args: args.into_iter().map(|arg| arg.to_string()).collect(),
        });
    }

    pub fn log_with_location(&mut self, event: impl Into<String>, args: Vec<String>, file: &str, line: u32) {
        self.flush(Some(event.into()), args, file, line);
    }

    pub fn log_silent_with_location(&mut self, file: &str, line: u32) {
        self.flush(None, Vec::new(), file, line);
    }

    fn flush(&mut self, event: Option<String>, event_args: Vec<String>, file: &str, line: u32) {
        let Some(sender) = &self.sender else {
            return;
        };

        let entry = LogEntry {
            spec: self.spec.clone(),
            producer: self.producer.clone(),
            updates: mem::take(&mut self.pending_updates),
            event,
            event_args,
            source_file: file.to_owned(),
            source_line: line,
        };

        if let Err(error) = sender.send(entry) {
            eprintln!("tla trace send failed: {error}");
        }
    }
}
