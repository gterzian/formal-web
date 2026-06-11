use log::error;
use std::process;

fn main() {
    env_logger::init();

    if let Err(error) = content::run_content_process_from_args() {
        error!("formal-web-content: {error}");
        process::exit(1);
    }
}
