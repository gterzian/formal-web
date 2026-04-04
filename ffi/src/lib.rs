mod content_bridge;
#[allow(dead_code)]
mod ui_event;

use anyrender::{PaintScene, Scene as RenderScene, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use blitz_dom::{BaseDocument, Document as BlitzDocument, DocumentConfig};
use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use blitz_html::HtmlDocument;
use content_process_protocol::{ContentCommand, PaintFrame, ScrollOffset};
use data_url::DataUrl;
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use kurbo::Affine;
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, c_char};
use std::panic::{self, AssertUnwindSafe};
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

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn lean_mk_string_from_bytes(value: *const c_char, size: usize) -> *mut lean_object;
    fn handleRuntimeMessage(message: *mut lean_object) -> *mut lean_object;
    fn startDocumentFetch(
        handler: usize,
        url: *mut lean_object,
        method: *mut lean_object,
        body: *mut lean_object,
    ) -> *mut lean_object;
    fn userAgentNoteRenderingOpportunity(message: *mut lean_object) -> *mut lean_object;
    fn leanIoResultMkOkUnit() -> *mut lean_object;
    fn leanIoResultMkOkUsize(value: usize) -> *mut lean_object;
    fn leanIoResultMkErrorFromBytes(
        value: *const c_char,
        size: usize,
    ) -> *mut lean_object;
    fn leanIoResultIsOk(result: *mut lean_object) -> u8;
    fn leanIoResultShowError(result: *mut lean_object);
    fn leanStringCstr(value: *mut lean_object) -> *const c_char;
    fn leanByteArraySize(value: *mut lean_object) -> usize;
    fn leanByteArrayCptr(value: *mut lean_object) -> *const u8;
    fn leanDec(value: *mut lean_object);
}

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";
static EVENT_LOOP_PROXY: LazyLock<Mutex<Option<EventLoopProxy<FormalWebUserEvent>>>> =
    LazyLock::new(|| Mutex::new(None));
static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<Option<(u32, u32, f32, ColorScheme)>>> =
    LazyLock::new(|| Mutex::new(None));

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";
const NEW_TOP_LEVEL_TRAVERSABLE_MESSAGE: &str = "NewTopLevelTraversable";
const DISPATCH_EVENT_MESSAGE_PREFIX: &str = "DispatchEvent|";

enum FormalWebUserEvent {
    Paint(PaintFrame),
    EmbedderRequestRedraw,
    NewTopLevelTraversable,
}

struct EmbedderPaintFrame {
    scene: RenderScene,
    viewport_scroll: ScrollOffset,
}

struct DataOnlyNetProvider;

struct DocumentFetchHandler {
    handler: Box<dyn NetHandler>,
}

struct FormalWebShellProvider;

impl ShellProvider for FormalWebShellProvider {
    fn request_redraw(&self) {
        with_event_loop_proxy(|proxy| {
            if let Some(proxy) = proxy {
                let _ = proxy.send_event(FormalWebUserEvent::EmbedderRequestRedraw);
            }
        });
    }
}

impl NetProvider for DataOnlyNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        match request.url.scheme() {
            "data" => match DataUrl::process(request.url.as_str()) {
                Ok(data_url) => match data_url.decode_to_vec() {
                    Ok((bytes, _fragment)) => handler.bytes(request.url.to_string(), Bytes::from(bytes)),
                    Err(_error) => {}
                },
                Err(_error) => {}
            },
            _scheme => {
                let handler = document_fetch_handler_pointer(handler);
                if let Err(error) = call_lean_document_fetch_start(handler, &request) {
                    drop_document_fetch_handler(handler);
                    eprintln!("failed to start document fetch: {error}");
                }
            }
        }
    }
}

fn document_fetch_handler_pointer(handler: Box<dyn NetHandler>) -> usize {
    Box::into_raw(Box::new(DocumentFetchHandler { handler })) as usize
}

fn drop_document_fetch_handler(pointer: usize) {
    if pointer == 0 {
        return;
    }

    unsafe {
        drop(Box::from_raw(pointer as *mut DocumentFetchHandler));
    }
}

fn request_body_string(body: &Body) -> String {
    match body {
        Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Body::Form(form) => serde_json::to_string(form).unwrap_or_default(),
        Body::Empty => String::new(),
    }
}

fn create_html_document_pointer(html: &str) -> usize {
    create_html_document_pointer_with_base_url(html, None)
}

fn create_html_document_pointer_with_base_url(html: &str, base_url: Option<String>) -> usize {
    let viewport = WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned")
        .as_ref()
        .map(|(width, height, scale, color_scheme)| {
            Viewport::new(*width, *height, *scale, *color_scheme)
        });
    let document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport,
            base_url,
            net_provider: Some(Arc::new(DataOnlyNetProvider)),
            shell_provider: Some(Arc::new(FormalWebShellProvider)),
            ..DocumentConfig::default()
        },
    );
    Box::into_raw(Box::new(document)) as usize
}

fn theme_to_color_scheme(theme: winit::window::Theme) -> ColorScheme {
    match theme {
        winit::window::Theme::Light => ColorScheme::Light,
        winit::window::Theme::Dark => ColorScheme::Dark,
    }
}

