use log::error;
use std::process;

fn main() {
    env_logger::init();

    if let Err(error) = webrtc::run_webrtc_process_from_args() {
        error!("formal-web-webrtc: {error}");
        process::exit(1);
    }
}
