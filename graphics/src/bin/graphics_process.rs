use ipc_messages::graphics::{GraphicsCommand, GraphicsEvent};

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

    let result = ipc::run_extension::<GraphicsCommand, GraphicsEvent>(
        &token,
        graphics::run_graphics_extension,
    );
    if let Err(error) = result {
        log::error!("[graphics] extension exited with error: {error}");
    }
}