fn viewport_for_window(window: &Window) -> Viewport {
    let size = window.inner_size();
    let scale = window.scale_factor() as f32;
    let color_scheme = theme_to_color_scheme(window.theme().unwrap_or(winit::window::Theme::Light));
    Viewport::new(size.width, size.height, scale, color_scheme)
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

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
enum SerializableUiEvent {
    PointerMove(SerializablePointerEvent),
    PointerUp(SerializablePointerEvent),
    PointerDown(SerializablePointerEvent),
    Wheel(SerializableWheelEvent),
    KeyUp(SerializableKeyEvent),
    KeyDown(SerializableKeyEvent),
    Ime(SerializableImeEvent),
}

#[derive(Serialize, Deserialize)]
struct SerializablePointerEvent {
    id: SerializablePointerId,
    is_primary: bool,
    coords: SerializablePointerCoords,
    button: String,
    buttons: u8,
    modifiers: u32,
    details: SerializablePointerDetails,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
enum SerializablePointerId {
    Mouse,
    Pen,
    Finger(u64),
}

#[derive(Serialize, Deserialize)]
struct SerializablePointerCoords {
    page_x: f32,
    page_y: f32,
    screen_x: f32,
    screen_y: f32,
    client_x: f32,
    client_y: f32,
}

#[derive(Serialize, Deserialize)]
struct SerializablePointerDetails {
    pressure: f64,
    tangential_pressure: f32,
    tilt_x: i8,
    tilt_y: i8,
    twist: u16,
    altitude: f64,
    azimuth: f64,
}

#[derive(Serialize, Deserialize)]
struct SerializableWheelEvent {
    delta: SerializableWheelDelta,
    coords: SerializablePointerCoords,
    buttons: u8,
    modifiers: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
enum SerializableWheelDelta {
    Lines((f64, f64)),
    Pixels((f64, f64)),
}

#[derive(Serialize, Deserialize)]
struct SerializableKeyEvent {
    key: SerializableKey,
    code: String,
    modifiers: u32,
    location: String,
    is_auto_repeating: bool,
    is_composing: bool,
    state: String,
    text: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
enum SerializableKey {
    Character(String),
    Named(String),
    Unidentified,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
enum SerializableImeEvent {
    Enabled,
    Preedit { text: String, cursor: Option<(usize, usize)> },
    Commit(String),
    Disabled,
}

fn mouse_button_name(button: MouseEventButton) -> &'static str {
    match button {
        MouseEventButton::Main => "main",
        MouseEventButton::Auxiliary => "auxiliary",
        MouseEventButton::Secondary => "secondary",
        MouseEventButton::Fourth => "fourth",
        MouseEventButton::Fifth => "fifth",
    }
}

fn mouse_button_from_name(name: &str) -> Result<MouseEventButton, String> {
    match name {
        "main" => Ok(MouseEventButton::Main),
        "auxiliary" => Ok(MouseEventButton::Auxiliary),
        "secondary" => Ok(MouseEventButton::Secondary),
        "fourth" => Ok(MouseEventButton::Fourth),
        "fifth" => Ok(MouseEventButton::Fifth),
        _ => Err(format!("unknown mouse button: {name}")),
    }
}

fn modifiers_bits(modifiers: KeyboardModifiers) -> u32 {
    modifiers.bits()
}

fn modifiers_from_bits(bits: u32) -> KeyboardModifiers {
    let mut modifiers = KeyboardModifiers::default();
    if bits & KeyboardModifiers::CONTROL.bits() != 0 {
        modifiers.insert(KeyboardModifiers::CONTROL);
    }
    if bits & KeyboardModifiers::ALT.bits() != 0 {
        modifiers.insert(KeyboardModifiers::ALT);
    }
    if bits & KeyboardModifiers::SHIFT.bits() != 0 {
        modifiers.insert(KeyboardModifiers::SHIFT);
    }
    if bits & KeyboardModifiers::SUPER.bits() != 0 {
        modifiers.insert(KeyboardModifiers::SUPER);
    }
    modifiers
}

fn location_name(location: Location) -> &'static str {
    match location {
        Location::Standard => "standard",
        Location::Left => "left",
        Location::Right => "right",
        Location::Numpad => "numpad",
    }
}

fn location_from_name(name: &str) -> Result<Location, String> {
    match name {
        "standard" => Ok(Location::Standard),
        "left" => Ok(Location::Left),
        "right" => Ok(Location::Right),
        "numpad" => Ok(Location::Numpad),
        _ => Err(format!("unknown key location: {name}")),
    }
}

fn key_state_name(state: KeyState) -> &'static str {
    match state {
        KeyState::Pressed => "pressed",
        KeyState::Released => "released",
    }
}

fn key_state_from_name(name: &str) -> Result<KeyState, String> {
    match name {
        "pressed" => Ok(KeyState::Pressed),
        "released" => Ok(KeyState::Released),
        _ => Err(format!("unknown key state: {name}")),
    }
}

fn key_to_serializable(key: &Key) -> SerializableKey {
    match key {
        Key::Character(value) => SerializableKey::Character(value.clone()),
        Key::Alt => SerializableKey::Named(String::from("Alt")),
        Key::Backspace => SerializableKey::Named(String::from("Backspace")),
        Key::Control => SerializableKey::Named(String::from("Control")),
        Key::Delete => SerializableKey::Named(String::from("Delete")),
        Key::ArrowDown => SerializableKey::Named(String::from("ArrowDown")),
        Key::End => SerializableKey::Named(String::from("End")),
        Key::Enter => SerializableKey::Named(String::from("Enter")),
        Key::Escape => SerializableKey::Named(String::from("Escape")),
        Key::Home => SerializableKey::Named(String::from("Home")),
        Key::ArrowLeft => SerializableKey::Named(String::from("ArrowLeft")),
        Key::Meta => SerializableKey::Named(String::from("Meta")),
        Key::PageDown => SerializableKey::Named(String::from("PageDown")),
        Key::PageUp => SerializableKey::Named(String::from("PageUp")),
        Key::ArrowRight => SerializableKey::Named(String::from("ArrowRight")),
        Key::Shift => SerializableKey::Named(String::from("Shift")),
        Key::Tab => SerializableKey::Named(String::from("Tab")),
        Key::ArrowUp => SerializableKey::Named(String::from("ArrowUp")),
        Key::Super => SerializableKey::Named(String::from("Super")),
        Key::Unidentified => SerializableKey::Unidentified,
        _ => SerializableKey::Unidentified,
    }
}

fn key_from_serializable(key: SerializableKey) -> Result<Key, String> {
    match key {
        SerializableKey::Character(value) => Ok(Key::Character(value)),
        SerializableKey::Named(name) => match name.as_str() {
            "Alt" => Ok(Key::Alt),
            "Backspace" => Ok(Key::Backspace),
            "Control" => Ok(Key::Control),
            "Delete" => Ok(Key::Delete),
            "ArrowDown" => Ok(Key::ArrowDown),
            "End" => Ok(Key::End),
            "Enter" => Ok(Key::Enter),
            "Escape" => Ok(Key::Escape),
            "Home" => Ok(Key::Home),
            "ArrowLeft" => Ok(Key::ArrowLeft),
            "Meta" => Ok(Key::Meta),
            "PageDown" => Ok(Key::PageDown),
            "PageUp" => Ok(Key::PageUp),
            "ArrowRight" => Ok(Key::ArrowRight),
            "Shift" => Ok(Key::Shift),
            "Tab" => Ok(Key::Tab),
            "ArrowUp" => Ok(Key::ArrowUp),
            "Super" => Ok(Key::Super),
            _ => Err(format!("unknown key variant: {name}")),
        },
        SerializableKey::Unidentified => Ok(Key::Unidentified),
    }
}

fn code_name(code: &Code) -> &'static str {
    match code {
        Code::Backquote => "Backquote",
        Code::Backslash => "Backslash",
        Code::Backspace => "Backspace",
        Code::BracketLeft => "BracketLeft",
        Code::BracketRight => "BracketRight",
        Code::Comma => "Comma",
        Code::ControlLeft => "ControlLeft",
        Code::ControlRight => "ControlRight",
        Code::Delete => "Delete",
        Code::Digit0 => "Digit0",
        Code::Digit1 => "Digit1",
        Code::Digit2 => "Digit2",
        Code::Digit3 => "Digit3",
        Code::Digit4 => "Digit4",
        Code::Digit5 => "Digit5",
        Code::Digit6 => "Digit6",
        Code::Digit7 => "Digit7",
        Code::Digit8 => "Digit8",
        Code::Digit9 => "Digit9",
        Code::ArrowDown => "ArrowDown",
        Code::End => "End",
        Code::Enter => "Enter",
        Code::Equal => "Equal",
        Code::Escape => "Escape",
        Code::Home => "Home",
        Code::KeyA => "KeyA",
        Code::KeyB => "KeyB",
        Code::KeyC => "KeyC",
        Code::KeyD => "KeyD",
        Code::KeyE => "KeyE",
        Code::KeyF => "KeyF",
        Code::KeyG => "KeyG",
        Code::KeyH => "KeyH",
        Code::KeyI => "KeyI",
        Code::KeyJ => "KeyJ",
        Code::KeyK => "KeyK",
        Code::KeyL => "KeyL",
        Code::KeyM => "KeyM",
        Code::KeyN => "KeyN",
        Code::KeyO => "KeyO",
        Code::KeyP => "KeyP",
        Code::KeyQ => "KeyQ",
        Code::KeyR => "KeyR",
        Code::KeyS => "KeyS",
        Code::KeyT => "KeyT",
        Code::KeyU => "KeyU",
        Code::KeyV => "KeyV",
        Code::KeyW => "KeyW",
        Code::KeyX => "KeyX",
        Code::KeyY => "KeyY",
        Code::KeyZ => "KeyZ",
        Code::Super => "Super",
        Code::Minus => "Minus",
        Code::PageDown => "PageDown",
        Code::PageUp => "PageUp",
        Code::Period => "Period",
        Code::Quote => "Quote",
        Code::ArrowLeft => "ArrowLeft",
        Code::ArrowRight => "ArrowRight",
        Code::Semicolon => "Semicolon",
        Code::ShiftLeft => "ShiftLeft",
        Code::ShiftRight => "ShiftRight",
        Code::Slash => "Slash",
        Code::Space => "Space",
        Code::Tab => "Tab",
        Code::ArrowUp => "ArrowUp",
        Code::Unidentified => "Unidentified",
        _ => "Unidentified",
    }
}

fn code_from_name(name: &str) -> Result<Code, String> {
    match name {
        "Backquote" => Ok(Code::Backquote),
        "Backslash" => Ok(Code::Backslash),
        "Backspace" => Ok(Code::Backspace),
        "BracketLeft" => Ok(Code::BracketLeft),
        "BracketRight" => Ok(Code::BracketRight),
        "Comma" => Ok(Code::Comma),
        "ControlLeft" => Ok(Code::ControlLeft),
        "ControlRight" => Ok(Code::ControlRight),
        "Delete" => Ok(Code::Delete),
        "Digit0" => Ok(Code::Digit0),
        "Digit1" => Ok(Code::Digit1),
        "Digit2" => Ok(Code::Digit2),
        "Digit3" => Ok(Code::Digit3),
        "Digit4" => Ok(Code::Digit4),
        "Digit5" => Ok(Code::Digit5),
        "Digit6" => Ok(Code::Digit6),
        "Digit7" => Ok(Code::Digit7),
        "Digit8" => Ok(Code::Digit8),
        "Digit9" => Ok(Code::Digit9),
        "ArrowDown" => Ok(Code::ArrowDown),
        "End" => Ok(Code::End),
        "Enter" => Ok(Code::Enter),
        "Equal" => Ok(Code::Equal),
        "Escape" => Ok(Code::Escape),
        "Home" => Ok(Code::Home),
        "KeyA" => Ok(Code::KeyA),
        "KeyB" => Ok(Code::KeyB),
        "KeyC" => Ok(Code::KeyC),
        "KeyD" => Ok(Code::KeyD),
        "KeyE" => Ok(Code::KeyE),
        "KeyF" => Ok(Code::KeyF),
        "KeyG" => Ok(Code::KeyG),
        "KeyH" => Ok(Code::KeyH),
        "KeyI" => Ok(Code::KeyI),
        "KeyJ" => Ok(Code::KeyJ),
        "KeyK" => Ok(Code::KeyK),
        "KeyL" => Ok(Code::KeyL),
        "KeyM" => Ok(Code::KeyM),
        "KeyN" => Ok(Code::KeyN),
        "KeyO" => Ok(Code::KeyO),
        "KeyP" => Ok(Code::KeyP),
        "KeyQ" => Ok(Code::KeyQ),
        "KeyR" => Ok(Code::KeyR),
        "KeyS" => Ok(Code::KeyS),
        "KeyT" => Ok(Code::KeyT),
        "KeyU" => Ok(Code::KeyU),
        "KeyV" => Ok(Code::KeyV),
        "KeyW" => Ok(Code::KeyW),
        "KeyX" => Ok(Code::KeyX),
        "KeyY" => Ok(Code::KeyY),
        "KeyZ" => Ok(Code::KeyZ),
        "Super" => Ok(Code::Super),
        "Minus" => Ok(Code::Minus),
        "PageDown" => Ok(Code::PageDown),
        "PageUp" => Ok(Code::PageUp),
        "Period" => Ok(Code::Period),
        "Quote" => Ok(Code::Quote),
        "ArrowLeft" => Ok(Code::ArrowLeft),
        "ArrowRight" => Ok(Code::ArrowRight),
        "Semicolon" => Ok(Code::Semicolon),
        "ShiftLeft" => Ok(Code::ShiftLeft),
        "ShiftRight" => Ok(Code::ShiftRight),
        "Slash" => Ok(Code::Slash),
        "Space" => Ok(Code::Space),
        "Tab" => Ok(Code::Tab),
        "ArrowUp" => Ok(Code::ArrowUp),
        "Unidentified" => Ok(Code::Unidentified),
        _ => Err(format!("unknown key code: {name}")),
    }
}

impl From<&BlitzPointerEvent> for SerializablePointerEvent {
    fn from(event: &BlitzPointerEvent) -> Self {
        Self {
            id: match event.id {
                BlitzPointerId::Mouse => SerializablePointerId::Mouse,
                BlitzPointerId::Pen => SerializablePointerId::Pen,
                BlitzPointerId::Finger(value) => SerializablePointerId::Finger(value),
            },
            is_primary: event.is_primary,
            coords: SerializablePointerCoords {
                page_x: event.coords.page_x,
                page_y: event.coords.page_y,
                screen_x: event.coords.screen_x,
                screen_y: event.coords.screen_y,
                client_x: event.coords.client_x,
                client_y: event.coords.client_y,
            },
            button: String::from(mouse_button_name(event.button)),
            buttons: event.buttons.bits(),
            modifiers: modifiers_bits(event.mods),
            details: SerializablePointerDetails {
                pressure: event.details.pressure,
                tangential_pressure: event.details.tangential_pressure,
                tilt_x: event.details.tilt_x,
                tilt_y: event.details.tilt_y,
                twist: event.details.twist,
                altitude: event.details.altitude,
                azimuth: event.details.azimuth,
            },
        }
    }
}

impl TryFrom<SerializablePointerEvent> for BlitzPointerEvent {
    type Error = String;

    fn try_from(event: SerializablePointerEvent) -> Result<Self, Self::Error> {
        Ok(Self {
            id: match event.id {
                SerializablePointerId::Mouse => BlitzPointerId::Mouse,
                SerializablePointerId::Pen => BlitzPointerId::Pen,
                SerializablePointerId::Finger(value) => BlitzPointerId::Finger(value),
            },
            is_primary: event.is_primary,
            coords: PointerCoords {
                page_x: event.coords.page_x,
                page_y: event.coords.page_y,
                screen_x: event.coords.screen_x,
                screen_y: event.coords.screen_y,
                client_x: event.coords.client_x,
                client_y: event.coords.client_y,
            },
            button: mouse_button_from_name(&event.button)?,
            buttons: MouseEventButtons::from_bits_retain(event.buttons),
            mods: modifiers_from_bits(event.modifiers),
            details: PointerDetails {
                pressure: event.details.pressure,
                tangential_pressure: event.details.tangential_pressure,
                tilt_x: event.details.tilt_x,
                tilt_y: event.details.tilt_y,
                twist: event.details.twist,
                altitude: event.details.altitude,
                azimuth: event.details.azimuth,
            },
        })
    }
}

impl From<&BlitzWheelEvent> for SerializableWheelEvent {
    fn from(event: &BlitzWheelEvent) -> Self {
        Self {
            delta: match event.delta {
                BlitzWheelDelta::Lines(x, y) => SerializableWheelDelta::Lines((x, y)),
                BlitzWheelDelta::Pixels(x, y) => SerializableWheelDelta::Pixels((x, y)),
            },
            coords: SerializablePointerCoords {
                page_x: event.coords.page_x,
                page_y: event.coords.page_y,
                screen_x: event.coords.screen_x,
                screen_y: event.coords.screen_y,
                client_x: event.coords.client_x,
                client_y: event.coords.client_y,
            },
            buttons: event.buttons.bits(),
            modifiers: modifiers_bits(event.mods),
        }
    }
}

impl From<SerializableWheelEvent> for BlitzWheelEvent {
    fn from(event: SerializableWheelEvent) -> Self {
        Self {
            delta: match event.delta {
                SerializableWheelDelta::Lines((x, y)) => BlitzWheelDelta::Lines(x, y),
                SerializableWheelDelta::Pixels((x, y)) => BlitzWheelDelta::Pixels(x, y),
            },
            coords: PointerCoords {
                page_x: event.coords.page_x,
                page_y: event.coords.page_y,
                screen_x: event.coords.screen_x,
                screen_y: event.coords.screen_y,
                client_x: event.coords.client_x,
                client_y: event.coords.client_y,
            },
            buttons: MouseEventButtons::from_bits_retain(event.buttons),
            mods: modifiers_from_bits(event.modifiers),
        }
    }
}

impl From<&BlitzKeyEvent> for SerializableKeyEvent {
    fn from(event: &BlitzKeyEvent) -> Self {
        Self {
            key: key_to_serializable(&event.key),
            code: String::from(code_name(&event.code)),
            modifiers: modifiers_bits(event.modifiers),
            location: String::from(location_name(event.location)),
            is_auto_repeating: event.is_auto_repeating,
            is_composing: event.is_composing,
            state: String::from(key_state_name(event.state)),
            text: event.text.as_ref().map(ToString::to_string),
        }
    }
}

impl TryFrom<SerializableKeyEvent> for BlitzKeyEvent {
    type Error = String;

    fn try_from(event: SerializableKeyEvent) -> Result<Self, Self::Error> {
        Ok(Self {
            key: key_from_serializable(event.key)?,
            code: code_from_name(&event.code)?,
            modifiers: modifiers_from_bits(event.modifiers),
            location: location_from_name(&event.location)?,
            is_auto_repeating: event.is_auto_repeating,
            is_composing: event.is_composing,
            state: key_state_from_name(&event.state)?,
            text: event.text.map(Into::into),
        })
    }
}

impl From<&BlitzImeEvent> for SerializableImeEvent {
    fn from(event: &BlitzImeEvent) -> Self {
        match event {
            BlitzImeEvent::Enabled => Self::Enabled,
            BlitzImeEvent::Preedit(text, cursor) => Self::Preedit {
                text: text.clone(),
                cursor: *cursor,
            },
            BlitzImeEvent::Commit(text) => Self::Commit(text.clone()),
            BlitzImeEvent::Disabled => Self::Disabled,
            BlitzImeEvent::DeleteSurrounding { .. } => Self::Disabled,
        }
    }
}

impl From<SerializableImeEvent> for BlitzImeEvent {
    fn from(event: SerializableImeEvent) -> Self {
        match event {
            SerializableImeEvent::Enabled => Self::Enabled,
            SerializableImeEvent::Preedit { text, cursor } => Self::Preedit(text, cursor),
            SerializableImeEvent::Commit(text) => Self::Commit(text),
            SerializableImeEvent::Disabled => Self::Disabled,
        }
    }
}

impl From<&UiEvent> for SerializableUiEvent {
    fn from(event: &UiEvent) -> Self {
        match event {
            UiEvent::PointerMove(data) => Self::PointerMove(data.into()),
            UiEvent::PointerUp(data) => Self::PointerUp(data.into()),
            UiEvent::PointerDown(data) => Self::PointerDown(data.into()),
            UiEvent::Wheel(data) => Self::Wheel(data.into()),
            UiEvent::KeyUp(data) => Self::KeyUp(data.into()),
            UiEvent::KeyDown(data) => Self::KeyDown(data.into()),
            UiEvent::Ime(data) => Self::Ime(data.into()),
        }
    }
}

impl TryFrom<SerializableUiEvent> for UiEvent {
    type Error = String;

    fn try_from(event: SerializableUiEvent) -> Result<Self, Self::Error> {
        match event {
            SerializableUiEvent::PointerMove(data) => Ok(Self::PointerMove(data.try_into()?)),
            SerializableUiEvent::PointerUp(data) => Ok(Self::PointerUp(data.try_into()?)),
            SerializableUiEvent::PointerDown(data) => Ok(Self::PointerDown(data.try_into()?)),
            SerializableUiEvent::Wheel(data) => Ok(Self::Wheel(data.into())),
            SerializableUiEvent::KeyUp(data) => Ok(Self::KeyUp(data.try_into()?)),
            SerializableUiEvent::KeyDown(data) => Ok(Self::KeyDown(data.try_into()?)),
            SerializableUiEvent::Ime(data) => Ok(Self::Ime(data.into())),
        }
    }
}

fn serialize_ui_event(event: &UiEvent) -> Result<String, String> {
    serde_json::to_string(&SerializableUiEvent::from(event))
        .map_err(|error| format!("failed to serialize UI event: {error}"))
}

fn deserialize_ui_event(message: &str) -> Result<UiEvent, String> {
    let event: SerializableUiEvent = serde_json::from_str(message)
        .map_err(|error| format!("failed to deserialize UI event: {error}"))?;
    event.try_into()
}

fn dispatch_event_runtime_message(event: UiEvent) {
    let Ok(event_message) = serialize_ui_event(&event) else {
        return;
    };
    let message = format!("{DISPATCH_EVENT_MESSAGE_PREFIX}{event_message}");
    call_lean_runtime_message_handler(&message);
}

fn lean_string_from_owned(value: String) -> *mut lean_object {
    unsafe { lean_mk_string_from_bytes(value.as_ptr() as *const c_char, value.len()) }
}

fn ok_unit_result() -> *mut lean_object {
    unsafe { leanIoResultMkOkUnit() }
}

fn ok_usize_result(value: usize) -> *mut lean_object {
    unsafe { leanIoResultMkOkUsize(value) }
}

fn error_result(message: &str) -> *mut lean_object {
    unsafe { leanIoResultMkErrorFromBytes(message.as_ptr() as *const c_char, message.len()) }
}

fn call_lean_runtime_message_handler(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { handleRuntimeMessage(lean_message) };

    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
    }

    unsafe { leanDec(io_result) };
}

