use std::mem;

use crate::{LogEntry, TraceSender, VarUpdate};

#[derive(Debug, Clone)]
pub struct TLATracer {
    spec: String,
    producer: String,
    sender: Option<TraceSender>,
    pending_updates: Vec<VarUpdate>,
}

impl TLATracer {
    pub fn new(
        spec: impl Into<String>,
        producer: impl Into<String>,
        sender: Option<TraceSender>,
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

    pub fn set_sender(&mut self, sender: Option<TraceSender>) {
        self.sender = sender;
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
            path: path
                .into_iter()
                .map(|component| component.to_string())
                .collect(),
            op: op.into(),
            args: args.into_iter().map(|arg| arg.to_string()).collect(),
        });
    }

    pub fn log_with_location(
        &mut self,
        event: impl Into<String>,
        args: Vec<String>,
        file: &str,
        line: u32,
    ) {
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
            eprintln!("verification trace send failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use ipc_channel::ipc;

    use super::TLATracer;

    #[test]
    fn disabled_tracer_drops_updates_and_events() {
        let mut tracer = TLATracer::new("Navigation", "test", None);

        tracer.notify_change("navigations", ["root"], "set", ["started"]);
        crate::tla_log!(tracer, "CreateNavigation", "nk-1", "nav-1");

        assert!(tracer.pending_updates.is_empty());
        assert!(!tracer.is_enabled());
    }

    #[test]
    fn enabled_tracer_sends_logged_entry() {
        let (sender, receiver) =
            ipc::channel().expect("failed to create verification test channel");
        let mut tracer = TLATracer::new("Navigation", "test", Some(sender));

        tracer.notify_change("navigations", ["root"], "set", ["started"]);
        crate::tla_log!(tracer, "CreateNavigation", "nk-1", "nav-1");

        let entry = receiver.recv().expect("expected one trace entry");
        assert_eq!(entry.spec, "Navigation");
        assert_eq!(entry.producer, "test");
        assert_eq!(entry.event.as_deref(), Some("CreateNavigation"));
        assert_eq!(
            entry.event_args,
            vec![String::from("nk-1"), String::from("nav-1")]
        );
        assert_eq!(entry.updates.len(), 1);
    }
}
