use log::error;
use std::process;

fn main() {
    env_logger::init();

    if let Err(error) = net::run_net_process_from_args() {
        error!("formal-web-net: {error}");
        process::exit(1);
    }
}
