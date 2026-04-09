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
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AppRunOptions {
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
}

pub(crate) fn run_app_with_options(options: AppRunOptions) -> Result<(), String> {
    ffi::initialize_lean_runtime()?;
    ffi::install_runtime_hooks();
    embedder::set_event_loop_options(embedder::EventLoopOptions {
        startup_url: options.startup_url,
        window_title: options.window_title,
    });

    if let Err(error) = ffi::start_kernel() {
        embedder::clear_event_loop_options();
        let _ = ffi::finalize_lean_runtime();
        return Err(error);
    }

    let event_loop_result = embedder::run_event_loop();
    let shutdown_result = ffi::shutdown_kernel();
    let finalize_result = ffi::finalize_lean_runtime();
    embedder::clear_event_loop_options();

    event_loop_result
        .and(shutdown_result)
        .and(finalize_result)
}

fn run_app() -> Result<(), String> {
    run_app_with_options(AppRunOptions::default())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        None => run_app(),
        Some(CommandKind::TestWpt(args)) => wpt::run(args),
    };

    if let Err(error) = result {
        eprintln!("formal-web: {error}");
        process::exit(1);
    }
}
