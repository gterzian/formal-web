pub mod backend;

use backend::{BackendEvent, MediaBackend, PipelineHandle};
use ipc::ExtensionEndpoint;
use ipc_messages::media::{MediaCommand, MediaEvent, MediaPipelineId};
use std::collections::HashMap;
use std::env;

// ---------------------------------------------------------------------------
// IPC manifest
// ---------------------------------------------------------------------------

struct MediaExtensionManifest;

impl ipc::ExtensionManifest for MediaExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint {
        ExtensionEndpoint::Singleton {
            service_name: "formal-web.media",
        }
    }
}

// ---------------------------------------------------------------------------
// Generic run loop
// ---------------------------------------------------------------------------

pub fn run_media_process<B: MediaBackend>(
    mut backend: B,
    cmd_rx: crossbeam_channel::Receiver<ipc::IpcIncoming<MediaCommand>>,
    frame_tx: crossbeam_channel::Sender<MediaEvent>,
    frame_rx: crossbeam_channel::Receiver<MediaEvent>,
    ipc_event_tx: ipc::IpcSender<MediaEvent>,
) {
    let backend_event_rx = backend.event_receiver();
    let mut pipelines: HashMap<MediaPipelineId, B::Pipeline> = HashMap::new();

    loop {
        crossbeam_channel::select! {
            recv(cmd_rx) -> cmd => {
                match cmd {
                    Ok(incoming) => {
                        if handle_command(
                            incoming.payload,
                            &mut backend,
                            &mut pipelines,
                            &frame_tx,
                        ) {
                            break; // Shutdown received
                        }
                    }
                    Err(_) => break, // command channel disconnected
                }
            }
            recv(backend_event_rx) -> event => {
                if let Ok(backend_event) = event {
                    handle_backend_event(backend_event, &frame_tx);
                }
            }
            recv(frame_rx) -> event => {
                let Ok(event) = event else { break; };
                match event {
                    MediaEvent::Frame(mut video_frame) => {
                        let data = std::mem::take(&mut video_frame.data);
                        let mut shmem_map = std::collections::HashMap::new();
                        shmem_map.insert(0, ipc::IpcSharedRegion::from_bytes(&data));
                        if ipc_event_tx
                            .send_with_shmem_map(MediaEvent::Frame(video_frame), shmem_map)
                            .is_err()
                        {
                            break;
                        }
                    }
                    other_event => {
                        if ipc_event_tx.send(other_event).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }

    // Clean up remaining pipelines on shutdown.
    for (_, pipeline) in pipelines.drain() {
        if let Err(error) = pipeline.destroy() {
            log::error!("failed to destroy pipeline during shutdown: {error}");
        }
    }
}

// ---------------------------------------------------------------------------
// Backend event dispatch
// ---------------------------------------------------------------------------

fn handle_backend_event(event: BackendEvent, event_tx: &crossbeam_channel::Sender<MediaEvent>) {
    match event {
        BackendEvent::Eos { pipeline_id } => {
            let _ = event_tx.send(MediaEvent::Eos { pipeline_id });
        }
        BackendEvent::Error {
            pipeline_id,
            message,
        } => {
            let _ = event_tx.send(MediaEvent::Error {
                pipeline_id,
                message,
            });
        }
        BackendEvent::DurationChanged {
            pipeline_id,
            duration_secs,
        } => {
            let _ = event_tx.send(MediaEvent::DurationChanged {
                pipeline_id,
                duration_secs,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

/// Returns `true` if the loop should exit (Shutdown received).
fn handle_command<B: MediaBackend>(
    cmd: MediaCommand,
    backend: &mut B,
    pipelines: &mut HashMap<MediaPipelineId, B::Pipeline>,
    event_tx: &crossbeam_channel::Sender<MediaEvent>,
) -> bool {
    match cmd {
        MediaCommand::CreatePipeline { pipeline_id, url } => {
            log::info!("[media] creating pipeline id={:?} url={}", pipeline_id, url);
            match backend.create_pipeline(pipeline_id, url, event_tx.clone()) {
                Ok(pipeline) => {
                    log::info!("[media] pipeline created id={:?}", pipeline_id);
                    pipelines.insert(pipeline_id, pipeline);
                }
                Err(error) => {
                    log::error!("[media] failed to create media pipeline {pipeline_id:?}: {error}");
                    let _ = event_tx.send(MediaEvent::Error {
                        pipeline_id,
                        message: format!("pipeline creation failed: {error}"),
                    });
                }
            }
        }
        MediaCommand::Play { pipeline_id } => {
            log::info!("[media] playing pipeline id={:?}", pipeline_id);
            if let Some(pipeline) = pipelines.get(&pipeline_id)
                && let Err(error) = pipeline.play()
            {
                log::error!("[media] failed to play pipeline {pipeline_id:?}: {error}");
            }
        }
        MediaCommand::Pause { pipeline_id } => {
            if let Some(pipeline) = pipelines.get(&pipeline_id)
                && let Err(error) = pipeline.pause()
            {
                log::error!("failed to pause pipeline {pipeline_id:?}: {error}");
            }
        }
        MediaCommand::Seek {
            pipeline_id,
            position_secs,
        } => {
            if let Some(pipeline) = pipelines.get(&pipeline_id)
                && let Err(error) = pipeline.seek(position_secs)
            {
                log::error!("failed to seek pipeline {pipeline_id:?}: {error}");
            }
        }
        MediaCommand::Destroy { pipeline_id } => {
            if let Some(pipeline) = pipelines.remove(&pipeline_id)
                && let Err(error) = pipeline.destroy()
            {
                log::error!("failed to destroy pipeline {pipeline_id:?}: {error}");
            }
        }
        MediaCommand::Shutdown => {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Entry point — select backend at compile time via Cargo features
// ---------------------------------------------------------------------------

pub fn run_media_process_from_args() -> Result<(), String> {
    let token = media_token_from_args()?;
    let manifest = MediaExtensionManifest;
    let server = ipc::run_extension::<MediaExtensionManifest, MediaCommand, MediaEvent>(
        &manifest,
        &token.unwrap_or_default(),
        "formal-web.media",
    )
    .map_err(|error| format!("ipc extension bootstrap failed: {error}"))?;

    let ipc_event_tx = server.tx;
    let (crossbeam_frame_tx, crossbeam_frame_rx) = crossbeam_channel::unbounded::<MediaEvent>();

    #[cfg(feature = "backend-gstreamer")]
    let backend = backend::gstreamer::GStreamerBackend::init()
        .map_err(|error| format!("GStreamer init failed: {error}"))?;

    #[cfg(feature = "backend-avfoundation")]
    let backend = backend::avfoundation::AvfBackend::init()
        .map_err(|error| format!("AVFoundation init failed: {error}"))?;

    run_media_process(
        backend,
        server.rx,
        crossbeam_frame_tx,
        crossbeam_frame_rx,
        ipc_event_tx,
    );
    Ok(())
}
