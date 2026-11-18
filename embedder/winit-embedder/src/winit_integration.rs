use ::winit::event::{ElementState, Ime, KeyEvent as WinitKeyEvent};
use ::winit::keyboard::{
    Key as WinitKey, KeyCode as WinitKeyCode, KeyLocation as WinitKeyLocation,
    ModifiersState as WinitModifiersState, NamedKey, PhysicalKey,
};
use ::winit::window::Window;
use blitz_traits::events::{BlitzImeEvent, BlitzKeyEvent, KeyState, PointerDetails};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use cursor_icon::CursorIcon;
use keyboard_types::NamedKey as KbtNamedKey;
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use log::error;
use std::sync::Arc;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::{Cursor, Window as WinitWindow};

fn theme_to_color_scheme(theme: ::winit::window::Theme) -> ColorScheme {
    match theme {
        ::winit::window::Theme::Light => ColorScheme::Light,
        ::winit::window::Theme::Dark => ColorScheme::Dark,
    }
}

pub fn viewport_snapshot_for_window(window: &Window) -> (u32, u32, f32, ColorScheme) {
    let size = window.inner_size();
    let scale = window.scale_factor() as f32;
    let color_scheme =
        theme_to_color_scheme(window.theme().unwrap_or(::winit::window::Theme::Light));
    (size.width, size.height, scale, color_scheme)
}

pub fn viewport_of_snapshot(snapshot: (u32, u32, f32, ColorScheme)) -> Viewport {
    let (width, height, scale, color_scheme) = snapshot;
    Viewport::new(width, height, scale, color_scheme)
}

pub fn winit_ime_to_blitz(event: Ime) -> BlitzImeEvent {
    match event {
        Ime::Enabled => BlitzImeEvent::Enabled,
        Ime::Disabled => BlitzImeEvent::Disabled,
        Ime::Preedit(text, cursor) => BlitzImeEvent::Preedit(text, cursor),
        Ime::Commit(text) => BlitzImeEvent::Commit(text),
    }
}

pub fn touch_pointer_details(force: Option<::winit::event::Force>) -> PointerDetails {
    PointerDetails {
        pressure: force.map(|value| value.normalized()).unwrap_or(0.0),
        ..PointerDetails::default()
    }
}

pub fn winit_modifiers_to_kbt_modifiers(winit_modifiers: WinitModifiersState) -> KeyboardModifiers {
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
        modifiers.insert(KeyboardModifiers::META);
    }
    modifiers
}

pub fn winit_key_location_to_kbt_location(location: WinitKeyLocation) -> Location {
    match location {
        WinitKeyLocation::Standard => Location::Standard,
        WinitKeyLocation::Left => Location::Left,
        WinitKeyLocation::Right => Location::Right,
        WinitKeyLocation::Numpad => Location::Numpad,
    }
}

pub fn winit_key_to_kbt_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Character(value) => Key::Character(value.to_string()),
        WinitKey::Named(named) => match named {
            NamedKey::Alt => Key::Named(KbtNamedKey::Alt),
            NamedKey::Backspace => Key::Named(KbtNamedKey::Backspace),
            NamedKey::Control => Key::Named(KbtNamedKey::Control),
            NamedKey::Delete => Key::Named(KbtNamedKey::Delete),
            NamedKey::ArrowDown => Key::Named(KbtNamedKey::ArrowDown),
            NamedKey::End => Key::Named(KbtNamedKey::End),
            NamedKey::Enter => Key::Named(KbtNamedKey::Enter),
            NamedKey::Escape => Key::Named(KbtNamedKey::Escape),
            NamedKey::Home => Key::Named(KbtNamedKey::Home),
            NamedKey::ArrowLeft => Key::Named(KbtNamedKey::ArrowLeft),
            NamedKey::Meta => Key::Named(KbtNamedKey::Meta),
            NamedKey::PageDown => Key::Named(KbtNamedKey::PageDown),
            NamedKey::PageUp => Key::Named(KbtNamedKey::PageUp),
            NamedKey::ArrowRight => Key::Named(KbtNamedKey::ArrowRight),
            NamedKey::Shift => Key::Named(KbtNamedKey::Shift),
            NamedKey::Space => Key::Character(" ".to_owned()),
            NamedKey::Tab => Key::Named(KbtNamedKey::Tab),
            NamedKey::ArrowUp => Key::Named(KbtNamedKey::ArrowUp),
            _ => Key::Named(KbtNamedKey::Unidentified),
        },
        _ => Key::Character(String::new()),
    }
}

pub fn winit_physical_key_to_kbt_code(physical_key: &PhysicalKey) -> Code {
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
            WinitKeyCode::SuperLeft => Code::MetaLeft,
            WinitKeyCode::SuperRight => Code::MetaRight,
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

pub fn winit_key_event_to_blitz(event: &WinitKeyEvent, mods: WinitModifiersState) -> BlitzKeyEvent {
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

// ── Shell provider for the chrome document ────────────────────────────────

/// The clipboard text a paste request read, waiting for the chrome document's
/// key event dispatch to return so the paste can be delivered to it.
pub type PendingClipboardText = Arc<std::sync::Mutex<Option<String>>>;

pub struct WinitShellProvider {
    window: Arc<WinitWindow>,
    pending_clipboard_text: PendingClipboardText,
}

impl WinitShellProvider {
    pub fn new(window: Arc<WinitWindow>, pending_clipboard_text: PendingClipboardText) -> Self {
        Self {
            window,
            pending_clipboard_text,
        }
    }
}

fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("clipboard read error: {e}"))
}

fn clipboard_write(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| format!("clipboard write error: {e}"))
}

impl ShellProvider for WinitShellProvider {
    fn request_redraw(&self) {
        self.window.request_redraw();
    }
    fn set_cursor(&self, icon: Option<CursorIcon>) {
        self.window
            .set_cursor(icon.map_or(Cursor::default(), Cursor::Icon));
    }
    fn set_window_title(&self, title: String) {
        self.window.set_title(&title);
    }
    fn set_ime_enabled(&self, enabled: bool) {
        self.window.set_ime_allowed(enabled);
    }
    fn set_ime_cursor_area(&self, x: f32, y: f32, w: f32, h: f32) {
        self.window
            .set_ime_cursor_area(LogicalPosition::new(x, y), LogicalSize::new(w, h));
    }
    fn request_clipboard_text(&self) -> Result<(), blitz_traits::shell::ClipboardError> {
        // The chrome document gets the text as a `UiEvent::Paste` once its key
        // event dispatch returns (see `ChromeUi::handle_ui_event`).
        match clipboard_read() {
            Ok(text) => {
                *self.pending_clipboard_text.lock().unwrap() = Some(text);
                Ok(())
            }
            Err(error) => {
                error!("clipboard read error: {error}");
                Err(blitz_traits::shell::ClipboardError)
            }
        }
    }
    fn set_clipboard_text(&self, text: String) -> Result<(), blitz_traits::shell::ClipboardError> {
        clipboard_write(text).map_err(|_| blitz_traits::shell::ClipboardError)
    }
}
