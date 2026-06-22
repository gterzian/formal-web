use clap::{Parser, Subcommand};
use log::error;
use std::ffi::OsString;
use std::process::ExitCode;
use verification::run_validation_from_iter;

#[derive(Parser, Debug)]
#[command(name = "formal-web")]
#[command(about = "Convenient repository development entrypoint")]
struct Cli {
    #[arg(long, alias = "tla", global = true, default_value_t = false)]
    verify: bool,

    #[arg(long, default_value_t = false)]
    headless: bool,

    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    #[command(name = "wpt")]
    Wpt {
        #[command(flatten)]
        args: wpt_runner::TestWptArgs,
    },

    #[command(name = "webdriver")]
    WebDriver(automation::WebDriverArgs),

    #[command(name = "cdp")]
    Cdp(automation::CdpArgs),
}

fn delegated_tla_validate_command() -> Option<ExitCode> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "validate-tla") {
        return None;
    }

    let forwarded_args = std::iter::once(OsString::from("tla-validate"))
        .chain(args.into_iter().skip(2))
        .collect::<Vec<_>>();
    Some(match run_validation_from_iter(forwarded_args) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            error!("formal-web: {error}");
            ExitCode::from(1)
        }
    })
}

fn main() -> ExitCode {
    env_logger::init();
    if let Some(exit_code) = delegated_tla_validate_command() {
        return exit_code;
    }

    let cli = Cli::parse();
    let (wpt_args, command) = match cli.command {
        Some(CommandKind::Wpt { args }) => (Some(args), None),
        other => (None, other),
    };

    if let Some(args) = wpt_args {
        return match wpt_runner::run(args, cli.verify) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                error!("formal-web: {error}");
                ExitCode::from(1)
            }
        };
    }

    let result = match command {
        None => embedder::run_default(cli.verify, cli.headless),
        Some(CommandKind::WebDriver(args)) => {
            embedder::run_webdriver(args, cli.verify, cli.headless)
        }
        Some(CommandKind::Cdp(args)) => embedder::run_cdp(args, cli.verify, cli.headless),
        Some(CommandKind::Wpt { .. }) => Ok(()),
    };

    if let Err(error) = result {
        error!("formal-web: {error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
