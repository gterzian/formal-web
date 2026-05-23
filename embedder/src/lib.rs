mod chrome;
mod headless;
mod windowed;
pub use webview::ui_event;

use crate::chrome::{ChromeAction, ChromeUi, ChromeViewState};
use crate::headless::HeadlessEmbedderApp;
use automation::{
    AutomationCommand, AutomationController, AutomationHost, AutomationSnapshot,
    AutomationVisibleFrameViewport,
};
use anyrender::{PaintScene, WindowRenderer, render_to_buffer};
use anyrender_vello::VelloWindowRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use cursor_icon::CursorIcon;
use ipc_messages::content::{NavigableId, PaintFrame, WebviewId};
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use kurbo::{Affine, Rect};
use peniko::{Color, Fill};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};
use verification::TraceSender;
use webview::{Embedder, EmbedderMsg, WebviewProvider};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{
    ElementState, Ime, KeyEvent as WinitKeyEvent, Modifiers, MouseButton, MouseScrollDelta,
    Touch, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{
    Key as WinitKey, KeyCode as WinitKeyCode, KeyLocation as WinitKeyLocation,
    ModifiersState as WinitModifiersState, NamedKey, PhysicalKey,
};
use winit::window::{Cursor, Window, WindowAttributes, WindowId};

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";
#[derive(Clone, Default)]
pub struct EventLoopOptions {
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
}

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

#[derive(Clone)]
pub struct UserEventDispatcher {
    proxy: EventLoopProxy<FormalWebUserEvent>,
}

impl UserEventDispatcher {
    fn new(proxy: EventLoopProxy<FormalWebUserEvent>) -> Self {
        Self { proxy }
    }

    pub fn send(&self, event: FormalWebUserEvent) -> Result<(), String> {
        self.proxy
            .send_event(event)
            .map_err(|error| format!("failed to send user event: {error}"))
    }
}

struct EventLoopEmbedder {
    dispatcher: UserEventDispatcher,
}

struct WinitShellProvider {
    window: Arc<Window>,
}

impl WinitShellProvider {
    fn new(window: Arc<Window>) -> Self {
        Self { window }
    }
}

fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to access clipboard: {error}"))?;
    clipboard
        .get_text()
        .map_err(|error| format!("failed to read clipboard text: {error}"))
}

fn write_clipboard_text(text: String) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to access clipboard: {error}"))?;
    clipboard
        .set_text(text)
        .map_err(|error| format!("failed to write clipboard text: {error}"))
}

impl ShellProvider for WinitShellProvider {
    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn set_cursor(&self, icon: CursorIcon) {
        self.window.set_cursor(Cursor::Icon(icon));
    }

    fn set_window_title(&self, title: String) {
        self.window.set_title(&title);
    }

    fn set_ime_enabled(&self, is_enabled: bool) {
        self.window.set_ime_allowed(is_enabled);
    }

    fn set_ime_cursor_area(&self, x: f32, y: f32, width: f32, height: f32) {
        self.window
            .set_ime_cursor_area(LogicalPosition::new(x, y), LogicalSize::new(width, height));
    }

    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        read_clipboard_text().map_err(|_| ClipboardError)
    }

    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        write_clipboard_text(text).map_err(|_| ClipboardError)
    }
}

impl Embedder for EventLoopEmbedder {
    fn send_msg(&self, msg: EmbedderMsg) -> Result<(), String> {
        let event = match msg {
            EmbedderMsg::Paint(snapshot) => FormalWebUserEvent::Paint(snapshot),
            EmbedderMsg::NavigationRequested {
                webview_id,
                destination_url,
            } => FormalWebUserEvent::NavigationRequested {
                webview_id,
                destination_url,
            },
            EmbedderMsg::NavigationCompleted(completed) => {
                let status = match completed.status {
                    webview::NavigationCompletion::Committed { url } => {
                        NavigationCompletion::Committed { url }
                    }
                    webview::NavigationCompletion::Aborted { message } => {
                        NavigationCompletion::Aborted { message }
                    }
                };
                FormalWebUserEvent::NavigationCompleted(NavigationCompleted {
                    webview_id: completed.webview_id,
                    status,
                })
            }
            EmbedderMsg::NewTopLevelTraversable(webview_id, target_name) => {
                FormalWebUserEvent::NewTopLevelTraversable(webview_id, target_name)
            }
        };
        self.dispatcher.send(event)
    }

