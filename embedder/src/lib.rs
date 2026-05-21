mod chrome;
mod headless;
pub use webview::ui_event;

use crate::chrome::{ChromeAction, ChromeUi, ChromeViewState};
use crate::headless::HeadlessEmbedderApp;
use automation::{
    AutomationCommand, AutomationController, AutomationHost, AutomationSnapshot,
};
use anyrender::{PaintScene, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use cursor_icon::CursorIcon;
use ipc_messages::content::{NavigableId, PaintFrame, WebviewId};
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use kurbo::Affine;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};
use webview::{EmbedderApi, UserAgentApi, WebviewProvider};
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

struct EventLoopEmbedderApi {
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

impl EmbedderApi for EventLoopEmbedderApi {
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

fn run_embedder_event_loop<F, A, MakeApp>(
    create_user_agent: F,
    make_app: MakeApp,
) -> Result<(), String>
where
    F: FnOnce(UserEventDispatcher) -> Result<Box<dyn UserAgentApi>, String>,
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

    let user_agent = match create_user_agent(dispatcher.clone()) {
        Ok(user_agent) => user_agent,
        Err(error) => {
            let mut guard = EVENT_LOOP_PROXY
                .lock()
                .expect("event loop proxy mutex poisoned");
            guard.take();
            update_window_viewport_snapshot(None);
            return Err(error);
        }
    };
    let embedder: Box<dyn EmbedderApi> = Box::new(EventLoopEmbedderApi { dispatcher });
    let provider = WebviewProvider::new(embedder, user_agent);

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

pub fn run_headed_event_loop<F>(create_user_agent: F) -> Result<(), String>
where
    F: FnOnce(UserEventDispatcher) -> Result<Box<dyn UserAgentApi>, String>,
{
    run_embedder_event_loop(create_user_agent, |provider| HeadedEmbedderApp {
        provider: Some(provider),
        ..HeadedEmbedderApp::default()
    })
}

pub fn run_headless_event_loop<F>(create_user_agent: F) -> Result<(), String>
where
    F: FnOnce(UserEventDispatcher) -> Result<Box<dyn UserAgentApi>, String>,
{
    run_embedder_event_loop(create_user_agent, |provider| HeadlessEmbedderApp {
        provider: Some(provider),
        ..HeadlessEmbedderApp::default()
    })
}

pub fn run_event_loop<F>(create_user_agent: F) -> Result<(), String>
where
    F: FnOnce(UserEventDispatcher) -> Result<Box<dyn UserAgentApi>, String>,
{
    run_headed_event_loop(create_user_agent)
}

pub fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned")
        .as_ref()
        .copied()
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

impl Default for HeadedEmbedderApp {
    fn default() -> Self {
        Self {
            window: None,
            renderer: VelloWindowRenderer::new(),
            chrome: None,
            browser: BrowserState::default(),
            automation: AutomationController::default(),
            provider: None,
            current_webview_id: None,
            has_top_level_traversable: false,
            window_occluded: false,
            animation_timer: None,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
        }
    }
}

impl HeadedEmbedderApp {
    fn has_visible_viewport(&self) -> bool {
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        if self.window_occluded {
            return false;
        }
        if matches!(window.is_visible(), Some(false)) {
            return false;
        }
        let size = window.inner_size();
        size.width > 0 && size.height > 0
    }

    fn pointer_position_in_viewport(&self, position: PhysicalPosition<f64>) -> bool {
        if !self.has_visible_viewport() {
            return false;
        }
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        let size = window.inner_size();
        position.x >= 0.0
            && position.y >= 0.0
            && position.x < f64::from(size.width)
            && position.y < f64::from(size.height)
    }

    fn chrome_height_css(&self) -> f32 {
        self.chrome
            .as_ref()
            .map(ChromeUi::height_css)
            .unwrap_or_default()
    }

    fn chrome_height_physical(&self) -> u32 {
        self.chrome
            .as_ref()
            .map(ChromeUi::height_physical)
            .unwrap_or_default()
    }

    fn content_has_visible_viewport(&self) -> bool {
        if !self.has_visible_viewport() {
            return false;
        }
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        window.inner_size().height > self.chrome_height_physical()
    }

    fn pointer_position_in_chrome(&self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position_in_viewport(position)
            && position.y < f64::from(self.chrome_height_physical())
    }

