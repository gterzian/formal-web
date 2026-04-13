mod chrome;
mod content_bridge;
pub mod ui_event;

use crate::chrome::{ChromeAction, ChromeUi, ChromeViewState};
use anyrender::{PaintScene, Scene as RenderScene, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use ipc_messages::content::{
    BeforeUnloadResult, Command as ContentCommand, FontTransportReceiver, NavigateRequest,
    NavigationCommitted, PaintFrame, SceneSummary, ScrollOffset,
};
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use kurbo::Affine;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, PhysicalPosition};
use winit::event::{
    ElementState, Ime, KeyEvent as WinitKeyEvent, Modifiers, MouseButton, MouseScrollDelta,
    Touch, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{
    Key as WinitKey, KeyCode as WinitKeyCode, KeyLocation as WinitKeyLocation,
    ModifiersState as WinitModifiersState, NamedKey, PhysicalKey,
};
use winit::window::{Window, WindowAttributes, WindowId};

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";
const NEW_TOP_LEVEL_TRAVERSABLE_MESSAGE: &str = "NewTopLevelTraversable";
const DISPATCH_EVENT_MESSAGE_PREFIX: &str = "DispatchEvent|";

#[derive(Clone, Default)]
pub struct EventLoopOptions {
    pub startup_url: Option<String>,
    pub window_title: Option<String>,
}

#[derive(Clone, Copy)]
pub struct RuntimeHooks {
    pub handle_runtime_message: fn(&str),
    pub start_document_fetch_parts: fn(usize, &str, &str, &str) -> Result<(), String>,
    pub start_navigation_parts: fn(usize, &str, &str, &str, bool) -> Result<(), String>,
    pub complete_before_unload_parts: fn(usize, usize, bool) -> Result<(), String>,
    pub abort_navigation_parts: fn(usize) -> Result<(), String>,
    pub note_rendering_opportunity: fn(&str),
}

#[derive(Clone)]
struct EmbedderPaintFrame {
    document_id: u64,
    scene: RenderScene,
    debug_summary: Option<SceneSummary>,
    viewport_scroll: ScrollOffset,
}

#[derive(Default)]
struct PendingUiEvents {
    events: Vec<UiEvent>,
}

impl PendingUiEvents {
    fn push(&mut self, event: UiEvent) {
        self.events.push(event);
    }

    fn take(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.events)
    }

    fn take_coalesced(&mut self) -> Vec<UiEvent> {
        let events = self.take();
        let mut last_pointer_move = None;
        let mut last_wheel = None;

        for (index, event) in events.iter().enumerate() {
            match event {
                UiEvent::PointerMove(_) => last_pointer_move = Some(index),
                UiEvent::Wheel(_) => last_wheel = Some(index),
                _ => {}
            }
        }

        events
            .into_iter()
            .enumerate()
            .filter_map(|(index, event)| match &event {
                UiEvent::PointerMove(_) => {
                    if last_pointer_move == Some(index) {
                        Some(event)
                    } else {
                        None
                    }
                }
                UiEvent::Wheel(_) => {
                    if last_wheel == Some(index) {
                        Some(event)
                    } else {
                        None
                    }
                }
                _ => Some(event),
            })
            .collect()
    }

    fn clear(&mut self) {
        self.events.clear();
    }
}

pub(crate) enum FormalWebUserEvent {
    Paint(PaintFrame),
    NavigationRequested(NavigateRequest),
    BeforeUnloadCompleted(BeforeUnloadResult),
    NavigationCommitted(NavigationCommitted),
    EmbedderRequestRedraw,
    NewTopLevelTraversable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingNavigation {
    url: String,
}

#[derive(Default)]
struct BrowserState {
    history: Vec<String>,
    history_index: Option<usize>,
    pending_navigation: Option<PendingNavigation>,
    current_document_id: Option<u64>,
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

