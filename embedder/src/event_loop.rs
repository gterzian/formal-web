#[path = "headless.rs"]
mod headless;
#[path = "windowed.rs"]
mod windowed;
#[path = "winit_integration.rs"]
mod winit_integration;

use self::headless::HeadlessEmbedderApp;
use self::windowed::WindowedApp;
use self::winit_integration::UserEventDispatcher;
pub use self::winit_integration::{
    EventLoopOptions, clear_event_loop_options, set_event_loop_options,
};
use anyrender::{PaintScene, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use automation::{AutomationCommand, AutomationVisibleFrameViewport};
use blitz_traits::shell::ColorScheme;
use ipc_messages::content::WebviewId;
use kurbo::{Affine, Rect};
use log::error;
use peniko::{Color, Fill};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::Duration;
use verification::TraceSender;
use webview::{Embedder, WebviewProvider};
use winit::application::ApplicationHandler;
use winit::event_loop::{EventLoop, EventLoopProxy};

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationCompletion {
    Committed { url: String },
    Aborted { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationCompleted {
    pub webview_id: WebviewId,
    pub status: NavigationCompletion,
}

struct EventLoopEmbedder {
    dispatcher: UserEventDispatcher,
}

impl EventLoopEmbedder {
    fn new(dispatcher: UserEventDispatcher) -> Self {
        Self { dispatcher }
    }
}

impl Embedder for EventLoopEmbedder {
    fn navigation_requested(
        &self,
        webview_id: WebviewId,
        destination_url: String,
    ) -> Result<(), String> {
        self.dispatcher
            .send(FormalWebUserEvent::NavigationRequested {
                webview_id,
                destination_url,
            })
    }

    fn navigation_completed(&self, completed: webview::NavigationCompleted) -> Result<(), String> {
        let status = match completed.status {
            webview::NavigationCompletion::Committed { url } => {
                NavigationCompletion::Committed { url }
            }
            webview::NavigationCompletion::Aborted { message } => {
                NavigationCompletion::Aborted { message }
            }
        };
        self.dispatcher
            .send(FormalWebUserEvent::NavigationCompleted(
                NavigationCompleted {
                    webview_id: completed.webview_id,
                    status,
                },
            ))
    }

    fn new_webview(&self, webview_id: WebviewId, target_name: String) -> Result<(), String> {
        log::debug!(
            "[embedder] Embedder::new_webview webview={:?} target={}",
            webview_id,
            target_name
        );
        self.dispatcher
            .send(FormalWebUserEvent::NewWebview(webview_id, target_name))
    }

    fn webview_provider_sync(&self) -> Result<(), String> {
        self.dispatcher
            .send(FormalWebUserEvent::WebviewProviderSync)
    }

    fn new_frame_rendered(&self) -> Result<(), String> {
        self.dispatcher.send(FormalWebUserEvent::NewFrameRendered)
    }

    fn request_redraw(&self, webview_id: WebviewId) {
        if let Err(error) = self
            .dispatcher
            .send(FormalWebUserEvent::RequestRedraw(webview_id))
        {
            error!("failed to request redraw for webview {webview_id:?}: {error}");
        }
    }

    fn viewport_scale_factor(&self) -> f32 {
        window_viewport_snapshot()
            .map(|(_, _, scale, _)| scale)
            .unwrap_or(1.0)
    }

    fn window_viewport_snapshot(&self) -> Option<(u32, u32, f32, ColorScheme)> {
        window_viewport_snapshot()
    }

    fn clipboard_get_text(&self, timeout: Duration) -> Result<String, String> {
        clipboard_get_text(timeout)
    }

    fn clipboard_set_text(&self, text: String, timeout: Duration) -> Result<(), String> {
        clipboard_set_text(text, timeout)
    }

    fn new_web_content_scene(
        &self,
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        font_data: std::collections::HashMap<usize, Vec<u8>>,
    ) -> Result<(), String> {
        self.dispatcher
            .send(FormalWebUserEvent::NewWebContentScene {
                webview_id,
                scene_bytes,
                font_registrations,
                font_data,
            })
    }

    fn new_web_content_surface(
        &self,
        webview_id: WebviewId,
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        generation: u64,
    ) -> Result<(), String> {
        self.dispatcher
            .send(FormalWebUserEvent::NewWebContentSurface {
                webview_id,
                pixels,
                width,
                height,
                generation,
            })
    }
}

pub enum FormalWebUserEvent {
    RequestRedraw(WebviewId),
    NewWebContentScene {
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        font_data: std::collections::HashMap<usize, Vec<u8>>,
    },
    NewWebContentSurface {
        webview_id: WebviewId,
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        generation: u64,
    },
    NavigationRequested {
        webview_id: WebviewId,
        destination_url: String,
    },
    NavigationCompleted(NavigationCompleted),
    #[allow(dead_code)]
    NewWebview(WebviewId, String),
    WebviewProviderSync,
    NewFrameRendered,
    CreateWindow,
    Automation(AutomationCommand),
    ClipboardRead {
        reply: mpsc::Sender<Result<String, String>>,
    },
    ClipboardWrite {
        text: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Exit,
}

static EVENT_LOOP_PROXY: LazyLock<Mutex<Option<EventLoopProxy<FormalWebUserEvent>>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn send_user_event(event: FormalWebUserEvent) -> Result<(), String> {
    with_event_loop_proxy(|proxy| match proxy {
        Some(proxy) => proxy
            .send_event(event)
            .map_err(|error| format!("failed to send user event: {error}")),
        None => Err(String::from("winit event loop proxy is not initialized")),
    })
}

fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("failed to access clipboard: {error}"))?;
    clipboard
        .get_text()
        .map_err(|error| format!("failed to read clipboard text: {error}"))
}

fn write_clipboard_text(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("failed to access clipboard: {error}"))?;
    clipboard
        .set_text(text)
        .map_err(|error| format!("failed to write clipboard text: {error}"))
}

pub fn clipboard_get_text(timeout: Duration) -> Result<String, String> {
    let (reply, receiver) = mpsc::channel();
    send_user_event(FormalWebUserEvent::ClipboardRead { reply })?;
    receiver.recv_timeout(timeout).map_err(|error| {
        format!(
            "timed out after {} ms waiting for clipboard text: {error}",
            timeout.as_millis()
        )
    })?
}

pub fn clipboard_set_text(text: String, timeout: Duration) -> Result<(), String> {
    let (reply, receiver) = mpsc::channel();
    send_user_event(FormalWebUserEvent::ClipboardWrite { text, reply })?;
    receiver.recv_timeout(timeout).map_err(|error| {
        format!(
            "timed out after {} ms waiting to write clipboard text: {error}",
            timeout.as_millis()
        )
    })?
}

pub fn event_loop_is_ready() -> bool {
    with_event_loop_proxy(|proxy| proxy.is_some())
}

fn run_embedder_event_loop<A, MakeApp>(
    trace_sender: Option<TraceSender>,
    make_app: MakeApp,
) -> Result<(), String>
where
    A: ApplicationHandler<FormalWebUserEvent>,
    MakeApp: FnOnce(WebviewProvider) -> A,
{
    let event_loop = EventLoop::<FormalWebUserEvent>::with_user_event()
        .build()
        .map_err(|error| format!("failed to create event loop: {error}"))?;
    let dispatcher = UserEventDispatcher::new(event_loop.create_proxy());
    {
        let mut guard = EVENT_LOOP_PROXY
            .lock()
            .expect("event loop proxy mutex poisoned");
        *guard = Some(dispatcher.proxy.clone());
    }

    let event_loop_embedder = Arc::new(EventLoopEmbedder::new(dispatcher));
    let embedder: Arc<dyn Embedder> = event_loop_embedder.clone();
    let provider = match WebviewProvider::new(embedder, trace_sender) {
        Ok(provider) => provider,
        Err(error) => {
            let mut guard = EVENT_LOOP_PROXY
                .lock()
                .expect("event loop proxy mutex poisoned");
            guard.take();
            update_window_viewport_snapshot(None);
            return Err(error);
        }
    };

    let mut app = make_app(provider);
    let run_result = event_loop
        .run_app(&mut app)
        .map_err(|error| format!("winit event loop failed: {error}"));

    {
        let mut guard = EVENT_LOOP_PROXY
            .lock()
            .expect("event loop proxy mutex poisoned");
        guard.take();
    }
    update_window_viewport_snapshot(None);

    run_result
}

pub fn run_headed_event_loop(trace_sender: Option<TraceSender>) -> Result<(), String> {
    run_embedder_event_loop(trace_sender, |provider| WindowedApp {
        provider: Some(provider),
        ..WindowedApp::default()
    })
}

pub fn run_headless_event_loop(trace_sender: Option<TraceSender>) -> Result<(), String> {
    run_embedder_event_loop(trace_sender, |provider| HeadlessEmbedderApp {
        provider: Some(provider),
        ..HeadlessEmbedderApp::default()
    })
}

pub fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    windowed::window_viewport_snapshot()
}

