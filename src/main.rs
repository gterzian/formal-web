mod webdriver;
mod wpt;

use clap::{Parser, Subcommand};
use ipc_channel::ipc::{self, IpcSender};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process;
use std::thread::JoinHandle;
use tla_trace::{LogEntry, Monitor, receive_monitor_sender};
use webview::UserAgentApi;

#[derive(Parser, Debug)]
#[command(name = "formal-web")]
#[command(about = "Rust entry point for the formal-web runtime and local WPT tooling")]
struct Cli {
    #[arg(long, global = true, default_value_t = false)]
    tla: bool,

    #[arg(long, global = true, value_name = "DIR", default_value = "tla-traces")]
    tla_dir: PathBuf,

    #[arg(long, global = true, hide = true, value_name = "TOKEN")]
    tla_log_server: Option<String>,

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
    pub monitor_tx: Option<IpcSender<LogEntry>>,
}

struct TracingRuntime {
    sender: IpcSender<LogEntry>,
    join_handle: JoinHandle<Result<(), String>>,
}

impl TracingRuntime {
    fn start(output_dir: PathBuf) -> Result<Self, String> {
        prepare_trace_output_dir(&output_dir)?;
        let (sender, receiver) =
            ipc::channel::<LogEntry>().map_err(|error| format!("failed to create TLA trace channel: {error}"))?;
        let join_handle = std::thread::Builder::new()
            .name(String::from("formal-web:monitor"))
            .spawn(move || Monitor::new(output_dir, receiver).run())
            .map_err(|error| format!("failed to spawn TLA monitor thread: {error}"))?;
        Ok(Self {
            sender,
            join_handle,
        })
    }

    fn sender_clone(&self) -> IpcSender<LogEntry> {
        self.sender.clone()
    }

    fn shutdown(self) -> Result<(), String> {
        let Self {
            sender,
            join_handle,
        } = self;
        drop(sender);
        join_handle
            .join()
            .map_err(|_| String::from("TLA monitor thread panicked"))?
    }
}

fn prepare_trace_output_dir(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to clear TLA trace directory {}: {error}",
                path.display()
            ));
        }
    }

    fs::create_dir_all(path).map_err(|error| {
        format!(
            "failed to create TLA trace directory {}: {error}",
            path.display()
        )
    })
}

pub(crate) fn run_app_with_options(options: AppRunOptions) -> Result<(), String> {
    embedder::set_event_loop_options(embedder::EventLoopOptions {
        headless: options.headless,
        startup_url: options.startup_url,
        window_title: options.window_title,
    });

    let event_loop_result = embedder::run_event_loop(|dispatcher| {
        user_agent::UserAgent::start(dispatcher, options.monitor_tx.clone())
            .map(|user_agent| Box::new(user_agent) as Box<dyn UserAgentApi>)
    });
    embedder::clear_event_loop_options();

    event_loop_result
}

fn main() {
    let cli = Cli::parse();

    if cli.tla && cli.tla_log_server.is_some() {
        eprintln!("formal-web: --tla and --tla-log-server cannot be used together");
        process::exit(1);
    }

    let tracing = if cli.tla_log_server.is_some() {
        None
    } else if cli.tla {
        match TracingRuntime::start(cli.tla_dir.clone()) {
            Ok(tracing) => Some(tracing),
            Err(error) => {
                eprintln!("formal-web: {error}");
                process::exit(1);
            }
        }
    } else {
        None
    };

    let monitor_tx = if let Some(server_name) = cli.tla_log_server.as_deref() {
        match receive_monitor_sender(Some(server_name)) {
            Ok(monitor_tx) => monitor_tx,
            Err(error) => {
                eprintln!("formal-web: {error}");
                process::exit(1);
            }
        }
    } else {
        tracing.as_ref().map(TracingRuntime::sender_clone)
    };
    let result = match cli.command {
        None => run_app_with_options(AppRunOptions {
            monitor_tx: monitor_tx.clone(),
            ..AppRunOptions::default()
        }),
        Some(CommandKind::TestWpt(args)) => wpt::run(args, monitor_tx.clone()),
        Some(CommandKind::WebDriver(args)) => webdriver::run(args, monitor_tx.clone()),
    };
    let monitor_result = tracing.map(TracingRuntime::shutdown).unwrap_or(Ok(()));
    let result = result.and(monitor_result);

    if let Err(error) = result {
        eprintln!("formal-web: {error}");
        process::exit(1);
    };
}