fn call_lean_document_fetch_start_parts(
    handler: usize,
    url: &str,
    method: &str,
    body: &str,
) -> Result<(), String> {
    let lean_url = lean_string_from_owned(url.to_owned());
    let lean_method = lean_string_from_owned(method.to_owned());
    let lean_body = lean_string_from_owned(body.to_owned());
    let io_result = unsafe {
        startDocumentFetch(handler, lean_url, lean_method, lean_body)
    };

    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean document fetch start failed"));
    }

    unsafe { leanDec(io_result) };
    Ok(())
}

fn call_lean_document_fetch_start(handler: usize, request: &Request) -> Result<(), String> {
    call_lean_document_fetch_start_parts(
        handler,
        &request.url.to_string(),
        &request.method.to_string(),
        &request_body_string(&request.body),
    )
}

fn startup_runtime_message() -> Result<String, String> {
    let artifact_path = startup_artifact_path()?;
    Ok(format!("FreshTopLevelTraversable|file://{}", artifact_path.display()))
}

fn startup_artifact_path() -> Result<PathBuf, String> {
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let artifact_path: PathBuf = current_dir.join(STARTUP_ARTIFACT_RELATIVE_PATH);
    artifact_path
        .canonicalize()
        .map_err(|error| format!("failed to resolve startup artifact path: {error}"))
}