fn update_window_viewport_snapshot(snapshot: Option<(u32, u32, f32, ColorScheme)>) {
    windowed::update_window_viewport_snapshot(snapshot);
}

fn automation_screenshot_png(
    provider: &mut Option<WebviewProvider>,
    current_webview_id: Option<WebviewId>,
) -> Result<Vec<u8>, String> {
    let Some((width, height, _, _)) = window_viewport_snapshot() else {
        return Err(String::from("content viewport is not initialized"));
    };
    if width == 0 || height == 0 {
        return Err(String::from("content viewport is zero-sized"));
    }

    let Some(provider) = provider.as_mut() else {
        return Err(String::from("webview provider is not initialized"));
    };
    let Some(webview_id) = current_webview_id else {
        return Err(String::from("no current webview is active"));
    };

    let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
        |painter| {
            painter.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::WHITE,
                None,
                &Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
            );
            let _ = provider.append_web_content_scene(webview_id, painter, Affine::IDENTITY);
        },
        width,
        height,
    );

    encode_png_rgba(&rgba, width, height)
}

fn automation_visible_frame_viewports(
    provider: &mut Option<WebviewProvider>,
    current_webview_id: Option<WebviewId>,
) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
    let Some(provider) = provider.as_mut() else {
        return Err(String::from("webview provider is not initialized"));
    };
    let Some(webview_id) = current_webview_id else {
        return Err(String::from("no current webview is active"));
    };

    Ok(provider
        .visible_frame_viewports(webview_id)
        .into_iter()
        .map(|viewport| AutomationVisibleFrameViewport {
            offset_x: viewport.offset_x,
            offset_y: viewport.offset_y,
            width: viewport.width,
            height: viewport.height,
        })
        .collect())
}