    fn request_redraw(&self, webview_id: WebviewId) {
        let _ = self
            .dispatcher
            .send(FormalWebUserEvent::RequestRedraw(webview_id));
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
}

pub enum FormalWebUserEvent {
    Paint(PaintFrame),
    RequestRedraw(WebviewId),
    NavigationRequested { webview_id: WebviewId, destination_url: String },
    NavigationCompleted(NavigationCompleted),
    NewTopLevelTraversable(WebviewId, String),
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingNavigation {
    url: String,
}

#[derive(Clone, Copy)]
struct ChildNavigableHostTarget {
    parent_traversable_id: WebviewId,
    content_frame_id: ipc_messages::content::FrameId,
}

#[derive(Default)]
struct BrowserState {
    history: Vec<String>,
    history_index: Option<usize>,
    pending_navigation: Option<PendingNavigation>,
    current_navigable_id: Option<NavigableId>,
}

impl BrowserState {
    fn displayed_url(&self) -> String {
        self.pending_navigation
            .as_ref()
            .map(|pending| pending.url.clone())
            .or_else(|| self.current_url().map(ToOwned::to_owned))
            .unwrap_or_default()
    }

    fn current_url(&self) -> Option<&str> {
        self.history_index
            .and_then(|index| self.history.get(index).map(String::as_str))
    }

    fn begin_navigation(&mut self, pending_navigation: PendingNavigation) {
        self.pending_navigation = Some(pending_navigation);
    }

    fn cancel_pending_navigation(&mut self) {
        self.pending_navigation = None;
    }

    fn commit_navigation(&mut self, url: String) {
        self.pending_navigation.take();

        if let Some(index) = self.history_index {
            if self.history.get(index).is_some_and(|current| current == &url) {
                self.history[index] = url;
            } else {
                self.history.truncate(index + 1);
                self.history.push(url);
                self.history_index = Some(self.history.len() - 1);
            }
        } else {
            self.history.push(url);
            self.history_index = Some(0);
        }
    }

    fn automation_snapshot(
        &self,
        current_webview_id: Option<WebviewId>,
        has_top_level_traversable: bool,
    ) -> AutomationSnapshot {
        AutomationSnapshot {
            webview_id: current_webview_id,
            current_url: self.current_url().map(ToOwned::to_owned),
            displayed_url: self.displayed_url(),
            navigable_id: self.current_navigable_id,
            has_top_level_traversable,
        }
    }

