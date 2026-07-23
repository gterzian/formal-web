use log::error;
use std::process;

fn main() {
    env_logger::init();
    error!("formal-web-media is obsolete — media is handled by the graphics process");
    process::exit(1);
}
