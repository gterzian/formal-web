use std::process;

fn main() {
    if let Err(error) = net::run_net_process_from_args() {
        eprintln!("formal-web-net: {error}");
        process::exit(1);
    }
}