    fn pointer_position_in_content_viewport(&self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position_in_viewport(position)
            && position.y >= f64::from(self.chrome_height_physical())
            && self.content_has_visible_viewport()
    }

    fn request_visible_redraw(&self, reason: &str) {
        if !self.has_visible_viewport() {
            return;
        }
        self.request_window_redraw();
        if let Some((provider, webview_id)) = self.provider.as_ref().zip(self.current_webview_id)
        {
            provider.note_rendering_opportunity(webview_id, reason);
        }
    }

    fn request_window_redraw(&self) {
        if !self.has_visible_viewport() {
            return;
        }
        let Some(window) = self.window.as_ref() else {
            return;
        };
        window.request_redraw();
    }

    fn content_viewport_snapshot(&self, window: &Window) -> (u32, u32, f32, ColorScheme) {
        let (width, height, scale, color_scheme) = viewport_snapshot_for_window(window);
        (
            width,
            height.saturating_sub(self.chrome_height_physical()),
            scale,
            color_scheme,
        )
    }

    fn update_content_viewport_snapshot(&mut self, window: &Window) {
        let viewport_snapshot = self.content_viewport_snapshot(window);
        update_window_viewport_snapshot(Some(viewport_snapshot));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(viewport_snapshot));
            if let Some(webview_id) = self.current_webview_id {
                let (width, height, scale, color_scheme) = viewport_snapshot;
                let _ = provider.set_traversable_viewport(
                    webview_id,
                    (width, height, scale, color_scheme),
                    0.0,
                    0.0,
                );
            }
        }
    }

    fn current_chrome_view_state(&self) -> ChromeViewState {
        ChromeViewState {
            address: self.browser.displayed_url(),
        }
    }

    fn sync_chrome_state(&mut self) {
        let chrome_view_state = self.current_chrome_view_state();
        if let Some(chrome) = self.chrome.as_mut() {
            chrome.sync_state(&chrome_view_state);
        }
        if let Some(window) = self.window.clone() {
            self.update_content_viewport_snapshot(&window);
        }
    }

