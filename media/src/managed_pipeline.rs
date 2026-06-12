use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use ipc_channel::ipc::{IpcSender, IpcSharedMemory};
use ipc_messages::media::{MediaEvent, MediaPipelineId, VideoFrame};

pub(crate) struct ManagedPipeline {
    pub id: MediaPipelineId,
    pub element: gst::Pipeline,
    pub bus: gst::Bus,
}

impl ManagedPipeline {
    pub fn new(
        id: MediaPipelineId,
        url: String,
        event_sender: IpcSender<MediaEvent>,
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
            .add_many(&[&src, &conv, &sink])
            .map_err(|error| format!("failed to add elements to pipeline: {error}"))?;
        gst::Element::link_many(&[&conv, &sink])
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
                    let Some(s) = caps.structure(0) else {
                        return Err(gst::FlowError::Error);
                    };
                    let width: i32 = s.get("width").map_err(|_| gst::FlowError::Error)?;
                    let height: i32 = s.get("height").map_err(|_| gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                    let frame = VideoFrame {
                        pipeline_id: id,
                        width: width as u32,
                        height: height as u32,
                        data: IpcSharedMemory::from_bytes(map.as_slice()),
                    };

                    let _ = event_sender.send(MediaEvent::Frame(frame));
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let bus = pipeline
            .bus()
            .ok_or_else(|| String::from("failed to get GStreamer bus"))?;

        pipeline
            .set_state(gst::State::Paused)
            .map_err(|error| format!("failed to set pipeline to paused: {error}"))?;

        Ok(Self {
            id,
            element: pipeline,
            bus,
        })
    }
}
