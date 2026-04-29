use blitz_traits::shell::ColorScheme;
use data_url::DataUrl;
use embedder::{ContentBridgeHooks, FormalWebUserEvent};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_messages::content::{
    AttachChildFrame, Bootstrap, ColorScheme as MessageColorScheme,
    Command as ContentCommand, Event as ContentEvent, FetchRequest, FetchResponse,
    LoadedDocumentResponse, NavigateRequest, UserNavigationInvolvement, ViewportSnapshot,
    WindowTimerRequest,
};
use reqwest::{Method, blocking::Client, header::CONTENT_TYPE};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);
const LOCAL_FETCH_TIMEOUT: Duration = Duration::from_secs(15);

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        eprintln!("[timer-debug][bridge] {}", message.as_ref());
    }
}

struct ContentBridge {
    command_sender: IpcSender<ContentCommand>,
    child: Mutex<Option<Child>>,
    listener: Mutex<Option<JoinHandle<()>>>,
    mode: BridgeMode,
    owned_subframes: Mutex<HashMap<u64, usize>>,
    local_timer_cancellations: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    script_waiters: Mutex<HashMap<u64, mpsc::Sender<Result<serde_json::Value, String>>>>,
}

#[derive(Clone, Copy)]
enum BridgeMode {
    TopLevel { event_loop_id: usize },
    Subframe { traversable_id: u64, frame_id: u64 },
}

static CONTENT_REGISTRY: LazyLock<Mutex<HashMap<usize, Arc<ContentBridge>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE_CONTENT_BRIDGE: LazyLock<Mutex<Option<Arc<ContentBridge>>>> =
    LazyLock::new(|| Mutex::new(None));
static NEXT_CONTENT_HANDLE: AtomicUsize = AtomicUsize::new(1);
static NEXT_SCRIPT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static LOCAL_FETCH_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(LOCAL_FETCH_TIMEOUT)
        .build()
        .expect("failed to build local content fetch client")
});

fn executable_file_name(stem: &str) -> String {
    if std::env::consts::EXE_EXTENSION.is_empty() {
        String::from(stem)
    } else {
        format!("{stem}.{}", std::env::consts::EXE_EXTENSION)
    }
}

fn content_executable_path() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| String::from("failed to resolve executable directory"))?;
    let profile_dir = parent;
    let profile_name = profile_dir
        .file_name()
        .ok_or_else(|| String::from("failed to resolve build profile directory"))?;
    let target_dir = profile_dir
        .parent()
        .ok_or_else(|| String::from("failed to resolve target directory"))?;

    let dedicated_target = target_dir
        .join("formal-web-content")
        .join(profile_name)
        .join(executable_file_name("content"));
    if dedicated_target.is_file() {
        return Ok(dedicated_target);
    }

    Ok(parent.join(executable_file_name("content")))
}

fn setup_common(command: &mut Command, token: &str) {
    command.arg("--content-token").arg(token);
}

fn content_color_scheme(color_scheme: ColorScheme) -> MessageColorScheme {
    match color_scheme {
        ColorScheme::Light => MessageColorScheme::Light,
        ColorScheme::Dark => MessageColorScheme::Dark,
    }
}

fn viewport_command(snapshot: (u32, u32, f32, ColorScheme)) -> ContentCommand {
    let (width, height, scale, color_scheme) = snapshot;
    ContentCommand::SetViewport(ViewportSnapshot {
        width,
        height,
        scale,
        color_scheme: content_color_scheme(color_scheme),
    })
}

fn navigation_user_involvement_name(user_involvement: UserNavigationInvolvement) -> &'static str {
    match user_involvement {
        UserNavigationInvolvement::None => "none",
        UserNavigationInvolvement::Activation => "activation",
        UserNavigationInvolvement::BrowserUi => "browser-ui",
    }
}

fn clear_local_timer(bridge: &ContentBridge, timer_key: u64) {
    if let Some(cancellation) = bridge
        .local_timer_cancellations
        .lock()
        .expect("content timer cancellation mutex poisoned")
        .remove(&timer_key)
    {
        cancellation.store(true, Ordering::Relaxed);
    }
}

fn clear_all_local_timers(bridge: &ContentBridge) {
    let cancellations = bridge
        .local_timer_cancellations
        .lock()
        .expect("content timer cancellation mutex poisoned")
        .drain()
        .map(|(_timer_key, cancellation)| cancellation)
        .collect::<Vec<_>>();
    for cancellation in cancellations {
        cancellation.store(true, Ordering::Relaxed);
    }
}