fn encode_png_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut png_data = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_data, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("failed to encode screenshot header: {error}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|error| format!("failed to encode screenshot pixels: {error}"))?;
    drop(writer);
    Ok(png_data)
}

fn startup_destination_url(startup_url: Option<&str>) -> Result<String, String> {
    match startup_url {
        Some(url) => Ok(url.to_owned()),
        None => startup_artifact_url(),
    }
}

fn startup_artifact_url() -> Result<String, String> {
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    // Try CWD-relative path first, then parent directory (for running from embedder/).
    for base in [current_dir.clone(), current_dir.join("..")] {
        let artifact_path = base.join(STARTUP_ARTIFACT_RELATIVE_PATH);
        if let Ok(canonical) = artifact_path.canonicalize() {
            return Ok(format!("file://{}", canonical.display()));
        }
    }
    Err(format!(
        "startup artifact not found at {} or ../{}",
        STARTUP_ARTIFACT_RELATIVE_PATH, STARTUP_ARTIFACT_RELATIVE_PATH
    ))
}

fn normalize_browser_destination(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") || trimmed.starts_with("about:") {
        return Some(trimmed.to_owned());
    }
    Some(format!("https://{trimmed}"))
}

pub(crate) fn with_event_loop_proxy<R>(
    f: impl FnOnce(&Option<EventLoopProxy<FormalWebUserEvent>>) -> R,
) -> R {
    let guard = EVENT_LOOP_PROXY
        .lock()
        .expect("event loop proxy mutex poisoned");
    f(&guard)
}
