mod webdriver;
mod wpt;

use clap::{Parser, Subcommand};
use std::process;

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

    let event_loop_result = match user_agent::UserAgent::start() {
        Ok(user_agent) => embedder::run_event_loop(Box::new(user_agent)),
        Err(error) => Err(error),
    };
    embedder::clear_event_loop_options();

    event_loop_result
}

fn run_app() -> Result<(), String> {
    run_app_with_options(AppRunOptions::default())
}

fn main() {
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

    let cli = Cli::parse();
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