    fn resume_renderer_for_window(&mut self, window: &Arc<Window>) {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height, || {});
            self.renderer.complete_resume();
        }
    }

    fn current_animation_time(&mut self) -> f64 {
        match self.animation_timer {
            Some(start) => Instant::now().duration_since(start).as_secs_f64(),
            None => {
                self.animation_timer = Some(Instant::now());
                0.0
            }
        }
    }

    fn create_window(event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let options = event_loop_options();
        let title = options
            .window_title
            .unwrap_or_else(|| String::from("formal-web"));
        let attributes: WindowAttributes = Window::default_attributes().with_title(title);
        event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|error| format!("failed to create winit window: {error}"))
    }

    fn paint_current_frame(&mut self) {
        if !self.has_visible_viewport() {
            return;
        }
        let _ = self.current_animation_time();
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let chrome_height = f64::from(self.chrome_height_physical());
        let chrome_scene = self.chrome.as_mut().map(ChromeUi::paint_scene);
        let content_scene = self
            .provider
            .as_mut()
            .zip(self.current_webview_id)
            .and_then(|(provider, webview_id)| provider.current_scene(webview_id))
            .map(|scene| scene);

        if chrome_scene.is_none() && content_scene.is_none() {
            return;
        }

        let size = window.inner_size();

        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height, || {});
            self.renderer.complete_resume();
        }

        self.renderer.render(|scene| {
            if let Some(content_scene) = content_scene.clone() {
                scene.append_scene(content_scene, Affine::translate((0.0, chrome_height)));
            }
            if let Some(chrome_scene) = chrome_scene.clone() {
                scene.append_scene(chrome_scene, Affine::IDENTITY);
            }
        });
    }

    fn logical_position(&self, position: PhysicalPosition<f64>) -> LogicalPosition<f32> {
        let scale = self
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);
        position.to_logical(scale)
    }

    fn chrome_pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x: screen_x, y: screen_y } = self.logical_position(position);
        PointerCoords {
            screen_x,
            screen_y,
            client_x: screen_x,
            client_y: screen_y,
            page_x: screen_x,
            page_y: screen_y,
        }
    }

    fn content_pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x: screen_x, y: screen_y } = self.logical_position(position);
        let client_x = screen_x;
        let client_y = screen_y - self.chrome_height_css();
        PointerCoords {
            screen_x,
            screen_y,
            client_x,
            client_y,
            page_x: client_x,
            page_y: client_y,
        }
    }

    fn send_content_ui_event(
        &mut self,
        event: UiEvent,
        require_visible_viewport: bool,
    ) -> Result<(), String> {
        if require_visible_viewport {
            if !self.content_has_visible_viewport() {
                return Err(String::from("content viewport is not visible"));
            }
        } else {
            let Some(window) = self.window.as_ref() else {
                return Err(String::from("window is not initialized"));
            };
            if window.inner_size().height <= self.chrome_height_physical() {
                return Err(String::from("content viewport is not ready for automation clicks"));
            }
        }

        if !self.has_top_level_traversable {
            return Err(String::from("no top-level traversable is active"));
        }

        let Some(provider) = self.provider.as_mut() else {
            return Err(String::from("webview provider is not initialized"));
        };
        let Some(webview_id) = self.current_webview_id else {
            return Err(String::from("no current webview is active"));
        };

        provider.send_ui_event(webview_id, event)?;
        if require_visible_viewport {
            self.request_window_redraw();
        }
        Ok(())
    }

    fn dispatch_content_ui_event(&mut self, event: UiEvent) {
        let _ = self.send_content_ui_event(event, true);
    }

    fn handle_chrome_ui_event(&mut self, event: UiEvent) {
        if !self.has_visible_viewport() {
            return;
        }

        let action = self
            .chrome
            .as_mut()
            .and_then(|chrome| chrome.handle_ui_event(event));
        self.request_window_redraw();
        if let Some(action) = action {
            self.handle_chrome_action(action);
        }
    }

    fn start_navigation_request(&self, destination_url: &str) -> Result<(), String> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("webview provider is not initialized"))?;
        provider.navigate(self.current_webview_id, destination_url)
    }

    fn begin_navigation(&mut self, pending_navigation: PendingNavigation) -> Result<(), String> {
        self.start_navigation_request(&pending_navigation.url)?;
        self.browser.begin_navigation(pending_navigation);
        self.sync_chrome_state();
        self.request_window_redraw();
        Ok(())
    }

    fn handle_chrome_action(&mut self, action: ChromeAction) {
        let result = match action {
            ChromeAction::Navigate => {
                let Some(chrome) = self.chrome.as_ref() else {
                    return;
                };
                let Some(destination_url) = normalize_browser_destination(&chrome.address_value()) else {
                    return;
                };
                self.begin_navigation(PendingNavigation { url: destination_url })
            }
        };

        if let Err(error) = result {
            eprintln!("{error}");
        }
    }

    fn handle_navigation_requested(&mut self, webview_id: WebviewId, destination_url: String) {
        if self.current_webview_id == Some(webview_id) {
            self.browser.begin_navigation(PendingNavigation {
                url: destination_url,
            });
            self.sync_chrome_state();
            self.request_window_redraw();
        }
    }

    fn sync_browser_navigable_id_from_provider(&mut self) {
        let navigable_id = self
            .provider
            .as_ref()
            .zip(self.current_webview_id)
            .and_then(|(provider, webview_id)| provider.current_navigable_id(webview_id));
        self.browser.set_current_navigable_id(navigable_id);
    }

    fn with_automation_controller<R>(
        &mut self,
        f: impl FnOnce(&mut AutomationController, &mut Self) -> R,
    ) -> R {
        let mut automation = std::mem::take(&mut self.automation);
        let result = f(&mut automation, self);
        self.automation = automation;
        result
    }

    fn handle_navigation_completed(&mut self, completed: NavigationCompleted) {
        let is_current = self.current_webview_id == Some(completed.webview_id);

        match &completed.status {
            NavigationCompletion::Committed { url } => {
                if is_current {
                    self.browser.commit_navigation(url.clone());
                    self.sync_chrome_state();
                    self.request_window_redraw();
                    self.with_automation_controller(|automation, app| {
                        automation.note_navigation_committed(app)
                    });
                }

                if let Some(provider) = self.provider.as_mut() {
                    provider.on_navigation_committed(completed.webview_id);
                }
            }
            NavigationCompletion::Aborted { message } => {
                if is_current {
                    self.with_automation_controller(|automation, _app| {
                        automation.abort_pending_navigation(message.clone())
                    });
                    self.browser.cancel_pending_navigation();
                    self.sync_chrome_state();
                }
            }
        }

        self.request_window_redraw();
    }

    fn dispatch_automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        let Some(window) = self.window.as_ref() else {
            return Err(String::from("window is not initialized"));
        };

        let scale = window.scale_factor();
        let chrome_height_css = f64::from(self.chrome_height_css());
        let position = PhysicalPosition::new(
            f64::from(x) * scale,
            (f64::from(y) + chrome_height_css) * scale,
        );
        self.pointer_pos = position;

        if let Some(chrome) = self.chrome.as_mut() {
            chrome.clear_focus();
        }
        self.request_window_redraw();

        let modifiers = winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state());
        let move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: Default::default(),
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerMove(move_event), false)?;

        self.buttons |= MouseEventButton::Main.into();
        let down_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerDown(down_event), false)?;

        self.buttons.remove(MouseEventButton::Main.into());
        let up_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerUp(up_event), false)?;

        Ok(())
    }

    fn dispatch_automation_click_element(&self, selector: String) -> Result<(), String> {
        match self.provider.as_ref().zip(self.current_webview_id) {
            Some((provider, webview_id)) => {
                provider.click_element(webview_id, selector)?;
                provider.note_rendering_opportunity(webview_id, "automation_element_click");
                Ok(())
            }
            None => Err(String::from(
                "no active top-level traversable is available for element click",
            )),
        }
    }

    fn dispatch_automation_scroll(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String> {
        let Some(window) = self.window.as_ref() else {
            return Err(String::from("window is not initialized"));
        };

        let scale = window.scale_factor();
        let chrome_height_css = f64::from(self.chrome_height_css());
        let position = PhysicalPosition::new(
            f64::from(x) * scale,
            (f64::from(y) + chrome_height_css) * scale,
        );
        self.pointer_pos = position;

        let modifiers = winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state());
        let move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: Default::default(),
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerMove(move_event), false)?;

        self.send_content_ui_event(
            UiEvent::Wheel(BlitzWheelEvent {
                delta: BlitzWheelDelta::Pixels(f64::from(delta_x), f64::from(delta_y)),
                coords: self.content_pointer_coords(position),
                buttons: self.buttons,
                mods: modifiers,
            }),
            false,
        )?;

        Ok(())
    }
}

