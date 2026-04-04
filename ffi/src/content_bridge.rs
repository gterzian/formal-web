use blitz_traits::shell::ColorScheme;
use content_process_protocol::{
    ContentBootstrap, ContentColorScheme, ContentCommand, ContentEvent, ViewportSnapshot,
};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::{self, JoinHandle};

struct ContentProcessBridge {
    command_sender: IpcSender<ContentCommand>,
    child: Mutex<Option<Child>>,
    listener: Mutex<Option<JoinHandle<()>>>,
}

static CONTENT_PROCESS_REGISTRY: LazyLock<Mutex<HashMap<usize, Arc<ContentProcessBridge>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_CONTENT_PROCESS_HANDLE: AtomicUsize = AtomicUsize::new(1);

fn executable_file_name(stem: &str) -> String {
    if std::env::consts::EXE_EXTENSION.is_empty() {
        String::from(stem)
    } else {
        format!("{stem}.{}", std::env::consts::EXE_EXTENSION)
    }
}

fn content_process_executable_path() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| String::from("failed to resolve executable directory"))?;
    Ok(parent.join(executable_file_name("formalweb-content-process")))
}

fn setup_common(command: &mut Command, token: &str) {
    command.arg("--content-process-token").arg(token);
}

fn content_color_scheme(color_scheme: ColorScheme) -> ContentColorScheme {
    match color_scheme {
        ColorScheme::Light => ContentColorScheme::Light,
        ColorScheme::Dark => ContentColorScheme::Dark,
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

fn spawn_listener(event_receiver: ipc_channel::ipc::IpcReceiver<ContentEvent>) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            match event {
                ContentEvent::DocumentFetchRequested(request) => {
                    let _ = super::call_lean_document_fetch_start_parts(
                        request.handler_id as usize,
                        &request.url,
                        &request.method,
                        &request.body,
                    );
                }
                ContentEvent::PaintReady(snapshot) => {
                    super::with_event_loop_proxy(|proxy| {
                        if let Some(proxy) = proxy {
                            let _ = proxy.send_event(super::FormalWebUserEvent::Paint(snapshot));
                        }
                    });
                }
            }
        }
    })
}

fn send_command_inner(bridge: &ContentProcessBridge, command: ContentCommand) -> Result<(), String> {
    bridge
        .command_sender
        .send(command)
        .map_err(|error| format!("failed to send content-process IPC message: {error}"))
}

pub fn start(_event_loop_id: usize) -> Result<usize, String> {
    let executable_path = content_process_executable_path()?;
    let (server, token) =
        IpcOneShotServer::<ContentBootstrap>::new().map_err(|error| format!("failed to create IPC one-shot server: {error}"))?;

    let mut child_process = Command::new(executable_path);
    setup_common(&mut child_process, &token);

    let child = child_process
        .spawn()
        .map_err(|error| format!("failed to start content process: {error}"))?;
    let (_receiver, bootstrap) = server
        .accept()
        .map_err(|error| format!("failed to accept content-process bootstrap: {error}"))?;

    let listener = spawn_listener(bootstrap.event_receiver);
    let bridge = Arc::new(ContentProcessBridge {
        command_sender: bootstrap.command_sender,
        child: Mutex::new(Some(child)),
        listener: Mutex::new(Some(listener)),
    });

    if let Ok(snapshot_guard) = super::WINDOW_VIEWPORT_SNAPSHOT.lock() {
        if let Some(snapshot) = *snapshot_guard {
            let _ = send_command_inner(&bridge, viewport_command(snapshot));
        }
    }

    let handle = NEXT_CONTENT_PROCESS_HANDLE.fetch_add(1, Ordering::Relaxed);
    CONTENT_PROCESS_REGISTRY
        .lock()
        .expect("content-process registry mutex poisoned")
        .insert(handle, bridge);
    Ok(handle)
}

pub fn stop(handle: usize) -> Result<(), String> {
    let bridge = CONTENT_PROCESS_REGISTRY
        .lock()
        .expect("content-process registry mutex poisoned")
        .remove(&handle)
        .ok_or_else(|| format!("unknown content-process handle: {handle}"))?;

    let _ = send_command_inner(&bridge, ContentCommand::Shutdown);
    if let Some(listener) = bridge
        .listener
        .lock()
        .expect("content-process listener mutex poisoned")
        .take()
    {
        let _ = listener.join();
    }
    if let Some(mut child) = bridge
        .child
        .lock()
        .expect("content-process child mutex poisoned")
        .take()
    {
        let _ = child.wait();
    }
    Ok(())
}

pub fn send_command(handle: usize, command: ContentCommand) -> Result<(), String> {
    let bridge = CONTENT_PROCESS_REGISTRY
        .lock()
        .expect("content-process registry mutex poisoned")
        .get(&handle)
        .cloned()
        .ok_or_else(|| format!("unknown content-process handle: {handle}"))?;
    send_command_inner(&bridge, command)
}

pub fn broadcast_viewport(snapshot: Option<(u32, u32, f32, ColorScheme)>) {
    let Some(snapshot) = snapshot else {
        return;
    };
    let command = viewport_command(snapshot);
    let bridges = CONTENT_PROCESS_REGISTRY
        .lock()
        .expect("content-process registry mutex poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for bridge in bridges {
        let _ = send_command_inner(&bridge, command.clone());
    }
}
