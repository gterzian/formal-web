use log::error;
use std::process;

fn main() {
    env_logger::init();

    if let Err(error) = media::run_media_process_from_args() {
        error!("formal-web-media: {error}");
        process::exit(1);
    }
}
