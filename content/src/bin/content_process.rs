use log::error;
use std::process;

fn main() {
    // Catch panics to prevent SIGTRAP from launchd
    std::panic::set_hook(Box::new(|info| {
        eprintln!("CONTENT_PANIC: {}", info);
    }));

    env_logger::init();

    if let Err(error) = content::run_content_process_from_args() {
        error!("formal-web-content: {error}");
        process::exit(1);
    }
}