    fn set_current_navigable_id(&mut self, navigable_id: Option<NavigableId>) {
        self.current_navigable_id = navigable_id;
    }
}

struct HeadedEmbedderApp {
    window: Option<Arc<Window>>,
    renderer: VelloWindowRenderer,
    chrome: Option<ChromeUi>,
    browser: BrowserState,
    automation: AutomationController,
    provider: Option<WebviewProvider>,
    current_webview_id: Option<WebviewId>,
    has_top_level_traversable: bool,
    window_occluded: bool,
    animation_timer: Option<Instant>,
    keyboard_modifiers: Modifiers,
    buttons: MouseEventButtons,
    pointer_pos: PhysicalPosition<f64>,
}

static EVENT_LOOP_PROXY: LazyLock<Mutex<Option<EventLoopProxy<FormalWebUserEvent>>>> =
    LazyLock::new(|| Mutex::new(None));
pub(crate) static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<Option<(u32, u32, f32, ColorScheme)>>> =
    LazyLock::new(|| Mutex::new(None));
static EVENT_LOOP_OPTIONS: LazyLock<Mutex<EventLoopOptions>> =
    LazyLock::new(|| Mutex::new(EventLoopOptions::default()));

fn parse_child_navigable_host_target(target_name: &str) -> Option<ChildNavigableHostTarget> {
    let (parent_traversable_id, _content_navigable_id, content_frame_id) =
        ipc_messages::content::parse_iframe_target_name(target_name)?;

    Some(ChildNavigableHostTarget {
        parent_traversable_id: WebviewId(parent_traversable_id),
        content_frame_id,
    })
}

pub fn set_event_loop_options(options: EventLoopOptions) {
    let mut guard = EVENT_LOOP_OPTIONS
        .lock()
        .expect("event loop options mutex poisoned");
    *guard = options;
}

pub fn clear_event_loop_options() {
    let mut options = EVENT_LOOP_OPTIONS
        .lock()
        .expect("event loop options mutex poisoned");
    *options = EventLoopOptions::default();
}

fn event_loop_options() -> EventLoopOptions {
    EVENT_LOOP_OPTIONS
        .lock()
        .expect("event loop options mutex poisoned")
        .clone()
}

pub fn send_user_event(event: FormalWebUserEvent) -> Result<(), String> {
    with_event_loop_proxy(|proxy| match proxy {
        Some(proxy) => proxy
            .send_event(event)
            .map_err(|error| format!("failed to send user event: {error}")),
        None => Err(String::from("winit event loop proxy is not initialized")),
    })
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

    let embedder: Arc<dyn Embedder> = Arc::new(EventLoopEmbedder { dispatcher });
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
    run_embedder_event_loop(trace_sender, |provider| HeadedEmbedderApp {
        provider: Some(provider),
        ..HeadedEmbedderApp::default()
    })
}

pub fn run_headless_event_loop(trace_sender: Option<TraceSender>) -> Result<(), String> {
    run_embedder_event_loop(trace_sender, |provider| HeadlessEmbedderApp {
        provider: Some(provider),
        ..HeadlessEmbedderApp::default()
    })
}

pub fn run_event_loop(trace_sender: Option<TraceSender>) -> Result<(), String> {
    run_headed_event_loop(trace_sender)
}

pub fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned")
        .as_ref()
        .copied()
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
    let artifact_path: PathBuf = current_dir.join(STARTUP_ARTIFACT_RELATIVE_PATH);
    let artifact_path = artifact_path
        .canonicalize()
        .map_err(|error| format!("failed to resolve startup artifact path: {error}"))?;
    Ok(format!("file://{}", artifact_path.display()))
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

fn theme_to_color_scheme(theme: winit::window::Theme) -> ColorScheme {
    match theme {
        winit::window::Theme::Light => ColorScheme::Light,
        winit::window::Theme::Dark => ColorScheme::Dark,
    }
}

fn update_window_viewport_snapshot(snapshot: Option<(u32, u32, f32, ColorScheme)>) {
    let mut guard = WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned");
    *guard = snapshot;
}

fn viewport_snapshot_for_window(window: &Window) -> (u32, u32, f32, ColorScheme) {
    let size = window.inner_size();
    let scale = window.scale_factor() as f32;
    let color_scheme = theme_to_color_scheme(window.theme().unwrap_or(winit::window::Theme::Light));
    (size.width, size.height, scale, color_scheme)
}

fn viewport_of_snapshot(snapshot: (u32, u32, f32, ColorScheme)) -> Viewport {
    let (width, height, scale, color_scheme) = snapshot;
    Viewport::new(width, height, scale, color_scheme)
}

fn winit_ime_to_blitz(event: Ime) -> BlitzImeEvent {
    match event {
        Ime::Enabled => BlitzImeEvent::Enabled,
        Ime::Disabled => BlitzImeEvent::Disabled,
        Ime::Preedit(text, cursor) => BlitzImeEvent::Preedit(text, cursor),
        Ime::Commit(text) => BlitzImeEvent::Commit(text),
    }
}

fn touch_pointer_details(force: Option<winit::event::Force>) -> PointerDetails {
    PointerDetails {
        pressure: force.map(|value| value.normalized()).unwrap_or(0.0),
        ..PointerDetails::default()
    }
}

fn winit_modifiers_to_kbt_modifiers(winit_modifiers: WinitModifiersState) -> KeyboardModifiers {
    let mut modifiers = KeyboardModifiers::default();
    if winit_modifiers.contains(WinitModifiersState::CONTROL) {
        modifiers.insert(KeyboardModifiers::CONTROL);
    }
    if winit_modifiers.contains(WinitModifiersState::ALT) {
        modifiers.insert(KeyboardModifiers::ALT);
    }
    if winit_modifiers.contains(WinitModifiersState::SHIFT) {
        modifiers.insert(KeyboardModifiers::SHIFT);
    }
    if winit_modifiers.contains(WinitModifiersState::SUPER) {
        modifiers.insert(KeyboardModifiers::SUPER);
    }
    modifiers
}

fn winit_key_location_to_kbt_location(location: WinitKeyLocation) -> Location {
    match location {
        WinitKeyLocation::Standard => Location::Standard,
        WinitKeyLocation::Left => Location::Left,
        WinitKeyLocation::Right => Location::Right,
        WinitKeyLocation::Numpad => Location::Numpad,
    }
}

fn winit_key_to_kbt_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Character(value) => Key::Character(value.to_string()),
        WinitKey::Named(named) => match named {
            NamedKey::Alt => Key::Alt,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Control => Key::Control,
            NamedKey::Delete => Key::Delete,
            NamedKey::ArrowDown => Key::ArrowDown,
            NamedKey::End => Key::End,
            NamedKey::Enter => Key::Enter,
            NamedKey::Escape => Key::Escape,
            NamedKey::Home => Key::Home,
            NamedKey::ArrowLeft => Key::ArrowLeft,
            NamedKey::Meta => Key::Meta,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::ArrowRight => Key::ArrowRight,
            NamedKey::Shift => Key::Shift,
            NamedKey::Space => Key::Character(" ".to_owned()),
            NamedKey::Tab => Key::Tab,
            NamedKey::ArrowUp => Key::ArrowUp,
            NamedKey::Super => Key::Super,
            _ => Key::Unidentified,
        },
        _ => Key::Unidentified,
    }
}