fn user_event_of_runtime_message(message: &str) -> Result<FormalWebUserEvent, String> {
    match message {
        NEW_TOP_LEVEL_TRAVERSABLE_MESSAGE => Ok(FormalWebUserEvent::NewTopLevelTraversable),
        _ => Err(format!("unknown runtime message: {message}")),
    }
}

fn user_agent_note_rendering_opportunity(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { userAgentNoteRenderingOpportunity(lean_message) };

    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
    }

    unsafe { leanDec(io_result) };
}

fn with_event_loop_proxy<R>(f: impl FnOnce(&Option<EventLoopProxy<FormalWebUserEvent>>) -> R) -> R {
    let guard = EVENT_LOOP_PROXY
        .lock()
        .expect("event loop proxy mutex poisoned");
    f(&guard)
}

fn queue_paint(pointer: usize) -> Result<(), String> {
    let _ = pointer;
    Err(String::from(
        "legacy in-process paint is unsupported; use content-process recorded paint frames",
    ))
}

struct FormalWebApp {
    window: Option<Arc<Window>>,
    renderer: VelloWindowRenderer,
    current_paint_frame: Option<EmbedderPaintFrame>,
    saw_redraw_requested: bool,
    has_top_level_traversable: bool,
    window_occluded: bool,
    animation_timer: Option<Instant>,
    keyboard_modifiers: Modifiers,
    buttons: MouseEventButtons,
    pointer_pos: PhysicalPosition<f64>,
}

