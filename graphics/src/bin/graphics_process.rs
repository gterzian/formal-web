use ipc_messages::graphics::{GraphicsCommand, GraphicsEvent};
use media::backend::{BackendEvent, MediaBackend, PipelineHandle};

struct NoopBackend;
impl MediaBackend for NoopBackend {
    type Pipeline = NoopPipeline;
    fn init() -> Result<Self, String> {
        Ok(Self)
    }
    fn create_pipeline(
        &mut self,
        _id: ipc_messages::media::MediaPipelineId,
        _url: String,
    ) -> Result<Self::Pipeline, String> {
        Err("no media backend".into())
    }
    fn event_receiver(&self) -> crossbeam_channel::Receiver<BackendEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        drop(tx);
        rx
    }
}
struct NoopPipeline;
impl PipelineHandle for NoopPipeline {
    fn play(&self) -> Result<(), String> {
        Ok(())
    }
    fn pause(&self) -> Result<(), String> {
        Ok(())
    }
    fn seek(&self, _p: f64) -> Result<(), String> {
        Ok(())
    }
    fn destroy(self) -> Result<(), String> {
        Ok(())
    }
}

fn main() {
    env_logger::init();
    log::info!("[graphics] starting graphics and media process");

    let token = {
        let mut args = std::env::args().skip(1);
        let mut found = None;
        while let Some(arg) = args.next() {
            if arg == "--graphics-token" {
                found = args.next();
                break;
            }
            if let Some(val) = arg.strip_prefix("--graphics-token=") {
                found = Some(val.to_owned());
                break;
            }
        }
        found.unwrap_or_default()
    };

    let result = ipc::run_extension::<GraphicsCommand, GraphicsEvent>(&token, |server| {
        let receiver = ipc::crossbeam_proxy(server.connection.receiver);
        let event_tx = server.connection.sender.clone();
        let backend: Option<NoopBackend> = None;
        graphics::run_graphics_process(receiver, event_tx, backend);
        Ok(())
    });
    if let Err(error) = result {
        log::error!("[graphics] extension exited with error: {error}");
    }
}
