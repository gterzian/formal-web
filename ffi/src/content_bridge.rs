use blitz_traits::shell::ColorScheme;
use embedder::{ContentBridgeHooks, FormalWebUserEvent};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_messages::content::{
    Bootstrap, ColorScheme as MessageColorScheme, Command as ContentCommand,
    Event as ContentEvent, UserNavigationInvolvement, ViewportSnapshot,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

fn iframe_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_IFRAMES").is_some()
}

fn log_iframe_debug(message: impl AsRef<str>) {
    if iframe_debug_enabled() {
        eprintln!(
            "[iframe-debug][bridge][pid={}] {}",
            std::process::id(),
            message.as_ref()
        );
    }
}

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
    event_loop_id: usize,
    script_waiters: Mutex<HashMap<u64, mpsc::Sender<Result<serde_json::Value, String>>>>,
}

static CONTENT_REGISTRY: LazyLock<Mutex<HashMap<usize, Arc<ContentBridge>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE_CONTENT_BRIDGE: LazyLock<Mutex<Option<Arc<ContentBridge>>>> =
    LazyLock::new(|| Mutex::new(None));
static TRAVERSABLE_BRIDGE_REGISTRY: LazyLock<Mutex<HashMap<u64, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_CONTENT_HANDLE: AtomicUsize = AtomicUsize::new(1);
static NEXT_SCRIPT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

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
                ContentEvent::DocumentFetchRequested(request) => {
                    log_iframe_debug(format!(
                        "forward_fetch_request event_loop={} handler={} method={} url={}",
                        bridge.event_loop_id,
                        request.handler_id,
                        request.method,
                        request.url
                    ));
                    let _ = super::call_lean_document_fetch_start_parts(
                        bridge.event_loop_id,
                        request.handler_id as usize,
                        &request.url,
                        &request.method,
                        &request.body,
                    );
                }
                ContentEvent::WindowTimerRequested(request) => {
                    log_timer_debug(format!(
                        "forward schedule document={} id={} key={} timeout_ms={} nesting={}",
                        request.document_id,
                        request.timer_id,
                        request.timer_key,
                        request.timeout_ms,
                        request.nesting_level
                    ));
                    let _ = super::call_lean_schedule_window_timer_parts(
                        bridge.event_loop_id,
                        request.document_id as usize,
                        request.timer_id as usize,
                        request.timer_key as usize,
                        request.timeout_ms as usize,
                        request.nesting_level as usize,
                    );
                }
                ContentEvent::WindowTimerCleared(request) => {
                    log_timer_debug(format!(
                        "forward clear document={} key={}",
                        request.document_id, request.timer_key
                    ));
                    let _ = request.document_id;
                    let _ = super::call_lean_clear_window_timer_parts(
                        bridge.event_loop_id,
                        request.timer_key as usize,
                    );
                }
                ContentEvent::NavigationRequested(request) => {
                    log_iframe_debug(format!(
                        "forward_navigation event_loop={} source={} target={} noopener={} destination={}",
                        bridge.event_loop_id,
                        request.source_navigable_id,
                        request.target,
                        request.noopener,
                        request.destination_url
                    ));
                    let _ = super::call_lean_navigation_start_from_event_loop_parts(
                        bridge.event_loop_id,
                        request.source_navigable_id as usize,
                        &request.destination_url,
                        &request.target,
                        navigation_user_involvement_name(request.user_involvement.clone()),
                        request.noopener,
                    );
                    suppress_next_command_completed = true;
                }
                ContentEvent::BeforeUnloadCompleted(result) => {
                    let _ = super::call_lean_before_unload_completed_parts(
                        result.document_id as usize,
                        result.check_id as usize,
                        result.canceled,
                    );
                }
                ContentEvent::FinalizeNavigation(finalized) => {
                    let _ = super::call_lean_finalize_navigation_parts(
                        finalized.document_id as usize,
                        &finalized.url,
                    );
                }
                ContentEvent::IframeTraversableRemoved(removal) => {
                    log_iframe_debug(format!(
                        "forward_iframe_removal parent_traversable={} source_navigable={}",
                        removal.parent_traversable_id,
                        removal.source_navigable_id
                    ));
                    let _ = super::call_lean_remove_iframe_traversable_parts(
                        removal.parent_traversable_id as usize,
                        removal.source_navigable_id as usize,
                    );
                }
                ContentEvent::ChildNavigableCreated(creation) => {
                    log_iframe_debug(format!(
                        "forward_child_navigable_created parent_traversable={} source_navigable={}",
                        creation.parent_traversable_id,
                        creation.source_navigable_id
                    ));
                    let _ = super::call_lean_child_navigable_created_parts(
                        creation.parent_traversable_id as usize,
                        creation.source_navigable_id as usize,
                    );
                }
                ContentEvent::CommandCompleted => {
                    if suppress_next_command_completed {
                        suppress_next_command_completed = false;
                    } else {
                        let _ = super::call_lean_run_next_event_loop_task(bridge.event_loop_id);
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
                    let transport = snapshot.transport_summary();
                    log_iframe_debug(format!(
                        "paint_ready event_loop={} traversable={} frame={} viewport={}x{} scroll=({:.1}, {:.1}) transport(scene_bytes={} fonts={} font_bytes={})",
                        bridge.event_loop_id,
                        snapshot.traversable_id.0,
                        snapshot.frame_id.0,
                        snapshot.viewport_width,
                        snapshot.viewport_height,
                        snapshot.viewport_scroll.x,
                        snapshot.viewport_scroll.y,
                        transport.scene_bytes,
                        transport.registered_fonts,
                        transport.registered_font_bytes,
                    ));
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
        .send(command.clone())
        .map_err(|error| format!("failed to send content IPC message: {error}"))?;

    match command {
        ContentCommand::CreateEmptyDocument {
            traversable_id,
            document_id: _,
        }
        | ContentCommand::CreateLoadedDocument {
            traversable_id,
            document_id: _,
            ..
        } => {
            TRAVERSABLE_BRIDGE_REGISTRY
                .lock()
                .expect("content traversable registry mutex poisoned")
                .insert(traversable_id, bridge.event_loop_id);
        }
        _ => {}
    }

    Ok(())
}

pub fn install_hooks() {
    embedder::set_content_bridge_hooks(ContentBridgeHooks { broadcast_viewport });
}

pub fn start(event_loop_id: usize) -> Result<usize, String> {
    start_bridge(event_loop_id, true)
}

fn start_bridge(event_loop_id: usize, make_active: bool) -> Result<usize, String> {
    let executable_path = content_executable_path()?;
    let (server, token) = IpcOneShotServer::<Bootstrap>::new()
        .map_err(|error| format!("failed to create IPC one-shot server: {error}"))?;

    let mut child_process = Command::new(&executable_path);
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
        event_loop_id,
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
    log_iframe_debug(format!(
        "start_bridge handle={} event_loop={} make_active={} executable={}",
        handle,
        event_loop_id,
        make_active,
        executable_path.display()
    ));
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
        .ok_or(())
        .ok();

    let Some(bridge) = bridge else {
        return Ok(());
    };

    {
        let mut active = ACTIVE_CONTENT_BRIDGE
            .lock()
            .expect("active content bridge mutex poisoned");
        if active.as_ref().is_some_and(|candidate| Arc::ptr_eq(candidate, &bridge)) {
            active.take();
        }
    }

    let removed_traversable_ids = {
        let mut traversable_bridge_registry = TRAVERSABLE_BRIDGE_REGISTRY
            .lock()
            .expect("content traversable registry mutex poisoned");
        let removed_traversable_ids = traversable_bridge_registry
            .iter()
            .filter_map(|(traversable_id, owner_event_loop_id)| {
                (*owner_event_loop_id == bridge.event_loop_id).then_some(*traversable_id)
            })
            .collect::<Vec<_>>();
        traversable_bridge_registry.retain(|_, owner_event_loop_id| *owner_event_loop_id != bridge.event_loop_id);
        removed_traversable_ids
    };
    TRAVERSABLE_BRIDGE_REGISTRY
        .lock()
        .expect("content traversable registry mutex poisoned")
        .retain(|traversable_id, _| !removed_traversable_ids.contains(traversable_id));

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

pub fn stop_event_loop(event_loop_id: usize) -> Result<(), String> {
    let handle = CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .iter()
        .find_map(|(handle, bridge)| (bridge.event_loop_id == event_loop_id).then_some(*handle));
    match handle {
        Some(handle) => stop(handle),
        None => Ok(()),
    }
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

fn bridge_for_traversable_id(traversable_id: u64) -> Result<Arc<ContentBridge>, String> {
    let event_loop_id = TRAVERSABLE_BRIDGE_REGISTRY
        .lock()
        .expect("content traversable registry mutex poisoned")
        .get(&traversable_id)
        .copied()
        .ok_or_else(|| format!("no content process owns traversable {traversable_id}"))?;

    let bridge = CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .values()
        .find(|bridge| bridge.event_loop_id == event_loop_id)
        .cloned()
        .ok_or_else(|| {
            format!(
                "no content bridge found for event loop {} owning traversable {}",
                event_loop_id, traversable_id
            )
        })?;

    Ok(bridge)
}

pub fn evaluate_script(
    traversable_id: u64,
    source: String,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let bridge = bridge_for_traversable_id(traversable_id)?;
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
            traversable_id,
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
    let (width, height, scale, _color_scheme) = snapshot;
    let bridges = CONTENT_REGISTRY
        .lock()
        .expect("content registry mutex poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    log_iframe_debug(format!(
        "broadcast_viewport width={} height={} scale={} bridge_count={}",
        width,
        height,
        scale,
        bridges.len()
    ));
    for bridge in bridges {
        let _ = send_command_inner(&bridge, command.clone());
    }
}