use std::process::{Child, Command as ProcessCommand};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crossbeam_channel::Sender;
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_channel::router::ROUTER;
use ipc_messages::media::{
    MediaBootstrap, MediaCommand, MediaEvent, MediaPipelineId, VideoPaintId,
};
use log::error;

use crate::sidecar_executable_path;

/// Manages the dedicated media process and its pipelines.
#[allow(dead_code)]
pub(crate) struct MediaHandler {
    /// IPC sender for commands into the media process.
    command_sender: Option<IpcSender<MediaCommand>>,
    /// Child process handle.
    child: Option<Child>,
    /// Thread handle for the event-drain loop.
    drain_join_handle: Option<JoinHandle<()>>,
    /// Monotonic pipeline id counter.
    next_pipeline_id: AtomicU64,
    /// Monotonic paint id counter.
    next_paint_id: AtomicU64,
    /// Sender back into the user-agent command loop.
    user_agent_command_sender: Sender<super::UserAgentCommand>,
    /// Whether the media process has been started.
    started: bool,
}

#[allow(dead_code)]
impl MediaHandler {
    pub(crate) fn new(
        user_agent_command_sender: Sender<super::UserAgentCommand>,
    ) -> Self {
        Self {
            command_sender: None,
            child: None,
            drain_join_handle: None,
            next_pipeline_id: AtomicU64::new(1),
            next_paint_id: AtomicU64::new(1),
            user_agent_command_sender,
            started: false,
        }
    }

    fn ensure_started(&mut self) -> Result<(), String> {
        if self.started {
            return Ok(());
        }

        let executable_path = sidecar_executable_path("formal-web-media")?;

        let (server, token) = IpcOneShotServer::<MediaBootstrap>::new()
            .map_err(|error| format!("failed to create media IPC one-shot server: {error}"))?;

        let mut child_process = ProcessCommand::new(&executable_path);
        #[cfg(unix)]
        child_process.arg0("formal-web-media");
        child_process.arg("--media-token").arg(&token);

        let child = child_process.spawn().map_err(|error| {
            format!("failed to start media process: {error}")
        })?;

        let (_receiver, bootstrap) = server.accept().map_err(|error| {
            format!("failed to accept media bootstrap: {error}")
        })?;

        let command_sender = bootstrap.command_sender;
        let event_receiver = bootstrap.event_receiver;

        let ua_cmd_sender = self.user_agent_command_sender.clone();
        let (drain_sender, drain_receiver) = crossbeam_channel::unbounded::<MediaEvent>();
        ROUTER.add_typed_route::<MediaEvent>(
            event_receiver,
            Box::new(move |message| {
                match message {
                    Ok(event) => {
                        let _ = drain_sender.send(event);
                    }
                    Err(error) => {
                        error!("[media] failed to decode IPC event: {error}");
                    }
                }
            }),
        );

        let ua_cmd_sender_clone = ua_cmd_sender.clone();
        let drain_join_handle = thread::Builder::new()
            .name(String::from("formal-web:media-drain"))
            .spawn(move || {
                loop {
                    match drain_receiver.recv() {
                        Ok(event) => {
                            let cmd = super::UserAgentCommand::MediaEvent(event);
                            if ua_cmd_sender_clone.send(cmd).is_err() {
                                break; // UA gone
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .unwrap_or_else(|error| {
                panic!("failed to spawn media drain thread: {error}")
            });

        self.command_sender = Some(command_sender);
        self.child = Some(child);
        self.drain_join_handle = Some(drain_join_handle);
        self.started = true;

        Ok(())
    }

    pub(crate) fn allocate_ids(&mut self) -> (MediaPipelineId, VideoPaintId) {
        let pipeline_id = MediaPipelineId(self.next_pipeline_id.fetch_add(1, Ordering::Relaxed));
        let paint_id = VideoPaintId(self.next_paint_id.fetch_add(1, Ordering::Relaxed));
        (pipeline_id, paint_id)
    }

    pub(crate) fn send_command(&mut self, command: MediaCommand) -> Result<(), String> {
        self.ensure_started()?;
        let sender = self.command_sender.as_ref().ok_or_else(|| {
            String::from("media process not started")
        })?;
        sender.send(command).map_err(|error| {
            format!("failed to send media command: {error}")
        })
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(sender) = self.command_sender.take() {
            let _ = sender.send(MediaCommand::Shutdown);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
        if let Some(handle) = self.drain_join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MediaHandler {
    fn drop(&mut self) {
        self.shutdown();
    }
}