fn run_local_fetch(request: &FetchRequest) -> Result<FetchResponse, String> {
    if request.url.starts_with("data:") {
        let data_url = DataUrl::process(&request.url)
            .map_err(|error| format!("failed to parse data URL: {error}"))?;
        let (body, _fragment) = data_url
            .decode_to_vec()
            .map_err(|error| format!("failed to decode data URL: {error}"))?;
        return Ok(FetchResponse {
            final_url: request.url.clone(),
            status: 200,
            content_type: String::new(),
            body,
        });
    }

    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|error| format!("unsupported local fetch method {}: {error}", request.method))?;
    let mut builder = LOCAL_FETCH_CLIENT.request(method, &request.url);
    if !request.body.is_empty() {
        builder = builder.body(request.body.clone());
    }
    let response = builder
        .send()
        .map_err(|error| format!("local content fetch failed for {}: {error}", request.url))?;
    let final_url = response.url().to_string();
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response
        .bytes()
        .map_err(|error| format!("failed to read local fetch body for {}: {error}", request.url))?
        .to_vec();
    Ok(FetchResponse {
        final_url,
        status,
        content_type,
        body,
    })
}

fn run_local_navigation(request: &NavigateRequest) -> Result<LoadedDocumentResponse, String> {
    let response = run_local_fetch(&FetchRequest {
        handler_id: 0,
        url: request.destination_url.clone(),
        method: String::from("GET"),
        body: String::new(),
    })?;
    Ok(LoadedDocumentResponse {
        final_url: response.final_url,
        status: response.status,
        content_type: response.content_type,
        body: String::from_utf8_lossy(&response.body).into_owned(),
    })
}

fn spawn_local_fetch(bridge: Arc<ContentBridge>, request: FetchRequest) {
    thread::spawn(move || {
        let command = match run_local_fetch(&request) {
            Ok(response) => ContentCommand::CompleteDocumentFetch {
                handler_id: request.handler_id,
                response,
            },
            Err(error) => {
                eprintln!("content bridge local fetch error: {error}");
                ContentCommand::FailDocumentFetch {
                    handler_id: request.handler_id,
                }
            }
        };
        let _ = send_command_inner(&bridge, command);
    });
}

fn spawn_local_navigation(
    bridge: Arc<ContentBridge>,
    traversable_id: u64,
    frame_id: u64,
    request: NavigateRequest,
) {
    thread::spawn(move || match run_local_navigation(&request) {
        Ok(response) => {
            let _ = send_command_inner(
                &bridge,
                ContentCommand::DestroyDocument {
                    document_id: frame_id,
                },
            );
            let _ = send_command_inner(
                &bridge,
                ContentCommand::CreateLoadedDocument {
                    traversable_id,
                    document_id: frame_id,
                    response,
                },
            );
            let _ = send_command_inner(
                &bridge,
                ContentCommand::UpdateTheRendering {
                    traversable_id,
                    document_id: frame_id,
                },
            );
        }
        Err(error) => eprintln!("content bridge local navigation error: {error}"),
    });
}

fn schedule_local_timer(bridge: Arc<ContentBridge>, request: WindowTimerRequest) {
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut timers = bridge
            .local_timer_cancellations
            .lock()
            .expect("content timer cancellation mutex poisoned");
        if let Some(previous) = timers.insert(request.timer_key, Arc::clone(&cancellation)) {
            previous.store(true, Ordering::Relaxed);
        }
    }

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(u64::from(request.timeout_ms)));
        if cancellation.load(Ordering::Relaxed) {
            return;
        }
        let _ = send_command_inner(
            &bridge,
            ContentCommand::RunWindowTimer {
                document_id: request.document_id,
                timer_id: request.timer_id,
                timer_key: request.timer_key,
                nesting_level: request.nesting_level,
            },
        );
        bridge
            .local_timer_cancellations
            .lock()
            .expect("content timer cancellation mutex poisoned")
            .remove(&request.timer_key);
    });
}

fn take_owned_subframe_handle(bridge: &Arc<ContentBridge>, frame_id: u64) -> Option<usize> {
    bridge
        .owned_subframes
        .lock()
        .expect("content child bridge mutex poisoned")
        .remove(&frame_id)
}

