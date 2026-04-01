use anyrender::WindowRenderer;
use anyrender_vello::VelloWindowRenderer;
use blitz_dom::{BaseDocument, Document as BlitzDocument, DocumentConfig};
use blitz_paint::paint_scene;
use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use blitz_html::HtmlDocument;
use data_url::DataUrl;
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
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
    fn formal_web_handle_runtime_message(message: *mut lean_object) -> *mut lean_object;
    fn formal_web_user_agent_note_rendering_opportunity(message: *mut lean_object) -> *mut lean_object;
    fn formal_web_lean_io_result_mk_ok_unit() -> *mut lean_object;
    fn formal_web_lean_io_result_mk_error_from_bytes(
        value: *const c_char,
        size: usize,
    ) -> *mut lean_object;
    fn formal_web_lean_io_result_is_ok(result: *mut lean_object) -> u8;
    fn formal_web_lean_io_result_show_error(result: *mut lean_object);
    fn formal_web_lean_string_cstr(value: *mut lean_object) -> *const c_char;
    fn formal_web_lean_dec(value: *mut lean_object);
}

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";
const LOADED_HTML_DOCUMENT: &str = include_str!("../../artifacts/StartupExample.html");

static EVENT_LOOP_PROXY: LazyLock<Mutex<Option<EventLoopProxy<FormalWebUserEvent>>>> =
    LazyLock::new(|| Mutex::new(None));
static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<Option<(u32, u32, f32, ColorScheme)>>> =
    LazyLock::new(|| Mutex::new(None));

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";
const NEW_TOP_LEVEL_TRAVERSABLE_MESSAGE: &str = "NewTopLevelTraversable";
const DISPATCH_EVENT_MESSAGE_PREFIX: &str = "DispatchEvent|";

enum FormalWebUserEvent {
    Paint(usize),
    DocumentRequestRedraw,
    NewTopLevelTraversable,
}

struct DataOnlyNetProvider;

struct FormalWebShellProvider;

impl ShellProvider for FormalWebShellProvider {
    fn request_redraw(&self) {
        with_event_loop_proxy(|proxy| {
            if let Some(proxy) = proxy {
                let _ = proxy.send_event(FormalWebUserEvent::DocumentRequestRedraw);
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
            _scheme => {}
        }
    }
}

fn create_html_document_pointer(html: &str) -> usize {
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
    unsafe { formal_web_lean_io_result_mk_ok_unit() }
}

fn error_result(message: &str) -> *mut lean_object {
    unsafe { formal_web_lean_io_result_mk_error_from_bytes(message.as_ptr() as *const c_char, message.len()) }
}

fn call_lean_runtime_message_handler(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { formal_web_handle_runtime_message(lean_message) };

    let is_ok = unsafe { formal_web_lean_io_result_is_ok(io_result) } != 0;
    if !is_ok {
        unsafe { formal_web_lean_io_result_show_error(io_result) };
    }

    unsafe { formal_web_lean_dec(io_result) };
}

fn startup_runtime_message() -> Result<String, String> {
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let artifact_path: PathBuf = current_dir.join(STARTUP_ARTIFACT_RELATIVE_PATH);
    let artifact_path = artifact_path
        .canonicalize()
        .map_err(|error| format!("failed to resolve startup artifact path: {error}"))?;
    Ok(format!(
        "FreshTopLevelTraversable|file://{}",
        artifact_path.display()
    ))
}

fn user_event_of_runtime_message(message: &str) -> Result<FormalWebUserEvent, String> {
    match message {
        NEW_TOP_LEVEL_TRAVERSABLE_MESSAGE => Ok(FormalWebUserEvent::NewTopLevelTraversable),
        _ => Err(format!("unknown runtime message: {message}")),
    }
}

fn user_agent_note_rendering_opportunity(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { formal_web_user_agent_note_rendering_opportunity(lean_message) };

    let is_ok = unsafe { formal_web_lean_io_result_is_ok(io_result) } != 0;
    if !is_ok {
        unsafe { formal_web_lean_io_result_show_error(io_result) };
    }

    unsafe { formal_web_lean_dec(io_result) };
}

fn with_event_loop_proxy<R>(f: impl FnOnce(&Option<EventLoopProxy<FormalWebUserEvent>>) -> R) -> R {
    let guard = EVENT_LOOP_PROXY
        .lock()
        .expect("event loop proxy mutex poisoned");
    f(&guard)
}

fn queue_paint(pointer: usize) -> Result<(), String> {
    with_event_loop_proxy(|proxy| match proxy {
        Some(proxy) => proxy
            .send_event(FormalWebUserEvent::Paint(pointer))
            .map_err(|error| format!("failed to queue paint event: {error}")),
        None => Err(String::from("winit event loop proxy is not initialized")),
    })
}

struct FormalWebApp {
    window: Option<Arc<Window>>,
    renderer: VelloWindowRenderer,
    current_base_document: Option<usize>,
    pending_base_document: Option<usize>,
    saw_redraw_requested: bool,
    has_top_level_traversable: bool,
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
            current_base_document: None,
            pending_base_document: None,
            saw_redraw_requested: false,
            has_top_level_traversable: false,
            animation_timer: None,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
        }
    }
}

