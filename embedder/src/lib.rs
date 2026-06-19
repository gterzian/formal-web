mod event_loop;

use verification::{TraceSender, VerificationRun};

pub use event_loop::{
    EventLoopOptions, FormalWebUserEvent, NavigationCompleted, NavigationCompletion,
    clear_event_loop_options, event_loop_is_ready, run_headed_event_loop, run_headless_event_loop,
    send_user_event, set_event_loop_options, window_viewport_snapshot,
};

#[derive(Clone, Default)]
pub struct AppRunOptions {
    pub headless: bool,
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
    pub trace_sender: Option<TraceSender>,
}

pub fn run_default(verify: bool, headless: bool) -> Result<(), String> {
    let verification_run = if verify {
        Some(
            VerificationRun::start()
                .map_err(|error| format!("failed to start verification: {error}"))?,
        )
    } else {
        None
    };
    let trace_sender = verification_run.as_ref().map(VerificationRun::sender_clone);

    let result = run_app_with_options(AppRunOptions {
        headless,
        trace_sender,
        ..AppRunOptions::default()
    });

    let verification_result = verification_run
        .map(VerificationRun::finish)
        .unwrap_or(Ok(()));
    combine_results(result, verification_result)
}

pub fn run_app_with_options(options: AppRunOptions) -> Result<(), String> {
    set_event_loop_options(EventLoopOptions {
        startup_url: options.startup_url,
        window_title: options.window_title,
    });

    let event_loop_result = if options.headless {
        run_headless_event_loop(options.trace_sender.clone())
    } else {
        run_headed_event_loop(options.trace_sender.clone())
    };
    clear_event_loop_options();

    event_loop_result
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
    let result = run_app_with_options(AppRunOptions {
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
    let result = run_app_with_options(AppRunOptions {
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
