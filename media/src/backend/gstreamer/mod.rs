mod pipeline;
pub use pipeline::GstPipeline;

use crate::backend::{BackendEvent, MediaBackend};
use crossbeam_channel::Sender;
use gstreamer as gst;
use ipc_messages::media::{MediaEvent, MediaPipelineId};

pub struct GStreamerBackend {
    event_tx: crossbeam_channel::Sender<BackendEvent>,
    event_rx: crossbeam_channel::Receiver<BackendEvent>,
}

impl MediaBackend for GStreamerBackend {
    type Pipeline = GstPipeline;

    fn init() -> Result<Self, String> {
        // On macOS, ensure NSApplication is set up on the main thread before any
        // GStreamer GL elements are created.
        #[cfg(target_os = "macos")]
        gst::macos_main(|| {});

        if gst::init().is_err() {
            return Err(String::from("GStreamer initialization failed"));
        }

        let (event_tx, event_rx) = crossbeam_channel::unbounded::<BackendEvent>();
        Ok(Self { event_tx, event_rx })
    }

    fn create_pipeline(
        &mut self,
        id: MediaPipelineId,
        url: String,
        frame_tx: Sender<MediaEvent>,
    ) -> Result<Self::Pipeline, String> {
        GstPipeline::new(id, url, frame_tx, self.event_tx.clone())
    }

    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent> {
        self.event_rx.clone()
    }
}