    fn commit_navigation(&mut self, document_id: u64, url: String) {
        self.pending_navigation.take();
        self.current_document_id = Some(document_id);

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
}

struct FormalWebApp {
    window: Option<Arc<Window>>,
    renderer: VelloWindowRenderer,
    chrome: Option<ChromeUi>,
    browser: BrowserState,
    current_paint_frame: Option<EmbedderPaintFrame>,
    font_receiver: FontTransportReceiver,
    pending_ui_events: PendingUiEvents,
    saw_redraw_requested: bool,
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
static RUNTIME_HOOKS: LazyLock<Mutex<Option<RuntimeHooks>>> =
    LazyLock::new(|| Mutex::new(None));
static EVENT_LOOP_OPTIONS: LazyLock<Mutex<EventLoopOptions>> =
    LazyLock::new(|| Mutex::new(EventLoopOptions::default()));

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

pub fn set_runtime_hooks(hooks: RuntimeHooks) {
    let mut guard = RUNTIME_HOOKS.lock().expect("runtime hooks mutex poisoned");
    *guard = Some(hooks);
}

fn runtime_hooks() -> Result<RuntimeHooks, String> {
    RUNTIME_HOOKS
        .lock()
        .expect("runtime hooks mutex poisoned")
        .as_ref()
        .copied()
        .ok_or_else(|| String::from("embedder runtime hooks are not initialized"))
}

fn call_lean_runtime_message_handler(message: &str) {
    if let Ok(hooks) = runtime_hooks() {
        (hooks.handle_runtime_message)(message);
    }
}

pub(crate) fn call_lean_document_fetch_start_parts(
    handler: usize,
    url: &str,
    method: &str,
    body: &str,
) -> Result<(), String> {
    let hooks = runtime_hooks()?;
    (hooks.start_document_fetch_parts)(handler, url, method, body)
}

pub(crate) fn call_lean_navigation_start_parts(
    document_id: usize,
    destination_url: &str,
    target: &str,
    user_involvement: &str,
    noopener: bool,
) -> Result<(), String> {
    let hooks = runtime_hooks()?;
    (hooks.start_navigation_parts)(
        document_id,
        destination_url,
        target,
        user_involvement,
        noopener,
    )
}

pub(crate) fn call_lean_before_unload_completed_parts(
    document_id: usize,
    check_id: usize,
    canceled: bool,
) -> Result<(), String> {
    let hooks = runtime_hooks()?;
    (hooks.complete_before_unload_parts)(document_id, check_id, canceled)
}

fn user_agent_note_rendering_opportunity(message: &str) {
    if let Ok(hooks) = runtime_hooks() {
        (hooks.note_rendering_opportunity)(message);
    }
}

pub fn send_runtime_message(message: &str) -> Result<(), String> {
    let user_event = user_event_of_runtime_message(message)?;
    with_event_loop_proxy(|proxy| match proxy {
        Some(proxy) => proxy
            .send_event(user_event)
            .map_err(|error| format!("failed to send runtime message event: {error}")),
        None => Err(String::from("winit event loop proxy is not initialized")),
    })
}

pub fn run_event_loop() -> Result<(), String> {
    let event_loop = EventLoop::<FormalWebUserEvent>::with_user_event()
        .build()
        .map_err(|error| format!("failed to create event loop: {error}"))?;
    {
        let mut guard = EVENT_LOOP_PROXY
            .lock()
            .expect("event loop proxy mutex poisoned");
        *guard = Some(event_loop.create_proxy());
    }

    let mut app = FormalWebApp::default();
    let run_result = event_loop
        .run_app(&mut app)
        .map_err(|error| format!("winit event loop failed: {error}"));

    {
        let mut guard = EVENT_LOOP_PROXY
            .lock()
            .expect("event loop proxy mutex poisoned");
        guard.take();
    }

    run_result
}

pub fn request_redraw() {
    with_event_loop_proxy(|proxy| {
        if let Some(proxy) = proxy {
            let _ = proxy.send_event(FormalWebUserEvent::EmbedderRequestRedraw);
        }
    });
}

pub fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned")
        .as_ref()
        .copied()
}

pub fn start_content(event_loop_id: usize) -> Result<usize, String> {
    content_bridge::start(event_loop_id)
}

pub fn stop_content(handle: usize) -> Result<(), String> {
    content_bridge::stop(handle)
}

pub fn send_content_command(handle: usize, command: ContentCommand) -> Result<(), String> {
    content_bridge::send_command(handle, command)
}

fn startup_runtime_message() -> Result<String, String> {
    let startup_url = match event_loop_options().startup_url {
        Some(url) => url,
        None => startup_artifact_url()?,
    };
    Ok(format!("FreshTopLevelTraversable|{startup_url}"))
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

fn user_event_of_runtime_message(message: &str) -> Result<FormalWebUserEvent, String> {
    match message {
        NEW_TOP_LEVEL_TRAVERSABLE_MESSAGE => Ok(FormalWebUserEvent::NewTopLevelTraversable),
        _ => Err(format!("unknown runtime message: {message}")),
    }
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

fn render_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_RENDER").is_some()
}

fn log_embedder_scene(stage: &str, document_id: u64, summary: SceneSummary) {
    if !render_debug_enabled() {
        return;
    }

    eprintln!(
        "[render-debug][embedder] stage={} doc={} {}",
        stage,
        document_id,
        summary.describe(),
    );
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

impl Default for FormalWebApp {
    fn default() -> Self {
        Self {
            window: None,
            renderer: VelloWindowRenderer::new(),
            chrome: None,
            browser: BrowserState::default(),
            current_paint_frame: None,
            font_receiver: FontTransportReceiver::default(),
            pending_ui_events: PendingUiEvents::default(),
            saw_redraw_requested: false,
            has_top_level_traversable: false,
            window_occluded: false,
            animation_timer: None,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
        }
    }
}

impl FormalWebApp {
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
        user_agent_note_rendering_opportunity(reason);
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

    fn paint_frame(&mut self, snapshot: PaintFrame) -> Result<EmbedderPaintFrame, String> {
        let document_id = snapshot.document_id;
        let viewport_scroll = snapshot.viewport_scroll.clone();
        let scene = snapshot.into_recorded_scene(&mut self.font_receiver)?;
        let debug_summary = render_debug_enabled().then(|| scene.summary());
        Ok(EmbedderPaintFrame {
            document_id,
            scene: scene.into_scene(&self.font_receiver),
            debug_summary,
            viewport_scroll,
        })
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

    fn update_content_viewport_snapshot(&self, window: &Window) {
        let viewport_snapshot = self.content_viewport_snapshot(window);
        let mut snapshot = WINDOW_VIEWPORT_SNAPSHOT
            .lock()
            .expect("window viewport snapshot mutex poisoned");
        *snapshot = Some(viewport_snapshot);
        content_bridge::broadcast_viewport(Some(viewport_snapshot));
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
        if let Some(window) = self.window.as_ref() {
            self.update_content_viewport_snapshot(window);
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
            self.renderer.resume(window_handle, size.width, size.height);
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
        let title = event_loop_options()
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
        let content_scene = self.current_paint_frame.as_ref().map(|current_paint_frame| {
            if let Some(summary) = current_paint_frame.debug_summary {
                log_embedder_scene("render", current_paint_frame.document_id, summary);
            }
            current_paint_frame.scene.clone()
        });

        if chrome_scene.is_none() && content_scene.is_none() {
            return;
        }

        let size = window.inner_size();

        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height);
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
        if let Some(current_paint_frame) = self.current_paint_frame.as_ref() {
            let page_x = client_x + current_paint_frame.viewport_scroll.x;
            let page_y = client_y + current_paint_frame.viewport_scroll.y;
            PointerCoords {
                screen_x,
                screen_y,
                client_x,
                client_y,
                page_x,
                page_y,
            }
        } else {
            PointerCoords {
                screen_x,
                screen_y,
                client_x: screen_x,
                client_y: screen_y,
                page_x: screen_x,
                page_y: screen_y,
            }
        }
    }

    fn dispatch_content_ui_event(&mut self, event: UiEvent) {
        if !self.content_has_visible_viewport() {
            return;
        }
        self.pending_ui_events.push(event);
        if self.has_top_level_traversable {
            self.request_window_redraw();
        }
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

    fn flush_pending_ui_events(&mut self) {
        if !self.has_top_level_traversable || !self.content_has_visible_viewport() {
            return;
        }

        let mut dispatched_any = false;
        let events = self.pending_ui_events.take_coalesced();
        for event in events {
            let Ok(event_message) = ui_event::serialize_ui_event(&event) else {
                continue;
            };
            let message = format!("{DISPATCH_EVENT_MESSAGE_PREFIX}{event_message}");
            call_lean_runtime_message_handler(&message);
            dispatched_any = true;
        }

        if dispatched_any {
            user_agent_note_rendering_opportunity("ui_event_batch");
        }
    }

    fn start_navigation_request(&self, destination_url: &str) -> Result<(), String> {
        match self.browser.current_document_id {
            Some(document_id) => call_lean_navigation_start_parts(
                document_id as usize,
                destination_url,
                "",
                "browser-ui",
                false,
            ),
            None => {
                let message = format!("FreshTopLevelTraversable|{destination_url}");
                call_lean_runtime_message_handler(&message);
                Ok(())
            }
        }
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

    fn handle_navigation_requested(&mut self, request: NavigateRequest) {
        self.browser.begin_navigation(PendingNavigation {
            url: request.destination_url,
        });
        self.sync_chrome_state();
        self.request_window_redraw();
    }

    fn handle_before_unload_completed(&mut self, result: BeforeUnloadResult) {
        if result.canceled {
            self.browser.cancel_pending_navigation();
            self.sync_chrome_state();
            self.request_window_redraw();
        }
    }

    fn handle_navigation_committed(&mut self, committed: NavigationCommitted) {
        self.browser
            .commit_navigation(committed.document_id, committed.url);
        self.sync_chrome_state();
        self.request_window_redraw();
    }
}

impl ApplicationHandler<FormalWebUserEvent> for FormalWebApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match Self::create_window(event_loop) {
                Ok(window) => {
                    let full_viewport = viewport_of_snapshot(viewport_snapshot_for_window(&window));
                    let chrome = match ChromeUi::new(full_viewport) {
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
                    match startup_runtime_message() {
                        Ok(message) => {
                            if let Some(destination_url) =
                                message.strip_prefix("FreshTopLevelTraversable|")
                            {
                                self.browser.begin_navigation(PendingNavigation {
                                    url: destination_url.to_owned(),
                                });
                                self.sync_chrome_state();
                            }
                            call_lean_runtime_message_handler(&message)
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
                self.saw_redraw_requested = true;
                self.flush_pending_ui_events();
                if self.current_paint_frame.is_some() || self.chrome.is_some() {
                    self.paint_current_frame();
                    self.saw_redraw_requested = false;
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
                self.renderer.suspend();
                self.animation_timer = None;
                self.chrome = None;
                self.browser = BrowserState::default();
                self.current_paint_frame = None;
                self.pending_ui_events.clear();
                self.has_top_level_traversable = false;
                self.window_occluded = false;
                if let Ok(mut snapshot) = WINDOW_VIEWPORT_SNAPSHOT.lock() {
                    *snapshot = None;
                }
                content_bridge::broadcast_viewport(None);
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
                    ElementState::Released => self.buttons ^= mapped_button.into(),
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::Paint(snapshot) => {
                let Ok(paint_frame) = self.paint_frame(snapshot) else {
                    return;
                };
                if let Some(summary) = paint_frame.debug_summary {
                    log_embedder_scene("received", paint_frame.document_id, summary);
                }
                self.browser.current_document_id = Some(paint_frame.document_id);
                self.current_paint_frame = Some(paint_frame);
                if self.saw_redraw_requested {
                    self.paint_current_frame();
                    self.saw_redraw_requested = false;
                } else if self.has_visible_viewport() {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            FormalWebUserEvent::NavigationRequested(request) => {
                self.handle_navigation_requested(request);
            }
            FormalWebUserEvent::BeforeUnloadCompleted(result) => {
                self.handle_before_unload_completed(result);
            }
            FormalWebUserEvent::NavigationCommitted(committed) => {
                self.handle_navigation_committed(committed);
            }
            FormalWebUserEvent::EmbedderRequestRedraw => {
                if self.has_visible_viewport() {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            FormalWebUserEvent::NewTopLevelTraversable => {
                self.has_top_level_traversable = true;
                self.request_visible_redraw("request_redraw");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PendingUiEvents;
    use blitz_traits::events::{
        BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButtons,
        PointerCoords, PointerDetails, UiEvent,
    };
    use keyboard_types::Modifiers as KeyboardModifiers;

    fn pointer_move(x: f32, y: f32) -> UiEvent {
        UiEvent::PointerMove(BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: PointerCoords {
                screen_x: x,
                screen_y: y,
                client_x: x,
                client_y: y,
                page_x: x,
                page_y: y,
            },
            button: Default::default(),
            buttons: MouseEventButtons::None,
            mods: KeyboardModifiers::default(),
            details: PointerDetails::default(),
        })
    }

    fn wheel_event(delta: BlitzWheelDelta) -> UiEvent {
        UiEvent::Wheel(BlitzWheelEvent {
            delta,
            coords: PointerCoords {
                screen_x: 10.0,
                screen_y: 20.0,
                client_x: 10.0,
                client_y: 20.0,
                page_x: 10.0,
                page_y: 20.0,
            },
            buttons: MouseEventButtons::None,
            mods: KeyboardModifiers::default(),
        })
    }

    #[test]
    fn pending_ui_events_keep_only_last_pointer_move_on_flush() {
        let mut pending = PendingUiEvents::default();
        pending.push(pointer_move(10.0, 20.0));
        pending.push(pointer_move(30.0, 40.0));

        let events = pending.take_coalesced();
        assert_eq!(events.len(), 1);
        match &events[0] {
            UiEvent::PointerMove(event) => {
                assert_eq!(event.coords.client_x, 30.0);
                assert_eq!(event.coords.client_y, 40.0);
            }
            event => panic!("expected pointer move, got {event:?}"),
        }
    }

    #[test]
    fn pending_ui_events_keep_only_last_wheel_on_flush() {
        let mut pending = PendingUiEvents::default();
        pending.push(wheel_event(BlitzWheelDelta::Lines(0.0, 1.0)));
        pending.push(wheel_event(BlitzWheelDelta::Lines(0.0, 2.5)));

        let events = pending.take_coalesced();
        assert_eq!(events.len(), 1);
        match &events[0] {
            UiEvent::Wheel(event) => match &event.delta {
                BlitzWheelDelta::Lines(x, y) => {
                    assert_eq!(*x, 0.0);
                    assert_eq!(*y, 2.5);
                }
                delta => panic!("expected line delta, got {delta:?}"),
            },
            event => panic!("expected wheel event, got {event:?}"),
        }
    }

    #[test]
    fn pending_ui_events_keep_only_last_wheel_event_type_on_flush() {
        let mut pending = PendingUiEvents::default();
        pending.push(wheel_event(BlitzWheelDelta::Lines(0.0, 1.0)));
        pending.push(wheel_event(BlitzWheelDelta::Pixels(0.0, 1.0)));

        let events = pending.take_coalesced();
        assert_eq!(events.len(), 1);
        match &events[0] {
            UiEvent::Wheel(event) => match &event.delta {
                BlitzWheelDelta::Pixels(x, y) => {
                    assert_eq!(*x, 0.0);
                    assert_eq!(*y, 1.0);
                }
                delta => panic!("expected pixel delta, got {delta:?}"),
            },
            event => panic!("expected wheel event, got {event:?}"),
        }
    }
}
