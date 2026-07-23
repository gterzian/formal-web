use ipc_messages::graphics::{GraphicsCommand, GraphicsEvent};
use media::backend::MediaBackend;

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

        // Initialize the platform-specific media backend.
        // On Apple platforms AVFoundation is used; elsewhere GStreamer.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let backend: Option<media::backend::avfoundation::AvfBackend> =
            match media::backend::avfoundation::AvfBackend::init() {
                Ok(b) => {
                    log::info!("[graphics] AVFoundation backend initialized");
                    Some(b)
                }
                Err(e) => {
                    log::error!("[graphics] AVFoundation init failed: {e}");
                    None
                }
            };

        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let backend: Option<media::backend::gstreamer::GStreamerBackend> =
            match media::backend::gstreamer::GStreamerBackend::init() {
                Ok(b) => {
                    log::info!("[graphics] GStreamer backend initialized");
                    Some(b)
                }
                Err(e) => {
                    log::error!("[graphics] GStreamer init failed: {e}");
                    None
                }
            };

        graphics::run_graphics_process(receiver, event_tx, backend);
        Ok(())
    });
    if let Err(error) = result {
        log::error!("[graphics] extension exited with error: {error}");
    }
}