fn drain_owned_subframe_handles(bridge: &Arc<ContentBridge>) -> Vec<usize> {
    bridge
        .owned_subframes
        .lock()
        .expect("content child bridge mutex poisoned")
        .drain()
        .map(|(_frame_id, handle)| handle)
        .collect()
}

fn attach_child_frame(bridge: &Arc<ContentBridge>, attach: AttachChildFrame) -> Result<(), String> {
    if let Some(handle) = take_owned_subframe_handle(bridge, attach.frame_id) {
        stop(handle)?;
    }

    let handle = start_bridge(
        BridgeMode::Subframe {
            traversable_id: attach.traversable_id,
            frame_id: attach.frame_id,
        },
        false,
    )?;

    if let Err(error) = send_command(
        handle,
        ContentCommand::CreateLoadedDocument {
            traversable_id: attach.traversable_id,
            document_id: attach.frame_id,
            response: attach.response.clone(),
        },
    )
    .and_then(|()| {
        send_command(
            handle,
            ContentCommand::UpdateTheRendering {
                traversable_id: attach.traversable_id,
                document_id: attach.frame_id,
            },
        )
    }) {
        let _ = stop(handle);
        return Err(error);
    }

    bridge
        .owned_subframes
        .lock()
        .expect("content child bridge mutex poisoned")
        .insert(attach.frame_id, handle);
    Ok(())
}

fn detach_child_frame(bridge: &Arc<ContentBridge>, frame_id: u64) -> Result<(), String> {
    if let Some(handle) = take_owned_subframe_handle(bridge, frame_id) {
        stop(handle)?;
    }
    Ok(())
}

fn spawn_listener(
    event_receiver: ipc_channel::ipc::IpcReceiver<ContentEvent>,
    bridge: Arc<ContentBridge>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut suppress_next_command_completed = false;
        loop {
            let event = match event_receiver.recv() {
                Ok(event) => event,
                Err(error) => {
                    eprintln!("content bridge listener error: {error}");
                    break;
                }
            };

            match event {
                ContentEvent::DocumentFetchRequested(request) => match bridge.mode {
                    BridgeMode::TopLevel { event_loop_id } => {
                        let _ = super::call_lean_document_fetch_start_parts(
                            event_loop_id,
                            request.handler_id as usize,
                            &request.url,
                            &request.method,
                            &request.body,
                        );
                    }
                    BridgeMode::Subframe { .. } => {
                        spawn_local_fetch(Arc::clone(&bridge), request);
                    }
                },
                ContentEvent::WindowTimerRequested(request) => match bridge.mode {
                    BridgeMode::TopLevel { event_loop_id } => {
                        log_timer_debug(format!(
                            "forward schedule document={} id={} key={} timeout_ms={} nesting={}",
                            request.document_id,
                            request.timer_id,
                            request.timer_key,
                            request.timeout_ms,
                            request.nesting_level
                        ));
                        let _ = super::call_lean_schedule_window_timer_parts(
                            event_loop_id,
                            request.document_id as usize,
                            request.timer_id as usize,
                            request.timer_key as usize,
                            request.timeout_ms as usize,
                            request.nesting_level as usize,
                        );
                    }
                    BridgeMode::Subframe { .. } => {
                        schedule_local_timer(Arc::clone(&bridge), request);
                    }
                },
                ContentEvent::WindowTimerCleared(request) => match bridge.mode {
                    BridgeMode::TopLevel { event_loop_id } => {
                        log_timer_debug(format!(
                            "forward clear document={} key={}",
                            request.document_id, request.timer_key
                        ));
                        let _ = request.document_id;
                        let _ = super::call_lean_clear_window_timer_parts(
                            event_loop_id,
                            request.timer_key as usize,
                        );
                    }
                    BridgeMode::Subframe { .. } => {
                        clear_local_timer(&bridge, request.timer_key);
                    }
                },
                ContentEvent::NavigationRequested(request) => match bridge.mode {
                    BridgeMode::TopLevel { event_loop_id } => {
                        let _ = super::call_lean_navigation_start_parts(
                            event_loop_id,
                            request.source_navigable_id as usize,
                            &request.destination_url,
                            &request.target,
                            navigation_user_involvement_name(request.user_involvement.clone()),
                            request.noopener,
                        );
                        suppress_next_command_completed = true;
                    }
                    BridgeMode::Subframe {
                        traversable_id,
                        frame_id,
                    } => {
                        let local_target = request.target.is_empty() || request.target == "_self";
                        if local_target && !request.noopener && request.source_navigable_id == frame_id {
                            spawn_local_navigation(
                                Arc::clone(&bridge),
                                traversable_id,
                                frame_id,
                                request,
                            );
                        }
                    }
                },
                ContentEvent::AttachChildFrame(attach) => {
                    let _ = attach_child_frame(&bridge, attach);
                }
                ContentEvent::DetachChildFrame { frame_id } => {
                    let _ = detach_child_frame(&bridge, frame_id);
                }
                ContentEvent::BeforeUnloadCompleted(result) => {
                    if matches!(bridge.mode, BridgeMode::TopLevel { .. }) {
                        let _ = super::call_lean_before_unload_completed_parts(
                            result.document_id as usize,
                            result.check_id as usize,
                            result.canceled,
                        );
                    }
                }
                ContentEvent::FinalizeNavigation(finalized) => {
                    if matches!(bridge.mode, BridgeMode::TopLevel { .. }) {
                        let _ = super::call_lean_finalize_navigation_parts(
                            finalized.document_id as usize,
                            &finalized.url,
                        );
                    }
                }
                ContentEvent::CommandCompleted => {
                    if let BridgeMode::TopLevel { event_loop_id } = bridge.mode {
                        if suppress_next_command_completed {
                            suppress_next_command_completed = false;
                        } else {
                            let _ = super::call_lean_run_next_event_loop_task(event_loop_id);
                        }
                    }
                }
                ContentEvent::ScriptEvaluated(result) => {
                    let waiter = bridge
                        .script_waiters
                        .lock()
                        .expect("content script waiter mutex poisoned")
                        .remove(&result.request_id);
                    if let Some(waiter) = waiter {
                        let send_result = match result.error {
                            Some(error) => Err(error),
                            None => serde_json::from_str(&result.value_json).map_err(|error| {
                                format!(
                                    "failed to decode content script evaluation result: {error}"
                                )
                            }),
                        };
                        let _ = waiter.send(send_result);
                    }
                }
                ContentEvent::PaintReady(snapshot) => {
                    let _ = embedder::send_user_event(FormalWebUserEvent::Paint(snapshot));
                }
                ContentEvent::ShutdownCompleted => break,
            }
        }

        let waiters = bridge
            .script_waiters
            .lock()
            .expect("content script waiter mutex poisoned")
            .drain()
            .collect::<Vec<_>>();
        for (_request_id, waiter) in waiters {
            let _ = waiter.send(Err(String::from("content process event channel closed")));
        }
    })
}

