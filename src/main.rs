mod webdriver;
mod wpt;

use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::process::ExitCode;
use verification::{TraceSender, VerificationRun, run_validation_from_iter};
use webview::UserAgentApi;

#[derive(Parser, Debug)]
#[command(name = "formal-web")]
#[command(about = "Rust entry point for the formal-web runtime and local WPT tooling")]
struct Cli {
    #[arg(long, alias = "tla", global = true, default_value_t = false)]
    verify: bool,

    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    TestWpt(wpt::TestWptArgs),
    #[command(name = "webdriver")]
    WebDriver(webdriver::WebDriverArgs),
}

#[derive(Clone, Default)]
pub(crate) struct AppRunOptions {
    pub headless: bool,
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
    pub trace_sender: Option<TraceSender>,
}

pub(crate) fn run_app_with_options(options: AppRunOptions) -> Result<(), String> {
    embedder::set_event_loop_options(embedder::EventLoopOptions {
        headless: options.headless,
        startup_url: options.startup_url,
        window_title: options.window_title,
    });

    let event_loop_result = embedder::run_event_loop(|dispatcher| {
        user_agent::UserAgent::start(dispatcher, options.trace_sender.clone())
            .map(|user_agent| Box::new(user_agent) as Box<dyn UserAgentApi>)
    });
    embedder::clear_event_loop_options();

    event_loop_result
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
            eprintln!("formal-web: {error}");
            ExitCode::from(1)
        }
    })
}

fn combine_results(primary: Result<(), String>, final_step: Result<(), String>) -> Result<(), String> {
    match (primary, final_step) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(final_error)) => Err(format!("{error}; {final_error}")),
    }
}

fn main() -> ExitCode {
    if let Some(exit_code) = delegated_tla_validate_command() {
        return exit_code;
    }

    let cli = Cli::parse();

    if let Some(CommandKind::TestWpt(args)) = cli.command {
        if let Err(error) = wpt::run(args, cli.verify) {
            eprintln!("formal-web: {error}");
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
    }

    let verification_run = if cli.verify {
        match VerificationRun::start() {
            Ok(run) => Some(run),
            Err(error) => {
                eprintln!("formal-web: {error}");
                return ExitCode::from(1);
            }
        }
    } else {
        None
    };

    let trace_sender = verification_run.as_ref().map(VerificationRun::sender_clone);
    let result = match cli.command {
        None => run_app_with_options(AppRunOptions {
            trace_sender: trace_sender.clone(),
            ..AppRunOptions::default()
        }),
        Some(CommandKind::WebDriver(args)) => webdriver::run(args, trace_sender.clone()),
        Some(CommandKind::TestWpt(_)) => unreachable!(),
    };
    drop(trace_sender);
    let verification_result = verification_run.map(VerificationRun::finish).unwrap_or(Ok(()));
    let result = combine_results(result, verification_result);

    if let Err(error) = result {
        eprintln!("formal-web: {error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