impl Default for FormalWebApp {
    fn default() -> Self {
        Self {
            window: None,
            renderer: VelloWindowRenderer::new(),
            current_paint_frame: None,
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

    fn request_visible_redraw(&self, reason: &str) {
        if !self.has_visible_viewport() {
            return;
        }

        let Some(window) = self.window.as_ref() else {
            return;
        };
        window.request_redraw();
        user_agent_note_rendering_opportunity(reason);
    }

    fn paint_frame(snapshot: PaintFrame) -> EmbedderPaintFrame {
        EmbedderPaintFrame {
            scene: snapshot.scene.into(),
            viewport_scroll: snapshot.viewport_scroll,
        }
    }

    fn update_window_viewport_snapshot(window: &Window) {
        let viewport = viewport_for_window(window);
        let viewport_snapshot = (
            viewport.window_size.0,
            viewport.window_size.1,
            viewport.hidpi_scale,
            viewport.color_scheme,
        );
        let mut snapshot = WINDOW_VIEWPORT_SNAPSHOT
            .lock()
            .expect("window viewport snapshot mutex poisoned");
        *snapshot = Some(viewport_snapshot);
        content_bridge::broadcast_viewport(Some(viewport_snapshot));
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
        let attributes: WindowAttributes = Window::default_attributes()
            .with_title("formal-web winit demo");
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
        let Some(current_paint_frame) = self.current_paint_frame.as_ref() else {
            return;
        };
        let size = window.inner_size();

        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height);
        }

        let scene_fragment = current_paint_frame.scene.clone();
        self.renderer.render(|scene| {
            scene.append_scene(scene_fragment, Affine::IDENTITY);
        });
    }