fn send_command_inner(bridge: &ContentBridge, command: ContentCommand) -> Result<(), String> {
    bridge
        .command_sender
        .send(command)
        .map_err(|error| format!("failed to send content IPC message: {error}"))
}

pub fn install_hooks() {
    embedder::set_content_bridge_hooks(ContentBridgeHooks { broadcast_viewport });
}

pub fn start(event_loop_id: usize) -> Result<usize, String> {
    start_bridge(BridgeMode::TopLevel { event_loop_id }, true)
}

fn start_bridge(mode: BridgeMode, make_active: bool) -> Result<usize, String> {
    let executable_path = content_executable_path()?;
    let (server, token) = IpcOneShotServer::<Bootstrap>::new()
        .map_err(|error| format!("failed to create IPC one-shot server: {error}"))?;

    let mut child_process = Command::new(executable_path);
    setup_common(&mut child_process, &token);

    let child = child_process
        .spawn()
        .map_err(|error| format!("failed to start content: {error}"))?;
    let (_receiver, bootstrap) = server
        .accept()
        .map_err(|error| format!("failed to accept content bootstrap: {error}"))?;

    let bridge = Arc::new(ContentBridge {
        command_sender: bootstrap.command_sender,
        child: Mutex::new(Some(child)),
        listener: Mutex::new(None),
        mode,
        owned_subframes: Mutex::new(HashMap::new()),
        local_timer_cancellations: Mutex::new(HashMap::new()),
        script_waiters: Mutex::new(HashMap::new()),
    });
    let listener = spawn_listener(bootstrap.event_receiver, Arc::clone(&bridge));
    *bridge
        .listener
        .lock()
        .expect("content listener mutex poisoned") = Some(listener);

    if let Some(snapshot) = embedder::window_viewport_snapshot() {
        let _ = send_command_inner(&bridge, viewport_command(snapshot));
    }

    let handle = NEXT_CONTENT_HANDLE.fetch_add(1, Ordering::Relaxed);
    CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .insert(handle, Arc::clone(&bridge));
    if make_active {
        *ACTIVE_CONTENT_BRIDGE
            .lock()
            .expect("active content bridge mutex poisoned") = Some(bridge);
    }
    Ok(handle)
}