impl FormalWebApp {
    fn update_window_viewport_snapshot(window: &Window) {
        let viewport = viewport_for_window(window);
        let mut snapshot = WINDOW_VIEWPORT_SNAPSHOT
            .lock()
            .expect("window viewport snapshot mutex poisoned");
        *snapshot = Some((
            viewport.window_size.0,
            viewport.window_size.1,
            viewport.hidpi_scale,
            viewport.color_scheme,
        ));
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

    fn paint_base_document(&mut self, pointer: usize) {
        if pointer == 0 {
            return;
        }

        let animation_time = self.current_animation_time();
        let Some(window) = self.window.as_ref() else {
            return;
        };

        let base_document = unsafe { &mut *(pointer as *mut BaseDocument) };
        base_document.set_viewport(viewport_for_window(window));
        for pass in 0..3 {
            base_document.resolve(animation_time);
            let _ = pass;
            if !base_document.has_pending_critical_resources() {
                break;
            }
        }

        let (width, height) = base_document.viewport().window_size;
        let scale = base_document.viewport().scale_f64();

        if self.renderer.is_active() {
            self.renderer.set_size(width, height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, width, height);
        }

        self.renderer.render(|scene| {
            paint_scene(scene, &*base_document, scale, width, height, 0, 0)
        });
    }

    fn with_current_base_document<R>(&self, f: impl FnOnce(&BaseDocument) -> R) -> Option<R> {
        let pointer = self.current_base_document?;
        let base_document = unsafe { &*(pointer as *const BaseDocument) };
        Some(f(base_document))
    }

    fn with_current_base_document_mut<R>(&mut self, f: impl FnOnce(&mut BaseDocument) -> R) -> Option<R> {
        let pointer = self.current_base_document?;
        let base_document = unsafe { &mut *(pointer as *mut BaseDocument) };
        Some(f(base_document))
    }

    fn pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        if let Some(coords) = self.with_current_base_document(|base_document| {
            let scale = base_document.viewport().scale_f64();
            let LogicalPosition::<f32> {
                x: screen_x,
                y: screen_y,
            } = position.to_logical(scale);
            let viewport_scroll = base_document.viewport_scroll();
            let client_x = screen_x;
            let client_y = screen_y;
            let page_x = client_x + viewport_scroll.x as f32;
            let page_y = client_y + viewport_scroll.y as f32;
            PointerCoords {
                screen_x,
                screen_y,
                client_x,
                client_y,
                page_x,
                page_y,
            }
        }) {
            coords
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
        dispatch_event_runtime_message(event);
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
                if let Some(pointer) = self.pending_base_document.take() {
                    self.paint_base_document(pointer);
                    self.saw_redraw_requested = false;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = self.window.as_ref() {
                    Self::update_window_viewport_snapshot(window);
                }
                let _ = self.with_current_base_document_mut(|base_document| {
                    let viewport = base_document.viewport().clone();
                    base_document.set_viewport(Viewport::new(
                        size.width,
                        size.height,
                        viewport.hidpi_scale,
                        viewport.color_scheme,
                    ));
                });
                if self.renderer.is_active() {
                    self.renderer.set_size(size.width, size.height);
                }
                if self.has_top_level_traversable {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                        user_agent_note_rendering_opportunity("request_redraw");
                    }
                }
            }
            WindowEvent::CloseRequested => {
                self.renderer.suspend();
                self.animation_timer = None;
                self.current_base_document = None;
                self.has_top_level_traversable = false;
                if let Ok(mut snapshot) = WINDOW_VIEWPORT_SNAPSHOT.lock() {
                    *snapshot = None;
                }
                self.window = None;
                event_loop.exit();
            }
            WindowEvent::Destroyed => {
                self.renderer.suspend();
                self.animation_timer = None;
                self.current_base_document = None;
                self.has_top_level_traversable = false;
                if let Ok(mut snapshot) = WINDOW_VIEWPORT_SNAPSHOT.lock() {
                    *snapshot = None;
                }
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
            FormalWebUserEvent::Paint(pointer) => {
                let Some(_window) = self.window.as_ref() else {
                    return;
                };

                self.current_base_document = Some(pointer);

                if self.saw_redraw_requested {
                    self.paint_base_document(pointer);
                    self.saw_redraw_requested = false;
                } else {
                    self.pending_base_document = Some(pointer);
                }
            }
            FormalWebUserEvent::DocumentRequestRedraw => {
                if self.has_top_level_traversable {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                        user_agent_note_rendering_opportunity("request_redraw");
                    }
                }
            }
            FormalWebUserEvent::NewTopLevelTraversable => {
                self.has_top_level_traversable = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                    user_agent_note_rendering_opportunity("request_redraw");
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_empty_html_document(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(EMPTY_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_loaded_html_document(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(LOADED_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_render_html_document(pointer: usize) -> *mut lean_object {
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
pub extern "C" fn formal_web_extract_base_document(pointer: usize) -> usize {
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
pub extern "C" fn formal_web_queue_paint(pointer: usize, _: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| queue_paint(pointer))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic queueing paint event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_apply_ui_event(
    pointer: usize,
    event: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            return Err(String::from("cannot apply UI event to null BaseDocument pointer"));
        }

        let c_event = unsafe { formal_web_lean_string_cstr(event) };
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
pub extern "C" fn formal_web_send_runtime_message(message: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_message = unsafe { formal_web_lean_string_cstr(message) };
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
pub extern "C" fn formal_web_run_winit_event_loop(_: *mut lean_object) -> *mut lean_object {
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