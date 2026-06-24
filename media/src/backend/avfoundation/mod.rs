mod av_sys;
mod pipeline;

pub use pipeline::AvfPipeline;

use crate::backend::{BackendEvent, MediaBackend};
use crossbeam_channel::Sender;
use ipc_messages::media::{MediaEvent, MediaPipelineId};

pub struct AvfBackend {
    // Event channel for future use (EOS/error/duration notifications).
    #[allow(dead_code)]
    event_tx: crossbeam_channel::Sender<BackendEvent>,
    event_rx: crossbeam_channel::Receiver<BackendEvent>,
}

impl MediaBackend for AvfBackend {
    type Pipeline = AvfPipeline;

    fn init() -> Result<Self, String> {
        let (event_tx, event_rx) = crossbeam_channel::unbounded::<BackendEvent>();
        Ok(Self { event_tx, event_rx })
    }

    fn create_pipeline(
        &mut self,
        id: MediaPipelineId,
        url: String,
        frame_tx: Sender<MediaEvent>,
    ) -> Result<Self::Pipeline, String> {
        AvfPipeline::new(id, url, frame_tx)
    }

    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent> {
        self.event_rx.clone()
    }
}
