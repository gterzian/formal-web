mod managed_pipeline;

use gstreamer as gst;
use gstreamer::prelude::*;
use ipc_channel::ipc::{self, IpcReceiver, IpcSender};
use ipc_messages::media::{MediaBootstrap, MediaCommand, MediaEvent, MediaPipelineId};
use managed_pipeline::ManagedPipeline;
use std::collections::HashMap;
use std::env;

pub fn run_media_process(
    cmd_receiver: IpcReceiver<MediaCommand>,
    event_sender: IpcSender<MediaEvent>,
) {
    if gst::init().is_err() {
        log::error!("GStreamer initialization failed");
        return;
    }

    let mut pipelines: HashMap<MediaPipelineId, ManagedPipeline> = HashMap::new();

    loop {
        // Drain incoming commands (non-blocking).
        loop {
            match cmd_receiver.try_recv() {
                Ok(cmd) => {
                    handle_command(cmd, &mut pipelines, &event_sender);
                }
                Err(_) => break, // empty or disconnected
            }
        }
        // Check if the sender has disconnected (user agent shut down).
        if cmd_receiver.try_recv().is_ok() || cmd_receiver.try_recv().is_err() {
            // We got an Ok or an Err - if Ok, we already broke. If Err, also not a disconnection
            // signal from ipc-channel. Instead, check by blocking recv returning Err.
        }

        // Poll GStreamer bus for each pipeline.
        pipelines.retain(|_id, pipeline| {
            while let Some(msg) = pipeline.bus.pop() {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        let _ = event_sender.send(MediaEvent::Eos {
                            pipeline_id: pipeline.id,
                        });
                        pipeline.element.set_state(gst::State::Null).ok();
                        return false; // drop pipeline
                    }
                    gst::MessageView::Error(error) => {
                        let _ = event_sender.send(MediaEvent::Error {
                            pipeline_id: pipeline.id,
                            message: error.error().to_string(),
                        });
                        pipeline.element.set_state(gst::State::Null).ok();
                        return false;
                    }
                    gst::MessageView::DurationChanged(..) => {
                        if let Some(dur) = pipeline.element.query_duration::<gst::ClockTime>() {
                            let _ = event_sender.send(MediaEvent::DurationChanged {
                                pipeline_id: pipeline.id,
                                duration_secs: dur.seconds_f64(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            true
        });

        // Yield briefly to avoid busy-looping at 100% CPU.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn handle_command(
    cmd: MediaCommand,
    pipelines: &mut HashMap<MediaPipelineId, ManagedPipeline>,
    event_sender: &IpcSender<MediaEvent>,
) {
    match cmd {
        MediaCommand::CreatePipeline {
            pipeline_id,
            url,
        } => {
            match ManagedPipeline::new(pipeline_id, url, event_sender.clone()) {
                Ok(p) => {
                    pipelines.insert(pipeline_id, p);
                }
                Err(error) => {
                    log::error!("failed to create media pipeline {pipeline_id:?}: {error}");
                    let _ = event_sender.send(MediaEvent::Error {
                        pipeline_id,
                        message: format!("pipeline creation failed: {error}"),
                    });
                }
            }
        }
        MediaCommand::Play { pipeline_id } => {
            if let Some(p) = pipelines.get(&pipeline_id) {
                if let Err(error) = p.element.set_state(gst::State::Playing) {
                    log::error!("failed to play pipeline {pipeline_id:?}: {error}");
                }
            }
        }
        MediaCommand::Pause { pipeline_id } => {
            if let Some(p) = pipelines.get(&pipeline_id) {
                if let Err(error) = p.element.set_state(gst::State::Paused) {
                    log::error!("failed to pause pipeline {pipeline_id:?}: {error}");
                }
            }
        }
        MediaCommand::Seek {
            pipeline_id,
            position_secs,
        } => {
            if let Some(p) = pipelines.get(&pipeline_id) {
                let pos = gst::ClockTime::from_seconds_f64(position_secs);
                if let Err(error) = p
                    .element
                    .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, pos)
                {
                    log::error!("failed to seek pipeline {pipeline_id:?}: {error}");
                }
            }
        }
        MediaCommand::Destroy { pipeline_id } => {
            if let Some(p) = pipelines.remove(&pipeline_id) {
                if let Err(error) = p.element.set_state(gst::State::Null) {
                    log::error!("failed to destroy pipeline {pipeline_id:?}: {error}");
                }
            }
        }
        MediaCommand::Shutdown => {
            for (_, pipeline) in pipelines.drain() {
                let _ = pipeline.element.set_state(gst::State::Null);
            }
        }
    }
}

fn media_token_from_args() -> Result<Option<String>, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--media-token" {
            return args
                .next()
                .map(Some)
                .ok_or_else(|| String::from("missing media token value"));
        }
    }
    Ok(None)
}

pub fn run_media_process_from_args() -> Result<(), String> {
    let token = media_token_from_args()?
        .ok_or_else(|| String::from("missing --media-token argument"))?;
    run_media_process_with_token(token)
}

pub fn run_media_process_with_token(token: String) -> Result<(), String> {
    let (command_sender, command_receiver) =
        ipc::channel::<MediaCommand>().map_err(|error| error.to_string())?;
    let (event_sender, event_receiver) =
        ipc::channel::<MediaEvent>().map_err(|error| error.to_string())?;
    let bootstrap = IpcSender::<MediaBootstrap>::connect(token)
        .map_err(|error| format!("failed to connect media bootstrap: {error}"))?;
    bootstrap
        .send(MediaBootstrap {
            command_sender,
            event_receiver,
        })
        .map_err(|error| format!("failed to send media bootstrap: {error}"))?;
    run_media_process(command_receiver, event_sender);
    Ok(())
}
