//! The winit-based embedder: a windowed app with a Blitz-rendered browser
//! chrome and a headless app for automation (WebDriver, CDP, WPT). The
//! windowed app requires the `windowed` feature (on by default); the
//! headless app is always available and pulls no graphics dependencies.

mod events;
mod headless;
mod shared;

#[cfg(feature = "windowed")]
mod chrome;
#[cfg(feature = "windowed")]
mod windowed;
#[cfg(feature = "windowed")]
mod winit_integration;

pub use events::{
    EventLoopEmbedder, FormalWebUserEvent, UserEventSink, WinitEventSink, clear_user_event_sink,
    event_loop_is_ready, install_user_event_sink, send_user_event,
};
pub use shared::*;

use std::sync::Arc;
use verification::{TraceSender, VerificationRun};
use webview::{EmbedderConfig, WebviewProvider};
use winit::application::ApplicationHandler;
use winit::event_loop::EventLoop;

#[derive(Clone, Default)]
pub struct AppRunOptions {
    pub headless: bool,
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
    pub trace_sender: Option<TraceSender>,
}

/// Run the winit app (windowed or headless) with the given options.
pub fn run_app(options: AppRunOptions) -> Result<(), String> {
    let trace_sender = options.trace_sender;
    if options.headless {
        run_headless_app(trace_sender, options.startup_url, options.window_title)
    } else {
        run_windowed_app(trace_sender, options.startup_url, options.window_title)
    }
}

/// Run the winit windowed app until it exits.
pub fn run_windowed_app(
    trace_sender: Option<TraceSender>,
    startup_url: Option<String>,
    window_title: Option<String>,
) -> Result<(), String> {
    #[cfg(feature = "windowed")]
    {
        use windowed::WindowedApp;

        run_winit_event_loop(trace_sender.clone(), |provider, _trace_sender| {
            WindowedApp {
                provider: Some(provider),
                startup_url,
                window_title,
                ..WindowedApp::default()
            }
        })
    }
    #[cfg(not(feature = "windowed"))]
    {
        let _ = (trace_sender, startup_url, window_title);
        Err(String::from(
            "the winit windowed app is not compiled: on macOS build the embedder with --features winit_embedder (headless automation with --headless works on any build)",
        ))
    }
}

/// Run the headless winit app until it exits. Used by WPT, WebDriver, and
/// CDP; no window, no chrome, no graphics dependencies.
pub fn run_headless_app(
    trace_sender: Option<TraceSender>,
    startup_url: Option<String>,
    _window_title: Option<String>,
) -> Result<(), String> {
    use headless::HeadlessEmbedderApp;

    run_winit_event_loop(trace_sender.clone(), |provider, _trace_sender| {
        HeadlessEmbedderApp {
            provider: Some(provider),
            startup_url,
            ..HeadlessEmbedderApp::default()
        }
    })
}

/// Run a winit-based app (headless, or the winit windowed backend) on a
/// winit event loop: installs the event sink, starts the user agent, and
/// runs the app until it exits.
pub fn run_winit_event_loop<A, MakeApp>(
    trace_sender: Option<TraceSender>,
    make_app: MakeApp,
) -> Result<(), String>
where
    A: ApplicationHandler<FormalWebUserEvent>,
    MakeApp: FnOnce(WebviewProvider, Option<TraceSender>) -> A,
{
    let event_loop = EventLoop::<FormalWebUserEvent>::with_user_event()
        .build()
        .map_err(|error| format!("failed to create event loop: {error}"))?;
    let sink: Arc<dyn UserEventSink> = Arc::new(WinitEventSink::new(event_loop.create_proxy()));
    install_user_event_sink(sink.clone());

    let event_loop_embedder = Arc::new(EventLoopEmbedder::new(sink));
    let provider = match WebviewProvider::new(
        event_loop_embedder,
        trace_sender.clone(),
        EmbedderConfig::default(),
    ) {
        Ok(provider) => provider,
        Err(error) => {
            clear_user_event_sink();
            update_window_viewport_snapshot(None);
            return Err(error);
        }
    };

    let mut app = make_app(provider, trace_sender);
    let run_result = event_loop
        .run_app(&mut app)
        .map_err(|error| format!("winit event loop failed: {error}"));

    clear_user_event_sink();
    update_window_viewport_snapshot(None);

    run_result
}

pub fn run_webdriver(
    args: automation::WebDriverArgs,
    verify: bool,
    headless: bool,
) -> Result<(), String> {
    let verification_run = if verify {
        Some(
            VerificationRun::start()
                .map_err(|error| format!("failed to start verification: {error}"))?,
        )
    } else {
        None
    };
    let trace_sender = verification_run.as_ref().map(VerificationRun::sender_clone);

    let runtime = automation::automation_bridge(
        |command| send_user_event(FormalWebUserEvent::Automation(command)),
        || send_user_event(FormalWebUserEvent::Exit),
        event_loop_is_ready,
    );
    let webdriver_server = automation::WebDriverServer::start(
        args.port,
        args.exit_on_session_delete,
        runtime.clone(),
    )?;
    let cdp_server = args
        .cdp_port
        .map(|port| automation::CdpServerHandle::start(port, runtime))
        .transpose()?;
    let result = run_app(AppRunOptions {
        headless: args.headless || headless,
        startup_url: args
            .startup_url
            .or_else(|| Some(String::from("about:blank"))),
        window_title: Some(format!("formal-web WebDriver :{}", args.port)),
        trace_sender,
    });
    drop(cdp_server);
    drop(webdriver_server);

    let verification_result = verification_run
        .map(VerificationRun::finish)
        .unwrap_or(Ok(()));
    combine_results(result, verification_result)
}

pub fn run_cdp(args: automation::CdpArgs, verify: bool, headless: bool) -> Result<(), String> {
    let verification_run = if verify {
        Some(
            VerificationRun::start()
                .map_err(|error| format!("failed to start verification: {error}"))?,
        )
    } else {
        None
    };
    let trace_sender = verification_run.as_ref().map(VerificationRun::sender_clone);

    let runtime = automation::automation_bridge(
        |command| send_user_event(FormalWebUserEvent::Automation(command)),
        || send_user_event(FormalWebUserEvent::Exit),
        event_loop_is_ready,
    );
    let server = automation::CdpServerHandle::start(args.port, runtime)?;
    let result = run_app(AppRunOptions {
        headless: args.headless || headless,
        startup_url: args
            .startup_url
            .or_else(|| Some(String::from("about:blank"))),
        window_title: Some(format!("formal-web CDP :{}", args.port)),
        trace_sender,
    });
    drop(server);

    let verification_result = verification_run
        .map(VerificationRun::finish)
        .unwrap_or(Ok(()));
    combine_results(result, verification_result)
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
