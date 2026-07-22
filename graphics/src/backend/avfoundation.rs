use crate::backend::{BackendEvent, MediaBackend, PipelineHandle};
use ipc_messages::media::MediaPipelineId;

/// AVFoundation backend for video decoding.
pub struct AvfBackend;

impl MediaBackend for AvfBackend {
    type Pipeline = AvfPipeline;

    fn init() -> Result<Self, String> {
        Err("AVFoundation backend not yet integrated in graphics process".into())
    }

    fn create_pipeline(
        &mut self,
        _id: MediaPipelineId,
        _url: String,
    ) -> Result<Self::Pipeline, String> {
        Err("AVFoundation backend not yet integrated in graphics process".into())
    }

    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        drop(tx);
        rx
    }
}

pub struct AvfPipeline;

impl PipelineHandle for AvfPipeline {
    fn play(&self) -> Result<(), String> {
        Ok(())
    }
    fn pause(&self) -> Result<(), String> {
        Ok(())
    }
    fn seek(&self, _position_secs: f64) -> Result<(), String> {
        Ok(())
    }
    fn destroy(self) -> Result<(), String> {
        Ok(())
    }
}
