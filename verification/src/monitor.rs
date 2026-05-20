use ipc_channel::ipc::{self, IpcReceiver};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use crate::{LogEntry, TraceSender};

pub struct TraceMonitor {
    sender: TraceSender,
    join_handle: JoinHandle<Result<(), String>>,
}

impl TraceMonitor {
    pub fn start(output_dir: impl Into<PathBuf>) -> Result<Self, String> {
        let output_dir = output_dir.into();
        prepare_output_dir(&output_dir)?;
        let (sender, receiver) = ipc::channel::<LogEntry>()
            .map_err(|error| format!("failed to create verification trace channel: {error}"))?;
        let join_handle = std::thread::Builder::new()
            .name(String::from("formal-web:verification-monitor"))
            .spawn(move || TraceLogWriter::new(output_dir, receiver).run())
            .map_err(|error| format!("failed to spawn verification monitor thread: {error}"))?;
        Ok(Self {
            sender,
            join_handle,
        })
    }

    pub fn sender_clone(&self) -> TraceSender {
        self.sender.clone()
    }

    pub fn shutdown(self) -> Result<(), String> {
        let Self {
            sender,
            join_handle,
        } = self;
        drop(sender);
        join_handle
            .join()
            .map_err(|_| String::from("verification monitor thread panicked"))?
    }
}

struct TraceLogWriter {
    output_dir: PathBuf,
    receiver: IpcReceiver<LogEntry>,
    next_clock: u64,
    writers: HashMap<String, BufWriter<File>>,
}

impl TraceLogWriter {
    fn new(output_dir: PathBuf, receiver: IpcReceiver<LogEntry>) -> Self {
        Self {
            output_dir,
            receiver,
            next_clock: 0,
            writers: HashMap::new(),
        }
    }

    fn run(mut self) -> Result<(), String> {
        loop {
            let entry = match self.receiver.recv() {
                Ok(entry) => entry,
                Err(_) => break,
            };
            self.write_entry(entry)?;
        }

        for writer in self.writers.values_mut() {
            writer
                .flush()
                .map_err(|error| format!("failed to flush verification log file: {error}"))?;
        }
        Ok(())
    }

    fn writer_for_spec(&mut self, spec: &str) -> Result<&mut BufWriter<File>, String> {
        if !self.writers.contains_key(spec) {
            let file_path = log_file_path(&self.output_dir, spec);
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)
                .map_err(|error| {
                    format!(
                        "failed to open verification log file {}: {error}",
                        file_path.display()
                    )
                })?;
            self.writers.insert(spec.to_owned(), BufWriter::new(file));
        }

        self.writers
            .get_mut(spec)
            .ok_or_else(|| format!("missing verification log writer for spec {spec}"))
    }

    fn write_entry(&mut self, entry: LogEntry) -> Result<(), String> {
        self.next_clock += 1;
        let clock = self.next_clock;
        let writer = self.writer_for_spec(&entry.spec)?;
        let json_entry = monitored_entry_json(clock, entry);
        serde_json::to_writer(&mut *writer, &json_entry)
            .map_err(|error| format!("failed to serialize trace entry: {error}"))?;
        writer
            .write_all(b"\n")
            .map_err(|error| format!("failed to append trace newline: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("failed to flush trace entry: {error}"))
    }
}

fn prepare_output_dir(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to clear verification log directory {}: {error}",
                path.display()
            ));
        }
    }

    fs::create_dir_all(path).map_err(|error| {
        format!(
            "failed to create verification log directory {}: {error}",
            path.display()
        )
    })
}

fn log_file_path(output_dir: &Path, spec: &str) -> PathBuf {
    output_dir.join(format!("{spec}.ndjson"))
}

fn monitored_entry_json(clock: u64, entry: LogEntry) -> Value {
    let mut object = Map::new();
    object.insert(String::from("clock"), json!(clock));
    object.insert(String::from("spec"), Value::String(entry.spec.clone()));
    object.insert(String::from("producer"), Value::String(entry.producer));

    let mut grouped_updates: HashMap<String, Vec<Value>> = HashMap::new();
    for update in entry.updates {
        grouped_updates
            .entry(update.variable)
            .or_default()
            .push(json!({
                "op": update.op,
                "path": update.path,
                "args": update.args,
            }));
    }

    for (variable, updates) in grouped_updates {
        object.insert(variable, Value::Array(updates));
    }

    if let Some(event) = entry.event {
        object.insert(String::from("event"), Value::String(event));
    }
    object.insert(String::from("event_args"), json!(entry.event_args));
    object.insert(String::from("source_file"), Value::String(entry.source_file));
    object.insert(String::from("source_line"), json!(entry.source_line));

    Value::Object(object)
}