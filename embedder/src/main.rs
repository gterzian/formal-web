use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::process::ExitCode;
use verification::{TraceSender, VerificationRun, run_validation_from_iter};

#[derive(Parser, Debug)]
#[command(name = "formal-web-embedder")]
#[command(about = "Run the formal-web embedder runtime")]
struct Cli {
    #[arg(long, alias = "tla", global = true, default_value_t = false)]
    verify: bool,

    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    #[command(name = "webdriver")]
    WebDriver(automation::WebDriverArgs),
}

#[derive(Clone, Default)]
struct AppRunOptions {
    headless: bool,
    startup_url: Option<String>,
    window_title: Option<String>,
    trace_sender: Option<TraceSender>,
}

fn run_app_with_options(options: AppRunOptions) -> Result<(), String> {
    embedder::set_event_loop_options(embedder::EventLoopOptions {
        startup_url: options.startup_url,
        window_title: options.window_title,
    });

    let event_loop_result = if options.headless {
        embedder::run_headless_event_loop(options.trace_sender.clone())
    } else {
        embedder::run_headed_event_loop(options.trace_sender.clone())
    };
    embedder::clear_event_loop_options();

    event_loop_result
}

fn automation_runtime() -> automation::AutomationRuntime {
    automation::AutomationRuntime::new(
        |command| embedder::send_user_event(embedder::FormalWebUserEvent::Automation(command)),
        || embedder::send_user_event(embedder::FormalWebUserEvent::Exit),
        embedder::event_loop_is_ready,
    )
}

fn run_webdriver(
    args: automation::WebDriverArgs,
    trace_sender: Option<TraceSender>,
) -> Result<(), String> {
    let server = automation::WebDriverServer::start(
        args.port,
        args.exit_on_session_delete,
        automation_runtime(),
    )?;
    let result = run_app_with_options(AppRunOptions {
        headless: args.headless,
        startup_url: args
            .startup_url
            .or_else(|| Some(String::from("about:blank"))),
        window_title: Some(format!("formal-web WebDriver :{}", args.port)),
        trace_sender,
    });
    drop(server);
    result
}

fn combine_results(
    primary: Result<(), String>,
    final_step: Result<(), String>,
) -> Result<(), String> {
    match (primary, final_step) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(final_error)) => Err(format!("{error}; {final_error}")),
    }
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
            eprintln!("formal-web-embedder: {error}");
            ExitCode::from(1)
        }
    })
}

fn main() -> ExitCode {
    if let Some(exit_code) = delegated_tla_validate_command() {
        return exit_code;
    }

    let cli = Cli::parse();

    let verification_run = if cli.verify {
        match VerificationRun::start() {
            Ok(run) => Some(run),
            Err(error) => {
                eprintln!("formal-web-embedder: {error}");
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
        Some(CommandKind::WebDriver(args)) => run_webdriver(args, trace_sender.clone()),
    };
    drop(trace_sender);

    let verification_result = verification_run
        .map(VerificationRun::finish)
        .unwrap_or(Ok(()));
    let result = combine_results(result, verification_result);

    if let Err(error) = result {
        eprintln!("formal-web-embedder: {error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}