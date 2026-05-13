fn main() -> Result<(), String> {
    match net::maybe_run_net_process() {
        Some(result) => result,
        None => Err(String::from("missing --net-token argument")),
    }
}
