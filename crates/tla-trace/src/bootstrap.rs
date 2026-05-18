use ipc_channel::ipc::{self, IpcOneShotServer, IpcSender};
use std::thread;

use crate::LogEntry;

pub fn receive_monitor_sender(
    server_name: Option<&str>,
) -> Result<Option<IpcSender<LogEntry>>, String> {
    let Some(server_name) = server_name else {
        return Ok(None);
    };

    let (reply_sender, reply_receiver) = ipc::channel::<Option<IpcSender<LogEntry>>>()
        .map_err(|error| format!("failed to create TLA log bootstrap reply channel: {error}"))?;
    let bootstrap =
        IpcSender::<IpcSender<Option<IpcSender<LogEntry>>>>::connect(server_name.to_owned())
            .map_err(|error| {
                format!(
                    "failed to connect to TLA log bootstrap server {server_name}: {error}"
                )
            })?;
    bootstrap.send(reply_sender).map_err(|error| {
        format!("failed to send TLA log bootstrap reply sender to {server_name}: {error}")
    })?;
    reply_receiver
        .recv()
        .map_err(|error| format!("failed to receive TLA monitor sender from {server_name}: {error}"))
}

pub fn spawn_monitor_sender_bridge(
    server: IpcOneShotServer<IpcSender<Option<IpcSender<LogEntry>>>>,
    monitor_tx: Option<IpcSender<LogEntry>>,
    thread_name: impl Into<String>,
    consumer_label: impl Into<String>,
) -> Result<(), String> {
    let thread_name = thread_name.into();
    let consumer_label = consumer_label.into();
    let thread_consumer_label = consumer_label.clone();
    thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let reply_sender = match server.accept() {
                Ok((_receiver, reply_sender)) => reply_sender,
                Err(error) => {
                    eprintln!(
                        "failed to accept TLA log bootstrap for {}: {}",
                        thread_consumer_label, error
                    );
                    return;
                }
            };

            if let Err(error) = reply_sender.send(monitor_tx) {
                eprintln!(
                    "failed to send TLA monitor sender to {}: {}",
                    thread_consumer_label, error
                );
            }
        })
        .map_err(|error| {
            format!(
                "failed to spawn TLA log bootstrap thread for {}: {error}",
                consumer_label
            )
        })?;
    Ok(())
}
