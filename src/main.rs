use clap::{Parser, Subcommand};
use log::error;
use std::ffi::OsString;
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
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

fn run_embedder_process(embedder_args: Vec<OsString>) -> Result<(), String> {
    // The embedder is a workspace member, so cargo build --release already
    // produces target/{profile}/formal-web-embedder alongside the root
    // binary.  Find it and run it directly rather than spawning a nested
    // cargo invocation (which would deadlock on the workspace target dir).
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let embedder_path = current_exe
        .parent()
        .ok_or_else(|| String::from("failed to resolve executable directory"))?
        .join("formal-web-embedder");

    if !embedder_path.is_file() {
        return Err(format!(
            "embedder binary not found at {}; run `cargo build --release -p embedder --bin formal-web-embedder` first",
            embedder_path.display()
        ));
    }

    let status = ProcessCommand::new(&embedder_path)
        .args(&embedder_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            format!(
                "failed to execute embedder at {}: {error}",
                embedder_path.display()
            )
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "embedder process exited with status {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| String::from("unknown"))
        ))
    }
}

fn run_embedder_default(verify: bool, headless: bool) -> Result<(), String> {
    let mut args = Vec::<OsString>::new();
    if verify {
        args.push(OsString::from("--verify"));
    }
    if headless {
        args.push(OsString::from("--headless"));
    }
    run_embedder_process(args)
}

fn run_embedder_webdriver(verify: bool, args: automation::WebDriverArgs) -> Result<(), String> {
    let mut embedder_args = Vec::<OsString>::new();
    if verify {
        embedder_args.push(OsString::from("--verify"));
    }
    embedder_args.push(OsString::from("webdriver"));
    embedder_args.push(OsString::from("--port"));
    embedder_args.push(OsString::from(args.port.to_string()));
    if let Some(cdp_port) = args.cdp_port {
        embedder_args.push(OsString::from("--cdp-port"));
        embedder_args.push(OsString::from(cdp_port.to_string()));
    }
    if args.headless {
        embedder_args.push(OsString::from("--headless"));
    }
    if let Some(startup_url) = args.startup_url {
        embedder_args.push(OsString::from("--startup-url"));
        embedder_args.push(OsString::from(startup_url));
    }
    if args.exit_on_session_delete {
        embedder_args.push(OsString::from("--exit-on-session-delete"));
    }
    run_embedder_process(embedder_args)
}

fn run_embedder_cdp(verify: bool, args: automation::CdpArgs) -> Result<(), String> {
    let mut embedder_args = Vec::<OsString>::new();
    if verify {
        embedder_args.push(OsString::from("--verify"));
    }
    embedder_args.push(OsString::from("cdp"));
    embedder_args.push(OsString::from("--port"));
    embedder_args.push(OsString::from(args.port.to_string()));
    if args.headless {
        embedder_args.push(OsString::from("--headless"));
    }
    if let Some(startup_url) = args.startup_url {
        embedder_args.push(OsString::from("--startup-url"));
        embedder_args.push(OsString::from(startup_url));
    }
    run_embedder_process(embedder_args)
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
        None => run_embedder_default(cli.verify, cli.headless),
        Some(CommandKind::WebDriver(args)) => run_embedder_webdriver(cli.verify, args),
        Some(CommandKind::Cdp(args)) => run_embedder_cdp(cli.verify, args),
        Some(CommandKind::Wpt { .. }) => Ok(()),
    };

    if let Err(error) = result {
        error!("formal-web: {error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
