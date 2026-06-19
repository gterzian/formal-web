mod managed_pipeline;

use gstreamer as gst;
use gstreamer::prelude::*;
use ipc_channel::ipc::{self, IpcReceiver, IpcSender};
use ipc_channel::router::RouterProxy;
use ipc_messages::media::{MediaBootstrap, MediaCommand, MediaEvent, MediaPipelineId};
use managed_pipeline::ManagedPipeline;
use std::collections::HashMap;
use std::env;

pub fn run_media_process(
    cmd_receiver: IpcReceiver<MediaCommand>,
    event_sender: IpcSender<MediaEvent>,
) {
    // On macOS, ensure NSApplication is set up on the main thread before any
    // GStreamer GL elements are created. This avoids the "An NSApplication needs
    // to be running on the main thread" warning.
    #[cfg(target_os = "macos")]
    gst::macos_main(|| {});

    if gst::init().is_err() {
        log::error!("GStreamer initialization failed");
        return;
    }

    // Route the IPC command receiver through the router to get a crossbeam receiver.
    let router = RouterProxy::new();
    let cmd_rx = router.route_ipc_receiver_to_new_crossbeam_receiver(cmd_receiver);

    // Shared channel for GStreamer bus messages from all pipelines.
    // Each pipeline's sync handler sends (pipeline_id, message) here.
    let (bus_msg_sender, bus_msg_receiver) =
        crossbeam_channel::unbounded::<(MediaPipelineId, gst::Message)>();

    let mut pipelines: HashMap<MediaPipelineId, ManagedPipeline> = HashMap::new();

    loop {
        crossbeam_channel::select! {
            recv(cmd_rx) -> cmd => {
                match cmd {
                    Ok(command) => {
                        if handle_command(command, &mut pipelines, &event_sender, &bus_msg_sender) {
                            break; // Shutdown received
                        }
                    }
                    Err(_) => break, // command channel disconnected
                }
            }
            recv(bus_msg_receiver) -> msg => {
                match msg {
                    Ok((pipeline_id, bus_msg)) => {
                        handle_bus_message(&pipeline_id, &bus_msg, &pipelines, &event_sender);
                    }
                    Err(_) => {} // bus message channel disconnected (should not happen)
                }
            }
        }
    }

    // Clean up remaining pipelines on shutdown.
    for (_, pipeline) in pipelines.drain() {
        if let Err(error) = pipeline.element.set_state(gst::State::Null) {
            log::error!("failed to destroy pipeline during shutdown: {error}");
        }
    }
}

fn handle_bus_message(
    pipeline_id: &MediaPipelineId,
    msg: &gst::Message,
    pipelines: &HashMap<MediaPipelineId, ManagedPipeline>,
    event_sender: &IpcSender<MediaEvent>,
) {
    match msg.view() {
        gst::MessageView::Eos(..) => {
            // EOS does not destroy the pipeline; the user may want to replay or seek.
            let _ = event_sender.send(MediaEvent::Eos {
                pipeline_id: *pipeline_id,
            });
        }
        gst::MessageView::Error(error) => {
            let _ = event_sender.send(MediaEvent::Error {
                pipeline_id: *pipeline_id,
                message: error.error().to_string(),
            });
        }
        gst::MessageView::DurationChanged(..) => {
            if let Some(pipeline) = pipelines.get(pipeline_id) {
                if let Some(dur) = pipeline.element.query_duration::<gst::ClockTime>() {
                    let _ = event_sender.send(MediaEvent::DurationChanged {
                        pipeline_id: *pipeline_id,
                        duration_secs: dur.seconds_f64(),
                    });
                }
            }
        }
        _ => {}
    }
}

/// Returns `true` if the loop should exit (Shutdown received).
fn handle_command(
    cmd: MediaCommand,
    pipelines: &mut HashMap<MediaPipelineId, ManagedPipeline>,
    event_sender: &IpcSender<MediaEvent>,
    bus_msg_sender: &crossbeam_channel::Sender<(MediaPipelineId, gst::Message)>,
) -> bool {
    match cmd {
        MediaCommand::CreatePipeline { pipeline_id, url } => {
            log::info!("[media] creating pipeline id={:?} url={}", pipeline_id, url);
            match ManagedPipeline::new(
                pipeline_id,
                url,
                event_sender.clone(),
                bus_msg_sender.clone(),
            ) {
                Ok(p) => {
                    log::info!("[media] pipeline created id={:?}", pipeline_id);
                    pipelines.insert(pipeline_id, p);
                }
                Err(error) => {
                    log::error!("[media] failed to create media pipeline {pipeline_id:?}: {error}");
                    let _ = event_sender.send(MediaEvent::Error {
                        pipeline_id,
                        message: format!("pipeline creation failed: {error}"),
                    });
                }
            }
        }
        MediaCommand::Play { pipeline_id } => {
            log::info!("[media] playing pipeline id={:?}", pipeline_id);
            if let Some(p) = pipelines.get(&pipeline_id) {
                if let Err(error) = p.element.set_state(gst::State::Playing) {
                    log::error!("[media] failed to play pipeline {pipeline_id:?}: {error}");
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
            return true;
        }
    }
    false
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
    let token =
        media_token_from_args()?.ok_or_else(|| String::from("missing --media-token argument"))?;
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