impl AutomationHost for HeadedEmbedderApp {
    fn automation_snapshot(&mut self) -> AutomationSnapshot {
        self.sync_browser_navigable_id_from_provider();
        self.browser.automation_snapshot(
            self.current_webview_id,
            self.has_top_level_traversable,
        )
    }

    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String> {
        self.begin_navigation(PendingNavigation { url })
    }

    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        self.dispatch_automation_click(x, y)
    }

    fn automation_click_element(&mut self, selector: String) -> Result<(), String> {
        self.dispatch_automation_click_element(selector)
    }

    fn automation_scroll(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String> {
        self.dispatch_automation_scroll(x, y, delta_x, delta_y)
    }

    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String> {
        match self.provider.as_ref().zip(self.current_webview_id) {
            Some((provider, webview_id)) => provider.evaluate_script(webview_id, source, timeout),
            None => Err(String::from(
                "no active top-level traversable is available for script execution",
            )),
        }
    }
}

impl ApplicationHandler<FormalWebUserEvent> for HeadedEmbedderApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match Self::create_window(event_loop) {
                Ok(window) => {
                    let full_viewport = viewport_of_snapshot(viewport_snapshot_for_window(&window));
                    let chrome_shell_provider: Arc<dyn ShellProvider> =
                        Arc::new(WinitShellProvider::new(window.clone()));
                    let chrome = match ChromeUi::new(full_viewport, chrome_shell_provider) {
                        Ok(chrome) => chrome,
                        Err(_error) => {
                            event_loop.exit();
                            return;
                        }
                    };
                    self.chrome = Some(chrome);
                    self.window = Some(window.clone());
                    self.sync_chrome_state();
                    self.update_content_viewport_snapshot(&window);
                    self.resume_renderer_for_window(&window);
                    let startup_url = event_loop_options().startup_url;
                    match startup_destination_url(startup_url.as_deref()) {
                        Ok(destination_url) => {
                            self.browser.begin_navigation(PendingNavigation {
                                url: destination_url,
                            });
                            self.sync_chrome_state();
                            if let Some(provider) = self.provider.as_ref() {
                                if provider.start(startup_url.as_deref()).is_err() {
                                    event_loop.exit();
                                }
                            }
                        }
                        Err(_error) => event_loop.exit(),
                    }
                    self.request_window_redraw();
                }
                Err(_error) => event_loop.exit(),
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(current_window_id) = self.window.as_ref().map(|window| window.id()) else {
            return;
        };
        if current_window_id != window_id {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                if self.provider.is_some() || self.chrome.is_some() {
                    self.paint_current_frame();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.window_occluded = occluded;
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = self.window.as_ref() {
                    let full_viewport = viewport_of_snapshot(viewport_snapshot_for_window(window));
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.set_viewport(full_viewport);
                    }
                    self.sync_chrome_state();
                }
                if self.renderer.is_active() {
                    self.renderer.set_size(size.width, size.height);
                }
                if self.has_top_level_traversable {
                    self.request_visible_redraw("request_redraw");
                } else {
                    self.request_window_redraw();
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                if let Some(window) = self.window.as_ref() {
                    window.set_visible(false);
                }
                self.with_automation_controller(|automation, _app| {
                    automation.abort_pending_navigation(String::from(
                        "window closed before navigation completed",
                    ))
                });
                self.renderer.suspend();
                self.animation_timer = None;
                self.chrome = None;
                self.browser = BrowserState::default();
                self.provider = None;
                self.current_webview_id = None;
                self.has_top_level_traversable = false;
                self.window_occluded = false;
                update_window_viewport_snapshot(None);
                self.window = None;
                event_loop.exit();
            }
            WindowEvent::Ime(ime_event) => {
                let event = UiEvent::Ime(winit_ime_to_blitz(ime_event));
                if self.chrome.as_ref().is_some_and(ChromeUi::takes_text_input_focus) {
                    self.handle_chrome_ui_event(event);
                } else {
                    self.dispatch_content_ui_event(event);
                }
            }
            WindowEvent::ModifiersChanged(new_state) => {
                self.keyboard_modifiers = new_state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let key_event = winit_key_event_to_blitz(&event, self.keyboard_modifiers.state());
                let event = if event.state.is_pressed() {
                    UiEvent::KeyDown(key_event)
                } else {
                    UiEvent::KeyUp(key_event)
                };
                if self.chrome.as_ref().is_some_and(ChromeUi::takes_text_input_focus) {
                    self.handle_chrome_ui_event(event);
                } else {
                    self.dispatch_content_ui_event(event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer_pos = position;
                if self.pointer_position_in_chrome(position) {
                    self.handle_chrome_ui_event(UiEvent::PointerMove(BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.chrome_pointer_coords(position),
                        button: Default::default(),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    }));
                } else if self.pointer_position_in_content_viewport(position) {
                    self.dispatch_content_ui_event(UiEvent::PointerMove(BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.content_pointer_coords(position),
                        button: Default::default(),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    }));
                }
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if !self.pointer_position_in_viewport(self.pointer_pos) {
                    return;
                }
                let mapped_button = match button {
                    MouseButton::Left => MouseEventButton::Main,
                    MouseButton::Right => MouseEventButton::Secondary,
                    MouseButton::Middle => MouseEventButton::Auxiliary,
                    MouseButton::Back => MouseEventButton::Fourth,
                    MouseButton::Forward => MouseEventButton::Fifth,
                    MouseButton::Other(_) => MouseEventButton::Auxiliary,
                };
                match state {
                    ElementState::Pressed => self.buttons |= mapped_button.into(),
                    ElementState::Released => self.buttons.remove(mapped_button.into()),
                }
                if self.pointer_position_in_chrome(self.pointer_pos) {
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.chrome_pointer_coords(self.pointer_pos),
                        button: mapped_button,
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    };
                    self.handle_chrome_ui_event(match state {
                        ElementState::Pressed => UiEvent::PointerDown(event),
                        ElementState::Released => UiEvent::PointerUp(event),
                    });
                } else if self.pointer_position_in_content_viewport(self.pointer_pos) {
                    if state.is_pressed() {
                        if let Some(chrome) = self.chrome.as_mut() {
                            chrome.clear_focus();
                        }
                        self.request_window_redraw();
                    }
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.content_pointer_coords(self.pointer_pos),
                        button: mapped_button,
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    };
                    self.dispatch_content_ui_event(match state {
                        ElementState::Pressed => UiEvent::PointerDown(event),
                        ElementState::Released => UiEvent::PointerUp(event),
                    });
                }
            }
            WindowEvent::Touch(Touch { phase, location, force, id, .. }) => {
                if !self.pointer_position_in_viewport(location) {
                    return;
                }
                if self.pointer_position_in_chrome(location) {
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Finger(id),
                        is_primary: true,
                        coords: self.chrome_pointer_coords(location),
                        button: Default::default(),
                        buttons: MouseEventButtons::None,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: touch_pointer_details(force),
                    };
                    match phase {
                        TouchPhase::Started => self.handle_chrome_ui_event(UiEvent::PointerDown(event)),
                        TouchPhase::Moved => self.handle_chrome_ui_event(UiEvent::PointerMove(event)),
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.handle_chrome_ui_event(UiEvent::PointerUp(event))
                        }
                    }
                } else if self.pointer_position_in_content_viewport(location) {
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Finger(id),
                        is_primary: true,
                        coords: self.content_pointer_coords(location),
                        button: Default::default(),
                        buttons: MouseEventButtons::None,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: touch_pointer_details(force),
                    };
                    match phase {
                        TouchPhase::Started => {
                            if let Some(chrome) = self.chrome.as_mut() {
                                chrome.clear_focus();
                            }
                            self.request_window_redraw();
                            self.dispatch_content_ui_event(UiEvent::PointerDown(event))
                        }
                        TouchPhase::Moved => self.dispatch_content_ui_event(UiEvent::PointerMove(event)),
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.dispatch_content_ui_event(UiEvent::PointerUp(event))
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !self.pointer_position_in_viewport(self.pointer_pos) {
                    return;
                }
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => BlitzWheelDelta::Lines(x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(pos) => BlitzWheelDelta::Pixels(pos.x, pos.y),
                };
                if self.pointer_position_in_chrome(self.pointer_pos) {
                    self.handle_chrome_ui_event(UiEvent::Wheel(BlitzWheelEvent {
                        delta,
                        coords: self.chrome_pointer_coords(self.pointer_pos),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    }));
                } else if self.pointer_position_in_content_viewport(self.pointer_pos) {
                    self.dispatch_content_ui_event(UiEvent::Wheel(BlitzWheelEvent {
                        delta,
                        coords: self.content_pointer_coords(self.pointer_pos),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    }));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::Paint(snapshot) => {
                let Some(provider) = self.provider.as_mut() else {
                    return;
                };
                match provider.on_paint_frame(snapshot) {
                    Ok(()) => {
                        self.with_automation_controller(|automation, app| {
                            automation.note_rendering_update(app)
                        });
                        self.request_window_redraw();
                    }
                    Err(error) => {
                        eprintln!("paint error: {error}");
                    }
                }
            }
            FormalWebUserEvent::RequestRedraw(webview_id) => {
                if self.current_webview_id == Some(webview_id) {
                    self.request_window_redraw();
                }
            }
            FormalWebUserEvent::NavigationRequested { webview_id, destination_url } => {
                self.handle_navigation_requested(webview_id, destination_url);
            }
            FormalWebUserEvent::NavigationCompleted(completed) => {
                self.handle_navigation_completed(completed);
            }
            FormalWebUserEvent::NewTopLevelTraversable(webview_id, target_name) => {
                if let Some(child_navigable_host) =
                    parse_child_navigable_host_target(&target_name)
                {
                    if let Some(provider) = self.provider.as_mut() {
                        provider.register_child_navigable_host(
                            webview_id,
                            child_navigable_host.parent_traversable_id,
                            child_navigable_host.content_frame_id,
                        );
                    }
                } else {
                    self.has_top_level_traversable = true;
                    self.current_webview_id = Some(webview_id);
                    if let Some(provider) = self.provider.as_mut() {
                        provider.on_new_top_level_traversable(webview_id);
                    }
                    self.request_visible_redraw("request_redraw");
                }
            }
            FormalWebUserEvent::Automation(command) => {
                self.with_automation_controller(|automation, app| {
                    automation.handle_command(app, command)
                });
            }
            FormalWebUserEvent::ClipboardRead { reply } => {
                let _ = reply.send(read_clipboard_text());
            }
            FormalWebUserEvent::ClipboardWrite { text, reply } => {
                let _ = reply.send(write_clipboard_text(text));
            }
            FormalWebUserEvent::Exit => {
                event_loop.exit();
            }
        }
    }
}
