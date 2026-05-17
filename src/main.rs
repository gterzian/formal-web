mod webdriver;
mod wpt;

use clap::{Parser, Subcommand};
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{self, Command as ProcessCommand};
use webview::UserAgentApi;

const MAIN_ROLE_BOOTSTRAP_FLAG: &str = "--formal-web-main-role";

#[derive(Parser, Debug)]
#[command(name = "formal-web")]
#[command(about = "Rust entry point for the formal-web runtime and local WPT tooling")]
struct Cli {
    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    TestWpt(wpt::TestWptArgs),
    #[command(name = "webdriver")]
    WebDriver(webdriver::WebDriverArgs),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AppRunOptions {
    pub headless: bool,
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
}

pub(crate) fn run_app_with_options(options: AppRunOptions) -> Result<(), String> {
    embedder::set_event_loop_options(embedder::EventLoopOptions {
        headless: options.headless,
        startup_url: options.startup_url,
        window_title: options.window_title,
    });

    let event_loop_result = embedder::run_event_loop(|dispatcher| {
        user_agent::UserAgent::start(dispatcher)
            .map(|user_agent| Box::new(user_agent) as Box<dyn UserAgentApi>)
    });
    embedder::clear_event_loop_options();

    event_loop_result
}

fn run_app() -> Result<(), String> {
    run_app_with_options(AppRunOptions::default())
}

fn argument_value(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .windows(2)
        .find_map(|pair| (pair[0] == name).then(|| pair[1].clone()))
}

fn has_argument(arguments: &[String], name: &str) -> bool {
    arguments.iter().any(|argument| argument == name)
}

fn is_sidecar_process(arguments: &[String]) -> bool {
    has_argument(arguments, "--content-token") || has_argument(arguments, "--net-token")
}

fn sanitize_process_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn process_role_name(arguments: &[String]) -> String {
    if argument_value(arguments, "--net-token").is_some() {
        return String::from("formal-web:net");
    }

    if argument_value(arguments, "--content-token").is_some() {
        let content_label = argument_value(arguments, "--content-label")
            .unwrap_or_else(|| String::from("unknown"));
        let site_label = url::Url::parse(&content_label)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_owned))
            .unwrap_or(content_label);
        return format!(
            "formal-web:content:{}",
            sanitize_process_component(&site_label)
        );
    }

    String::from("formal-web:main")
}

fn maybe_reexec_main_with_role_name(arguments: &[String]) -> Result<(), String> {
    if is_sidecar_process(arguments) || has_argument(arguments, MAIN_ROLE_BOOTSTRAP_FLAG) {
        return Ok(());
    }

    let executable_path = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let mut child_process = ProcessCommand::new(&executable_path);
    #[cfg(unix)]
    child_process.arg0("formal-web:main");
    child_process.args(arguments.iter().skip(1));
    child_process.arg(MAIN_ROLE_BOOTSTRAP_FLAG);

    #[cfg(unix)]
    {
        let error = child_process.exec();
        return Err(format!("failed to exec main process role: {error}"));
    }

    #[cfg(not(unix))]
    {
        let status = child_process
            .status()
            .map_err(|error| format!("failed to launch main process wrapper: {error}"))?;
        let exit_code = status.code().unwrap_or(1);
        process::exit(exit_code);
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn apply_process_role_name(name: &str) {
    if let Ok(c_name) = CString::new(name) {
        // setprogname keeps the pointer for process lifetime; leak one allocation per process.
        let raw = c_name.into_raw();
        unsafe {
            libc::setprogname(raw);
        }
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
fn apply_process_role_name(_name: &str) {}

fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if let Err(error) = maybe_reexec_main_with_role_name(&arguments) {
        eprintln!("formal-web: {error}");
        process::exit(1);
    }

    let role_name = process_role_name(&arguments);
    apply_process_role_name(&role_name);

    if let Some(result) = content::maybe_run_content_process() {
        if let Err(error) = result {
            eprintln!("formal-web: {error}");
            process::exit(1);
        }
        return;
    }

    if let Some(result) = net::maybe_run_net_process() {
        if let Err(error) = result {
            eprintln!("formal-web: {error}");
            process::exit(1);
        }
        return;
    }

    let cli_arguments = arguments
        .into_iter()
        .filter(|argument| argument != MAIN_ROLE_BOOTSTRAP_FLAG)
        .collect::<Vec<_>>();
    let cli = Cli::parse_from(cli_arguments);
    let result = match cli.command {
        None => run_app(),
        Some(CommandKind::TestWpt(args)) => wpt::run(args),
        Some(CommandKind::WebDriver(args)) => webdriver::run(args),
    };

    if let Err(error) = result {
        eprintln!("formal-web: {error}");
        process::exit(1);
    }
}