pub fn stop(handle: usize) -> Result<(), String> {
    let bridge = CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .remove(&handle)
        .ok_or_else(|| format!("unknown content handle: {handle}"))?;

    {
        let mut active = ACTIVE_CONTENT_BRIDGE
            .lock()
            .expect("active content bridge mutex poisoned");
        if active.as_ref().is_some_and(|candidate| Arc::ptr_eq(candidate, &bridge)) {
            active.take();
        }
    }

    clear_all_local_timers(&bridge);
    let child_handles = drain_owned_subframe_handles(&bridge);
    for child_handle in child_handles {
        let _ = stop(child_handle);
    }

    let _ = send_command_inner(&bridge, ContentCommand::Shutdown);
    let child = bridge
        .child
        .lock()
        .expect("content child mutex poisoned")
        .take();
    let listener = bridge
        .listener
        .lock()
        .expect("content listener mutex poisoned")
        .take();

    let waiters = bridge
        .script_waiters
        .lock()
        .expect("content script waiter mutex poisoned")
        .drain()
        .collect::<Vec<_>>();
    for (_request_id, waiter) in waiters {
        let _ = waiter.send(Err(String::from("content process stopped")));
    }

    finish_shutdown_async(child, listener);

    Ok(())
}

fn finish_shutdown_async(child: Option<Child>, listener: Option<JoinHandle<()>>) {
    thread::spawn(move || {
        finish_shutdown(child, listener);
    });
}

fn finish_shutdown(mut child: Option<Child>, listener: Option<JoinHandle<()>>) {
    if let Some(child) = child.as_mut() {
        match wait_for_child_exit(child, CONTENT_SHUTDOWN_GRACE_TIMEOUT) {
            Ok(true) => {}
            Ok(false) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Err(error) => {
                eprintln!("content bridge shutdown poll error: {error}");
            }
        }
    }

    if let Some(listener) = listener {
        let _ = listener.join();
    }
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Ok(true),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!("failed to poll content process exit: {error}"));
            }
        }
    }
}

pub fn send_command(handle: usize, command: ContentCommand) -> Result<(), String> {
    let bridge = CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .get(&handle)
        .cloned()
        .ok_or_else(|| format!("unknown content handle: {handle}"))?;
    send_command_inner(&bridge, command)
}

fn active_bridge() -> Result<Arc<ContentBridge>, String> {
    ACTIVE_CONTENT_BRIDGE
        .lock()
        .expect("active content bridge mutex poisoned")
        .as_ref()
        .cloned()
        .ok_or_else(|| String::from("no active content process"))
}

pub fn evaluate_script(
    document_id: u64,
    source: String,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let bridge = active_bridge()?;
    let request_id = NEXT_SCRIPT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = mpsc::channel();

    bridge
        .script_waiters
        .lock()
        .expect("content script waiter mutex poisoned")
        .insert(request_id, sender);

    if let Err(error) = send_command_inner(
        &bridge,
        ContentCommand::EvaluateScript {
            document_id,
            request_id,
            source,
        },
    ) {
        bridge
            .script_waiters
            .lock()
            .expect("content script waiter mutex poisoned")
            .remove(&request_id);
        return Err(error);
    }

    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            bridge
                .script_waiters
                .lock()
                .expect("content script waiter mutex poisoned")
                .remove(&request_id);
            Err(format!(
                "timed out waiting {} ms for script evaluation result",
                timeout.as_millis()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            bridge
                .script_waiters
                .lock()
                .expect("content script waiter mutex poisoned")
                .remove(&request_id);
            Err(String::from("content script evaluation channel disconnected"))
        }
    }
}

pub fn broadcast_viewport(snapshot: Option<(u32, u32, f32, ColorScheme)>) {
    let Some(snapshot) = snapshot else {
        return;
    };

    let command = viewport_command(snapshot);
    let bridges = CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for bridge in bridges {
        let _ = send_command_inner(&bridge, command.clone());
    }
}