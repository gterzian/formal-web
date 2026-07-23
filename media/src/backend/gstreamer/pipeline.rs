use crossbeam_channel::Sender;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use ipc_messages::media::{MediaPipelineId, VideoFrame};

use crate::backend::{MediaBackendEvent, PipelineHandle};

pub struct GstPipeline {
    element: gst::Pipeline,
}

impl GstPipeline {
    pub fn new(
        id: MediaPipelineId,
        url: String,
        backend_event_tx: Sender<MediaBackendEvent>,
    ) -> Result<Self, String> {
        let pipeline = gst::Pipeline::new();

        let src = gst::ElementFactory::make("uridecodebin")
            .build()
            .map_err(|error| format!("failed to create uridecodebin: {error}"))?;
        let conv = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|error| format!("failed to create videoconvert: {error}"))?;
        let sink = gst::ElementFactory::make("appsink")
            .build()
            .map_err(|error| format!("failed to create appsink: {error}"))?;

        src.set_property_from_str("uri", &url);
        pipeline
            .add_many([&src, &conv, &sink])
            .map_err(|error| format!("failed to add elements to pipeline: {error}"))?;
        gst::Element::link_many([&conv, &sink])
            .map_err(|error| format!("failed to link elements: {error}"))?;

        // Force RGBA so the compositor can pass it directly.
        let appsink = sink
            .dynamic_cast::<gst_app::AppSink>()
            .map_err(|_| String::from("failed to cast sink to AppSink"))?;
        appsink.set_caps(Some(
            &gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .build(),
        ));

        // Dynamic pad: uridecodebin creates video pads at runtime.
        let conv_clone = conv.clone();
        src.connect_pad_added(move |_source, pad| {
            let Some(caps) = pad.current_caps() else {
                return;
            };
            let Some(structure) = caps.structure(0) else {
                return;
            };
            if !structure.name().starts_with("video/") {
                return;
            }
            let Some(sink_pad) = conv_clone.static_pad("sink") else {
                return;
            };
            if !sink_pad.is_linked() {
                let _ = pad.link(&sink_pad);
            }
        });

        // Frame callback — fires on the GStreamer streaming thread.
        let frame_tx = backend_event_tx.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = match sink.pull_sample() {
                        Ok(s) => s,
                        Err(_) => return Err(gst::FlowError::Eos),
                    };
                    let buffer = match sample.buffer() {
                        Some(b) => b,
                        None => return Err(gst::FlowError::Error),
                    };
                    let Some(caps) = sample.caps() else {
                        return Err(gst::FlowError::Error);
                    };
                    let Some(structure) = caps.structure(0) else {
                        return Err(gst::FlowError::Error);
                    };
                    let width: i32 = structure.get("width").map_err(|_| gst::FlowError::Error)?;
                    let height: i32 = structure.get("height").map_err(|_| gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                    use std::sync::atomic::{AtomicU64, Ordering};
                    static G_FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
                    let frame = VideoFrame {
                        pipeline_id: id,
                        width: width as u32,
                        height: height as u32,
                        data: map.as_slice().to_vec(),
                    };

                    let count = G_FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
                    if count % 30 == 0 {
                        log::debug!(
                            "[gst] pipeline {:?}: frame #{} ({}x{})",
                            id,
                            count,
                            width,
                            height,
                        );
                    }
                    let _ = frame_tx.send(MediaBackendEvent::Frame(frame));
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        // Route bus messages to MediaBackendEvent via sync handler.
        let pipeline_for_bus = pipeline.clone();
        let bus = pipeline
            .bus()
            .ok_or_else(|| String::from("failed to get GStreamer bus"))?;
        bus.set_sync_handler(move |_bus, message| {
            match message.view() {
                gst::MessageView::Eos(..) => {
                    let _ = backend_event_tx.send(MediaBackendEvent::Eos { pipeline_id: id });
                }
                gst::MessageView::Error(error) => {
                    let _ = backend_event_tx.send(MediaBackendEvent::Error {
                        pipeline_id: id,
                        message: error.error().to_string(),
                    });
                }
                gst::MessageView::DurationChanged(..) => {
                    if let Some(duration) = pipeline_for_bus.query_duration::<gst::ClockTime>() {
                        let _ = backend_event_tx.send(MediaBackendEvent::DurationChanged {
                            pipeline_id: id,
                            duration_secs: duration.seconds_f64(),
                        });
                    }
                }
                _ => {}
            }
            gst::BusSyncReply::Drop
        });

        pipeline
            .set_state(gst::State::Paused)
            .map_err(|error| format!("failed to set pipeline to paused: {error}"))?;

        Ok(Self { element: pipeline })
    }
}

impl PipelineHandle for GstPipeline {
    fn play(&self) -> Result<(), String> {
        self.element
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|error| format!("failed to play pipeline: {error}"))
    }

    fn pause(&self) -> Result<(), String> {
        self.element
            .set_state(gst::State::Paused)
            .map(|_| ())
            .map_err(|error| format!("failed to pause pipeline: {error}"))
    }

    fn seek(&self, position_secs: f64) -> Result<(), String> {
        let position = gst::ClockTime::from_seconds_f64(position_secs);
        self.element
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, position)
            .map_err(|error| format!("failed to seek pipeline: {error}"))
    }

    fn destroy(self) -> Result<(), String> {
        self.element
            .set_state(gst::State::Null)
            .map(|_| ())
            .map_err(|error| format!("failed to destroy pipeline: {error}"))
    }
}