fn winit_physical_key_to_kbt_code(physical_key: &PhysicalKey) -> Code {
    match physical_key {
        PhysicalKey::Code(code) => match code {
            WinitKeyCode::Backquote => Code::Backquote,
            WinitKeyCode::Backslash => Code::Backslash,
            WinitKeyCode::Backspace => Code::Backspace,
            WinitKeyCode::BracketLeft => Code::BracketLeft,
            WinitKeyCode::BracketRight => Code::BracketRight,
            WinitKeyCode::Comma => Code::Comma,
            WinitKeyCode::ControlLeft => Code::ControlLeft,
            WinitKeyCode::ControlRight => Code::ControlRight,
            WinitKeyCode::Delete => Code::Delete,
            WinitKeyCode::Digit0 => Code::Digit0,
            WinitKeyCode::Digit1 => Code::Digit1,
            WinitKeyCode::Digit2 => Code::Digit2,
            WinitKeyCode::Digit3 => Code::Digit3,
            WinitKeyCode::Digit4 => Code::Digit4,
            WinitKeyCode::Digit5 => Code::Digit5,
            WinitKeyCode::Digit6 => Code::Digit6,
            WinitKeyCode::Digit7 => Code::Digit7,
            WinitKeyCode::Digit8 => Code::Digit8,
            WinitKeyCode::Digit9 => Code::Digit9,
            WinitKeyCode::ArrowDown => Code::ArrowDown,
            WinitKeyCode::End => Code::End,
            WinitKeyCode::Enter => Code::Enter,
            WinitKeyCode::Equal => Code::Equal,
            WinitKeyCode::Escape => Code::Escape,
            WinitKeyCode::Home => Code::Home,
            WinitKeyCode::KeyA => Code::KeyA,
            WinitKeyCode::KeyB => Code::KeyB,
            WinitKeyCode::KeyC => Code::KeyC,
            WinitKeyCode::KeyD => Code::KeyD,
            WinitKeyCode::KeyE => Code::KeyE,
            WinitKeyCode::KeyF => Code::KeyF,
            WinitKeyCode::KeyG => Code::KeyG,
            WinitKeyCode::KeyH => Code::KeyH,
            WinitKeyCode::KeyI => Code::KeyI,
            WinitKeyCode::KeyJ => Code::KeyJ,
            WinitKeyCode::KeyK => Code::KeyK,
            WinitKeyCode::KeyL => Code::KeyL,
            WinitKeyCode::KeyM => Code::KeyM,
            WinitKeyCode::KeyN => Code::KeyN,
            WinitKeyCode::KeyO => Code::KeyO,
            WinitKeyCode::KeyP => Code::KeyP,
            WinitKeyCode::KeyQ => Code::KeyQ,
            WinitKeyCode::KeyR => Code::KeyR,
            WinitKeyCode::KeyS => Code::KeyS,
            WinitKeyCode::KeyT => Code::KeyT,
            WinitKeyCode::KeyU => Code::KeyU,
            WinitKeyCode::KeyV => Code::KeyV,
            WinitKeyCode::KeyW => Code::KeyW,
            WinitKeyCode::KeyX => Code::KeyX,
            WinitKeyCode::KeyY => Code::KeyY,
            WinitKeyCode::KeyZ => Code::KeyZ,
            WinitKeyCode::SuperLeft => Code::Super,
            WinitKeyCode::SuperRight => Code::Super,
            WinitKeyCode::Minus => Code::Minus,
            WinitKeyCode::PageDown => Code::PageDown,
            WinitKeyCode::PageUp => Code::PageUp,
            WinitKeyCode::Period => Code::Period,
            WinitKeyCode::Quote => Code::Quote,
            WinitKeyCode::ArrowLeft => Code::ArrowLeft,
            WinitKeyCode::ArrowRight => Code::ArrowRight,
            WinitKeyCode::Semicolon => Code::Semicolon,
            WinitKeyCode::ShiftLeft => Code::ShiftLeft,
            WinitKeyCode::ShiftRight => Code::ShiftRight,
            WinitKeyCode::Slash => Code::Slash,
            WinitKeyCode::Space => Code::Space,
            WinitKeyCode::Tab => Code::Tab,
            WinitKeyCode::ArrowUp => Code::ArrowUp,
            _ => Code::Unidentified,
        },
        PhysicalKey::Unidentified(_) => Code::Unidentified,
    }
}

fn winit_key_event_to_blitz(event: &WinitKeyEvent, mods: WinitModifiersState) -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: winit_key_to_kbt_key(&event.logical_key),
        code: winit_physical_key_to_kbt_code(&event.physical_key),
        modifiers: winit_modifiers_to_kbt_modifiers(mods),
        location: winit_key_location_to_kbt_location(event.location),
        is_auto_repeating: event.repeat,
        is_composing: false,
        state: match event.state {
            ElementState::Pressed => KeyState::Pressed,
            ElementState::Released => KeyState::Released,
        },
        text: event.text.as_ref().map(|text| text.as_str().into()),
    }
}