    fn pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        if let Some(current_paint_frame) = self.current_paint_frame.as_ref() {
            let scale = self
                .window
                .as_ref()
                .map(|window| window.scale_factor())
                .unwrap_or(1.0);
            let LogicalPosition::<f32> {
                x: screen_x,
                y: screen_y,
            } = position.to_logical(scale);
            let client_x = screen_x;
            let client_y = screen_y;
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
            let scale = self
                .window
                .as_ref()
                .map(|window| window.scale_factor())
                .unwrap_or(1.0);
            let LogicalPosition::<f32> {
                x: screen_x,
                y: screen_y,
            } = position.to_logical(scale);
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

    fn dispatch_ui_event(&mut self, event: UiEvent) {
        if !self.has_visible_viewport() {
            return;
        }
        dispatch_event_runtime_message(event);
        if self.has_top_level_traversable {
            self.request_visible_redraw("ui_event");
        }
    }
}

impl ApplicationHandler<FormalWebUserEvent> for FormalWebApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match Self::create_window(event_loop) {
                Ok(window) => {
                    Self::update_window_viewport_snapshot(&window);
                    self.resume_renderer_for_window(&window);
                    self.window = Some(window);
                    match startup_runtime_message() {
                        Ok(message) => call_lean_runtime_message_handler(&message),
                        Err(_error) => event_loop.exit(),
                    }
                }
                Err(error) => {
                    let _ = error;
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(current_window_id) = self.window.as_ref().map(|window| window.id()) else {
            return;
        };

        if current_window_id != window_id {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                self.saw_redraw_requested = true;
                if self.current_paint_frame.is_some() {
                    self.paint_current_frame();
                    self.saw_redraw_requested = false;
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.window_occluded = occluded;
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = self.window.as_ref() {
                    Self::update_window_viewport_snapshot(window);
                }
                if self.renderer.is_active() {
                    self.renderer.set_size(size.width, size.height);
                }
                if self.has_top_level_traversable {
                    self.request_visible_redraw("request_redraw");
                }
            }
            WindowEvent::CloseRequested => {
                self.renderer.suspend();
                self.animation_timer = None;
                self.current_paint_frame = None;
                self.has_top_level_traversable = false;
                self.window_occluded = false;
                if let Ok(mut snapshot) = WINDOW_VIEWPORT_SNAPSHOT.lock() {
                    *snapshot = None;
                }
                content_bridge::broadcast_viewport(None);
                self.window = None;
                event_loop.exit();
            }
            WindowEvent::Destroyed => {
                self.renderer.suspend();
                self.animation_timer = None;
                self.current_paint_frame = None;
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
                self.dispatch_ui_event(UiEvent::Ime(winit_ime_to_blitz(ime_event)));
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
                self.dispatch_ui_event(event);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer_pos = position;
                if !self.pointer_position_in_viewport(position) {
                    return;
                }
                self.dispatch_ui_event(UiEvent::PointerMove(BlitzPointerEvent {
                    id: BlitzPointerId::Mouse,
                    is_primary: true,
                    coords: self.pointer_coords(position),
                    button: Default::default(),
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    details: PointerDetails::default(),
                }));
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if !self.pointer_position_in_viewport(self.pointer_pos) {
                    return;
                }
                let coords = self.pointer_coords(self.pointer_pos);
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

                let event = BlitzPointerEvent {
                    id: BlitzPointerId::Mouse,
                    is_primary: true,
                    coords,
                    button: mapped_button,
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    details: PointerDetails::default(),
                };
                self.dispatch_ui_event(match state {
                    ElementState::Pressed => UiEvent::PointerDown(event),
                    ElementState::Released => UiEvent::PointerUp(event),
                });
            }
            WindowEvent::Touch(Touch {
                phase,
                location,
                force,
                id,
                ..
            }) => {
                if !self.pointer_position_in_viewport(location) {
                    return;
                }
                let coords = self.pointer_coords(location);
                let event = BlitzPointerEvent {
                    id: BlitzPointerId::Finger(id),
                    is_primary: true,
                    coords,
                    button: Default::default(),
                    buttons: MouseEventButtons::None,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    details: touch_pointer_details(force),
                };
                match phase {
                    TouchPhase::Started => self.dispatch_ui_event(UiEvent::PointerDown(event)),
                    TouchPhase::Moved => self.dispatch_ui_event(UiEvent::PointerMove(event)),
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.dispatch_ui_event(UiEvent::PointerUp(event))
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
                self.dispatch_ui_event(UiEvent::Wheel(BlitzWheelEvent {
                    delta,
                    coords: self.pointer_coords(self.pointer_pos),
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                }));
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::Paint(snapshot) => {
                let Some(_window) = self.window.as_ref() else {
                    return;
                };

                self.current_paint_frame = Some(Self::paint_frame(snapshot));

                if self.saw_redraw_requested {
                    self.paint_current_frame();
                    self.saw_redraw_requested = false;
                }
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

#[unsafe(no_mangle)]
pub extern "C" fn createEmptyHtmlDocument(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(EMPTY_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn createLoadedHtmlDocument(
    base_url: *mut lean_object,
    html: *mut lean_object,
) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| {
        let c_base_url = unsafe { leanStringCstr(base_url) };
        let base_url = unsafe { CStr::from_ptr(c_base_url) }
            .to_string_lossy()
            .into_owned();
        let c_html = unsafe { leanStringCstr(html) };
        let html = unsafe { CStr::from_ptr(c_html) }
            .to_string_lossy()
            .into_owned();
        create_html_document_pointer_with_base_url(&html, Some(base_url))
    }))
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn renderHtmlDocument(pointer: usize) -> *mut lean_object {
    let html = panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            String::from("<null rust document pointer>")
        } else {
            let document = unsafe { &*(pointer as *const HtmlDocument) };
            document.root_element().outer_html()
        }
    }))
    .unwrap_or_else(|_| String::from("<panic rendering rust document>"));

    lean_string_from_owned(html)
}

#[unsafe(no_mangle)]
pub extern "C" fn extractBaseDocument(pointer: usize) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            0
        } else {
            let document = unsafe { &*(pointer as *const HtmlDocument) };
            let base_document: *const BaseDocument = &**document;
            base_document as usize
        }
    }))
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn queuePaint(pointer: usize, _: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| queue_paint(pointer))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic queueing paint event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn applyUiEvent(
    pointer: usize,
    event: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            return Err(String::from("cannot apply UI event to null BaseDocument pointer"));
        }

        let c_event = unsafe { leanStringCstr(event) };
        let event = unsafe { CStr::from_ptr(c_event) }
            .to_string_lossy()
            .into_owned();
        let event = deserialize_ui_event(&event)?;

        let base_document = unsafe { &mut *(pointer as *mut BaseDocument) };
        BlitzDocument::handle_ui_event(base_document, event);
        Ok(())
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic applying UI event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn completeDocumentFetch(
    handler: usize,
    resolved_url: *mut lean_object,
    bytes: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        if handler == 0 {
            return Err(String::from(
                "cannot complete document fetch with null handler pointer",
            ));
        }

        let c_resolved_url = unsafe { leanStringCstr(resolved_url) };
        let resolved_url = unsafe { CStr::from_ptr(c_resolved_url) }
            .to_string_lossy()
            .into_owned();

        let size = unsafe { leanByteArraySize(bytes) };
        let bytes_ptr = unsafe { leanByteArrayCptr(bytes) };
        let payload = unsafe { std::slice::from_raw_parts(bytes_ptr, size) };

        let handler = unsafe { Box::from_raw(handler as *mut DocumentFetchHandler) };
        handler
            .handler
            .bytes(resolved_url, Bytes::copy_from_slice(payload));
        Ok(())
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic completing document fetch"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sendEmbedderMessage(message: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_message = unsafe { leanStringCstr(message) };
        let message = unsafe { CStr::from_ptr(c_message) }
            .to_string_lossy()
            .into_owned();
        let user_event = user_event_of_runtime_message(&message)?;
        with_event_loop_proxy(|proxy| match proxy {
            Some(proxy) => proxy
                .send_event(user_event)
                .map_err(|error| format!("failed to send runtime message event: {error}")),
            None => Err(String::from("winit event loop proxy is not initialized")),
        })
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic sending runtime message"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn runEmbedderEventLoop(_: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
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
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running winit event loop"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStart(
    event_loop_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        content_bridge::start(event_loop_id)
    })) {
        Ok(Ok(handle)) => ok_usize_result(handle),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic starting content process"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStop(
    handle: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| content_bridge::stop(handle))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic stopping content process"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCreateEmptyDocument(
    handle: usize,
    document_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        content_bridge::send_command(
            handle,
            ContentCommand::CreateEmptyDocument {
                document_id: document_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic creating content-process document"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCreateLoadedDocument(
    handle: usize,
    document_id: usize,
    url: *mut lean_object,
    body: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_url = unsafe { leanStringCstr(url) };
        let url = unsafe { CStr::from_ptr(c_url) }.to_string_lossy().into_owned();
        let c_body = unsafe { leanStringCstr(body) };
        let body = unsafe { CStr::from_ptr(c_body) }.to_string_lossy().into_owned();
        content_bridge::send_command(
            handle,
            ContentCommand::CreateLoadedDocument {
                document_id: document_id as u64,
                url,
                body,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic creating loaded content-process document"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessDispatchEvent(
    handle: usize,
    document_id: usize,
    event: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_event = unsafe { leanStringCstr(event) };
        let event = unsafe { CStr::from_ptr(c_event) }.to_string_lossy().into_owned();
        content_bridge::send_command(
            handle,
            ContentCommand::DispatchEvent {
                document_id: document_id as u64,
                event,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic dispatching content-process event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessUpdateTheRendering(
    handle: usize,
    document_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        content_bridge::send_command(
            handle,
            ContentCommand::UpdateTheRendering {
                document_id: document_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running content-process update-the-rendering"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCompleteDocumentFetch(
    handle: usize,
    handler_id: usize,
    resolved_url: *mut lean_object,
    bytes: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_resolved_url = unsafe { leanStringCstr(resolved_url) };
        let resolved_url = unsafe { CStr::from_ptr(c_resolved_url) }
            .to_string_lossy()
            .into_owned();
        let size = unsafe { leanByteArraySize(bytes) };
        let bytes_ptr = unsafe { leanByteArrayCptr(bytes) };
        let payload = unsafe { std::slice::from_raw_parts(bytes_ptr, size) };
        content_bridge::send_command(
            handle,
            ContentCommand::CompleteDocumentFetch {
                handler_id: handler_id as u64,
                resolved_url,
                body: payload.to_vec(),
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic completing content-process fetch"),
    }
}