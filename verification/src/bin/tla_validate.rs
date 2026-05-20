use std::process::ExitCode;

fn main() -> ExitCode {
    match verification::run_validation_from_iter(std::env::args_os()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("tla-validate: {error}");
            ExitCode::from(1)
        }
    }
}