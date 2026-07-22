use crate::backend::{BackendEvent, MediaBackend, PipelineHandle};
use ipc_messages::media::MediaPipelineId;

/// GStreamer backend for video decoding.
pub struct GStreamerBackend;

impl MediaBackend for GStreamerBackend {
    type Pipeline = GStreamerPipeline;

    fn init() -> Result<Self, String> {
        Err("GStreamer backend not yet integrated in graphics process".into())
    }

    fn create_pipeline(
        &mut self,
        _id: MediaPipelineId,
        _url: String,
    ) -> Result<Self::Pipeline, String> {
        Err("GStreamer backend not yet integrated in graphics process".into())
    }

    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        drop(tx);
        rx
    }
}

pub struct GStreamerPipeline;

impl PipelineHandle for GStreamerPipeline {
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
