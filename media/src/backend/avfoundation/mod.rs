mod av_sys;
mod pipeline;

pub use pipeline::AvfPipeline;

use crate::backend::{BackendEvent, MediaBackend};
use ipc_messages::media::MediaPipelineId;

pub struct AvfBackend {
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
    ) -> Result<Self::Pipeline, String> {
        AvfPipeline::new(id, url, self.event_tx.clone())
    }

    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent> {
        self.event_rx.clone()
    }
}
