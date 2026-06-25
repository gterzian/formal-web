use std::process::Child;
use std::thread;

use crossbeam_channel::{Receiver, Sender, select};

use ipc_messages::media::{MediaCommand as MediaProcessCommand, MediaEvent, MediaPipelineId};
use log::{debug, error};

use crate::UserAgentCommand;
use crate::ipc_manifest::MediaExtensionManifest;

/// Commands that the user-agent and event-loop workers can send into the dedicated media worker.
pub enum MediaCommand {
    CreatePipeline {
        pipeline_id: MediaPipelineId,
        url: String,
    },
    Play {
        pipeline_id: MediaPipelineId,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// Worker that owns the media process.
struct MediaWorker {
    /// Receiver for commands from the user-agent / event-loop workers.
    command_receiver: Receiver<MediaCommand>,
    /// Sender back into the user-agent thread for non-frame media events.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// IPC sender to the dedicated media process.
    media_process_sender: ipc::IpcSender<MediaProcessCommand>,
    /// Crossbeam receiver for media process events.
    media_event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<MediaEvent>>,
    /// Child process handle for the media process.
    child: Option<Child>,
    /// Deferred shutdown reply completed after the media process exits.
    shutdown_reply: Option<Sender<Result<(), String>>>,
}

/// Bootstrap the dedicated media process using the new IPC abstraction layer.
fn start_media_extension() -> Result<
    (
        ipc::IpcSender<MediaProcessCommand>,
        crossbeam_channel::Receiver<ipc::IpcIncoming<MediaEvent>>,
        Option<Child>,
    ),
    String,
> {
    let manifest = MediaExtensionManifest;
    let (mut handle, connection) =
        ipc::ExtensionHandle::launch::<MediaExtensionManifest, MediaProcessCommand, MediaEvent>(
            &manifest,
        )
        .map_err(|error| format!("failed to start media extension: {error}"))?;

    let sender = connection.sender.clone();
    let receiver = connection.receiver;
    let child = handle.take_child();
    Ok((sender, ipc::crossbeam_proxy(receiver), child))
}

impl MediaWorker {
    fn new(
        command_receiver: Receiver<MediaCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
    ) -> Result<Self, String> {
        let (media_process_sender, media_event_receiver, child) = start_media_extension()?;
        Ok(Self {
            command_receiver,
            user_agent_command_sender,
            media_process_sender,
            media_event_receiver,
            child,
            shutdown_reply: None,
        })
    }

    fn handle_command(&mut self, command: MediaCommand) {
        match command {
            MediaCommand::CreatePipeline { pipeline_id, url } => {
                debug!(
                    "[media] media worker forwarding CreatePipeline id={:?} url={}",
                    pipeline_id, url
                );
                if let Err(error) = self
                    .media_process_sender
                    .send(MediaProcessCommand::CreatePipeline { pipeline_id, url })
                {
                    error!("[media] failed to send CreatePipeline: {error}");
                }
            }
            MediaCommand::Play { pipeline_id } => {
                debug!("[media] media worker forwarding Play id={:?}", pipeline_id);
                if let Err(error) = self
                    .media_process_sender
                    .send(MediaProcessCommand::Play { pipeline_id })
                {
                    error!("[media] failed to send Play: {error}");
                }
            }

            MediaCommand::Shutdown { reply } => {
                self.shutdown_reply = Some(reply);
                let _ = self
                    .media_process_sender
                    .send(MediaProcessCommand::Shutdown);
            }
        }
    }

    fn handle_media_event(&mut self, event: MediaEvent) {
        let cmd = UserAgentCommand::MediaEvent(event);
        if self.user_agent_command_sender.send(cmd).is_err() {
            // UA gone — the worker loop will break on the next recv() error.
        }
    }

    fn run(&mut self) {
        loop {
            let command_receiver = &self.command_receiver;
            let media_event_receiver = &self.media_event_receiver;
            select! {
                recv(command_receiver) -> command => {
                    let Ok(command) = command else {
                        break;
                    };
                    let shutting_down = matches!(command, MediaCommand::Shutdown { .. });
                    self.handle_command(command);
                    if shutting_down {
                        break;
                    }
                }
                recv(media_event_receiver) -> event => {
                    match event {
                        Ok(mut incoming) => {
                            match &mut incoming.payload {
                                MediaEvent::Frame(video_frame) => {
                                    if let Some(region) =
                                        incoming.shmem_regions.get(&0)
                                    {
                                        video_frame.data = region.as_slice().to_vec();
                                    }
                                }
                                _ => {
                                    if !incoming.shmem_regions.is_empty() {
                                        log::warn!(
                                            "unexpected shared memory on {:?}",
                                            std::mem::discriminant(&incoming.payload)
                                        );
                                    }
                                }
                            }
                            self.handle_media_event(incoming.payload);
                        }
                        Err(_) => {
                            error!("[media] event route closed");
                            break;
                        }
                    }
                }
            }
        }

        // Clean up the media process.
        if let Some(mut child) = self.child.take() {
            // Give the media process a moment to exit after Shutdown.
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if std::time::Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => {
                        error!("[media] failed to poll child exit: {error}");
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }

        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.send(Ok(()));
        }
    }
}

/// Spawn the dedicated media worker thread owned by `UserAgentWorker`.
pub fn run_media_thread(
    command_receiver: Receiver<MediaCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
) {
    let mut worker = match MediaWorker::new(command_receiver, user_agent_command_sender) {
        Ok(worker) => worker,
        Err(error) => {
            error!("[media] worker startup failed: {error}");
            return;
        }
    };
    worker.run();
}
