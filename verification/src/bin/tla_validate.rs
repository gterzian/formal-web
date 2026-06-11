use log::error;
use std::process::ExitCode;

fn main() -> ExitCode {
    env_logger::init();

    match verification::run_validation_from_iter(std::env::args_os()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            error!("tla-validate: {error}");
            ExitCode::from(1)
        }
    }
}
