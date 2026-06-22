mod managed_pipeline;

use gstreamer as gst;
use gstreamer::prelude::*;
use ::ipc_channel::ipc::{self as ipc_channel, IpcReceiver, IpcSender};
use ipc::{ExtensionEndpoint, ExtensionManifest};
use ::ipc_channel::router::RouterProxy;
use ipc_messages::media::{MediaCommand, MediaEvent, MediaPipelineId};
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

struct MediaExtensionManifest;

impl ExtensionManifest for MediaExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint {
        ExtensionEndpoint::Singleton {
            service_name: "formal-web.media",
        }
    }
}

/// Run the media process using the new IPC abstraction layer.
pub fn run_media_process_v2(token: String) -> Result<(), String> {
    let manifest = MediaExtensionManifest;
    let server = ipc::run_extension::<MediaExtensionManifest, MediaCommand, MediaEvent>(
        &manifest,
        &token,
        "formal-web.media",
    )
    .map_err(|error| format!("ipc extension bootstrap failed: {error}"))?;

    // Create legacy channels to bridge with the existing run_media_process function.
    let (legacy_cmd_tx, legacy_cmd_rx) = ipc_channel::channel::<MediaCommand>()
        .map_err(|error| format!("failed to create legacy command channel: {error}"))?;
    let (legacy_evt_tx, legacy_evt_rx) = ipc_channel::channel::<MediaEvent>()
        .map_err(|error| format!("failed to create legacy event channel: {error}"))?;

    // Bridge new command receiver -> legacy command sender
    let cmd_bridge = legacy_cmd_tx;
    std::thread::spawn(move || {
        while let Ok(incoming) = server.rx.recv() {
            if cmd_bridge.send(incoming.payload).is_err() {
                break;
            }
        }
    });

    // Bridge legacy event receiver -> new event sender
    std::thread::spawn(move || {
        while let Ok(event) = legacy_evt_rx.recv() {
            if server.tx.send(event).is_err() {
                break;
            }
        }
    });

    run_media_process(legacy_cmd_rx, legacy_evt_tx);
    Ok(())
}

pub fn run_media_process_from_args() -> Result<(), String> {
    let token = media_token_from_args()?;
    // If a token was provided (ipc-channel mode), use it.
    // Otherwise, use the native XPC backend (process launched by launchd).
    run_media_process_v2(token.unwrap_or_default())
}
