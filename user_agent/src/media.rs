use std::process::{Child, Command as ProcessCommand};
use std::thread;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crossbeam_channel::{Receiver, Sender, select, unbounded};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_channel::router::ROUTER;
use ipc_messages::media::{
    MediaBootstrap, MediaEvent, MediaPipelineId, MediaCommand as MediaProcessCommand,
};
use log::error;

use crate::sidecar_executable_path;
use crate::UserAgentCommand;

/// Commands that the user-agent and event-loop workers can send into the dedicated media worker.
pub enum MediaCommand {
    #[allow(dead_code)]
    CreatePipeline {
        pipeline_id: MediaPipelineId,
        url: String,
    },
    #[allow(dead_code)]
    Play {
        pipeline_id: MediaPipelineId,
    },
    #[allow(dead_code)]
    Pause {
        pipeline_id: MediaPipelineId,
    },
    #[allow(dead_code)]
    Seek {
        pipeline_id: MediaPipelineId,
        position_secs: f64,
    },
    #[allow(dead_code)]
    Destroy {
        pipeline_id: MediaPipelineId,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// Worker that owns the dedicated media process and its event loop.
struct MediaWorker {
    /// Receiver for commands from the user-agent / event-loop workers.
    command_receiver: Receiver<MediaCommand>,
    /// Sender back into the user-agent thread for non-frame media events.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// IPC sender to the dedicated media process.
    media_process_sender: IpcSender<MediaProcessCommand>,
    /// Crossbeam receiver for media process events routed via the IPC router.
    media_event_receiver: Receiver<Result<MediaEvent, String>>,
    /// Child process handle for the media sidecar.
    child: Option<Child>,
    /// Deferred shutdown reply completed after the media process exits.
    shutdown_reply: Option<Sender<Result<(), String>>>,
    /// Monotonic pipeline id counter.
    #[allow(dead_code)]
    next_pipeline_id: u64,
    /// Monotonic paint id counter.
    #[allow(dead_code)]
    next_paint_id: u64,
}

/// Bootstrap the dedicated media sidecar process.
fn start_media_process() -> Result<
    (
        IpcSender<MediaProcessCommand>,
        Receiver<Result<MediaEvent, String>>,
        Child,
    ),
    String,
> {
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

    let (event_sender, event_receiver) = unbounded();
    ROUTER.add_typed_route::<MediaEvent>(
        bootstrap.event_receiver,
        Box::new(move |message| {
            let _ = event_sender.send(message.map_err(|error| {
                format!("failed to decode media IPC event: {error}")
            }));
        }),
    );

    Ok((bootstrap.command_sender, event_receiver, child))
}

impl MediaWorker {
    fn new(
        command_receiver: Receiver<MediaCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
    ) -> Result<Self, String> {
        let (media_process_sender, media_event_receiver, child) = start_media_process()?;
        Ok(Self {
            command_receiver,
            user_agent_command_sender,
            media_process_sender,
            media_event_receiver,
            child: Some(child),
            shutdown_reply: None,
            next_pipeline_id: 1,
            next_paint_id: 1,
        })
    }

    fn handle_command(&mut self, command: MediaCommand) {
        match command {
            MediaCommand::CreatePipeline { pipeline_id, url } => {
                if let Err(error) = self.media_process_sender
                    .send(MediaProcessCommand::CreatePipeline { pipeline_id, url })
                {
                    error!("[media] failed to send CreatePipeline: {error}");
                }
            }
            MediaCommand::Play { pipeline_id } => {
                if let Err(error) = self.media_process_sender
                    .send(MediaProcessCommand::Play { pipeline_id })
                {
                    error!("[media] failed to send Play: {error}");
                }
            }
            MediaCommand::Pause { pipeline_id } => {
                if let Err(error) = self.media_process_sender
                    .send(MediaProcessCommand::Pause { pipeline_id })
                {
                    error!("[media] failed to send Pause: {error}");
                }
            }
            MediaCommand::Seek { pipeline_id, position_secs } => {
                if let Err(error) = self.media_process_sender
                    .send(MediaProcessCommand::Seek { pipeline_id, position_secs })
                {
                    error!("[media] failed to send Seek: {error}");
                }
            }
            MediaCommand::Destroy { pipeline_id } => {
                if let Err(error) = self.media_process_sender
                    .send(MediaProcessCommand::Destroy { pipeline_id })
                {
                    error!("[media] failed to send Destroy: {error}");
                }
            }
            MediaCommand::Shutdown { reply } => {
                self.shutdown_reply = Some(reply);
                let _ = self.media_process_sender.send(MediaProcessCommand::Shutdown);
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
                    let event = match event {
                        Ok(Ok(event)) => event,
                        Ok(Err(error)) => {
                            error!("[media] process event error: {error}");
                            break;
                        }
                        Err(error) => {
                            error!("[media] event route closed: {error}");
                            break;
                        }
                    };
                    self.handle_media_event(event);
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
