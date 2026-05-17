use std::process;

fn main() {
    if let Err(error) = content::run_content_process_from_args() {
        eprintln!("formal-web-content: {error}");
        process::exit(1);
    }
}
