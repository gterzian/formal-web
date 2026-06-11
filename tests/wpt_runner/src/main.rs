use clap::Parser;
use log::error;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "formal-web-wpt")]
#[command(about = "Run the formal-web WPT runner")]
struct Cli {
    #[arg(long, alias = "tla", global = true, default_value_t = false)]
    verify: bool,

    #[command(flatten)]
    args: wpt_runner::TestWptArgs,
}

fn main() -> ExitCode {
    env_logger::init();
    let cli = Cli::parse();
    match wpt_runner::run(cli.args, cli.verify) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("formal-web-wpt: {error}");
            ExitCode::from(1)
        }
    }
}
