//! NSEvent → input event mapping for the AppKit backend.

use crate::platform::read_clipboard_text;
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use log::error;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSInteger;
use webview::{
    BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, KeyState, MouseEventButton,
    MouseEventButtons, PointerCoords, PointerDetails, SmolStr,
};

/// Map a macOS virtual key code (NSEvent.keyCode) to a W3C keyboard code.
pub(super) fn code_from_keycode(keycode: u16) -> Code {
    match keycode {
        0x00 => Code::KeyA,
        0x01 => Code::KeyS,
        0x02 => Code::KeyD,
        0x03 => Code::KeyF,
        0x04 => Code::KeyH,
        0x05 => Code::KeyG,
        0x06 => Code::KeyZ,
        0x07 => Code::KeyX,
        0x08 => Code::KeyC,
        0x09 => Code::KeyV,
        0x0B => Code::KeyB,
        0x0C => Code::KeyQ,
        0x0D => Code::KeyW,
        0x0E => Code::KeyE,
        0x0F => Code::KeyR,
        0x10 => Code::KeyY,
        0x11 => Code::KeyT,
        0x12 => Code::Digit1,
        0x13 => Code::Digit2,
        0x14 => Code::Digit3,
        0x15 => Code::Digit4,
        0x16 => Code::Digit6,
        0x17 => Code::Digit5,
        0x18 => Code::Equal,
        0x19 => Code::Digit9,
        0x1A => Code::Digit7,
        0x1B => Code::Minus,
        0x1C => Code::Digit8,
        0x1D => Code::Digit0,
        0x1E => Code::BracketRight,
        0x1F => Code::KeyO,
        0x20 => Code::KeyU,
        0x21 => Code::BracketLeft,
        0x22 => Code::KeyI,
        0x23 => Code::KeyP,
        0x24 => Code::Enter,
        0x25 => Code::KeyL,
        0x26 => Code::KeyJ,
        0x27 => Code::Quote,
        0x28 => Code::KeyK,
        0x29 => Code::Semicolon,
        0x2A => Code::Backslash,
        0x2B => Code::Comma,
        0x2C => Code::Slash,
        0x2D => Code::KeyN,
        0x2E => Code::KeyM,
        0x2F => Code::Period,
        0x30 => Code::Tab,
        0x31 => Code::Space,
        0x33 => Code::Backspace,
        0x35 => Code::Escape,
        0x36 => Code::Super,
        0x37 => Code::ShiftLeft,
        0x39 => Code::AltLeft,
        0x3A => Code::ControlLeft,
        0x3B => Code::ShiftRight,
        0x3C => Code::AltRight,
        0x3D => Code::ControlRight,
        0x41 => Code::NumpadDecimal,
        0x43 => Code::NumpadMultiply,
        0x45 => Code::NumpadAdd,
        0x47 => Code::NumpadClear,
        0x48 => Code::NumpadDivide,
        0x49 => Code::NumpadEnter,
        0x4A => Code::NumpadSubtract,
        0x4F => Code::F5,
        0x50 => Code::F6,
        0x51 => Code::F7,
        0x52 => Code::F3,
        0x53 => Code::F8,
        0x54 => Code::F9,
        0x55 => Code::F11,
        0x56 => Code::F13,
        0x57 => Code::F16,
        0x58 => Code::F14,
        0x59 => Code::F10,
        0x5A => Code::F12,
        0x5B => Code::F15,
        0x5D => Code::Home,
        0x5E => Code::PageUp,
        0x5F => Code::Delete,
        0x60 => Code::F4,
        0x61 => Code::End,
        0x62 => Code::F2,
        0x63 => Code::PageDown,
        0x64 => Code::F1,
        0x65 => Code::ArrowLeft,
        0x66 => Code::ArrowRight,
        0x67 => Code::ArrowDown,
        0x68 => Code::ArrowUp,
        _ => Code::Unidentified,
    }
}

/// Map an NSEvent to a logical keyboard key, using the event's characters
/// (with shift applied) for printable keys and named keys for the rest.
pub(super) fn key_from_event(event: &NSEvent) -> Key {
    let code = code_from_keycode(event.keyCode());
    match code {
        Code::Enter => Key::Enter,
        Code::Tab => Key::Tab,
        Code::Backspace => Key::Backspace,
        Code::Delete => Key::Delete,
        Code::Escape => Key::Escape,
        Code::ArrowLeft => Key::ArrowLeft,
        Code::ArrowRight => Key::ArrowRight,
        Code::ArrowUp => Key::ArrowUp,
        Code::ArrowDown => Key::ArrowDown,
        Code::Home => Key::Home,
        Code::End => Key::End,
        Code::PageUp => Key::PageUp,
        Code::PageDown => Key::PageDown,
        Code::Super => Key::Super,
        Code::ShiftLeft | Code::ShiftRight => Key::Shift,
        Code::AltLeft | Code::AltRight => Key::Alt,
        Code::ControlLeft | Code::ControlRight => Key::Control,
        Code::CapsLock => Key::CapsLock,
        _ => {
            let characters = event
                .charactersIgnoringModifiers()
                .and_then(|text| text.to_string().chars().next())
                .unwrap_or_default();
            Key::Character(characters.to_string())
        }
    }
}

/// Map NSEvent modifier flags to keyboard-types modifiers.
pub(super) fn modifiers_from_flags(flags: NSEventModifierFlags) -> KeyboardModifiers {
    let mut modifiers = KeyboardModifiers::default();
    if flags.contains(NSEventModifierFlags::Shift) {
        modifiers.insert(KeyboardModifiers::SHIFT);
    }
    if flags.contains(NSEventModifierFlags::Control) {
        modifiers.insert(KeyboardModifiers::CONTROL);
    }
    if flags.contains(NSEventModifierFlags::Option) {
        modifiers.insert(KeyboardModifiers::ALT);
    }
    if flags.contains(NSEventModifierFlags::Command) {
        modifiers.insert(KeyboardModifiers::SUPER);
    }
    modifiers
}

/// Build a key event from an NSEvent.
pub(super) fn ns_event_to_key_event(event: &NSEvent) -> BlitzKeyEvent {
    let pressed = event.r#type() == NSEventType::KeyDown;
    let characters = if pressed {
        event.characters().map(|text| {
            let value: SmolStr = text.to_string().into();
            value
        })
    } else {
        None
    };
    BlitzKeyEvent {
        key: key_from_event(event),
        code: code_from_keycode(event.keyCode()),
        modifiers: modifiers_from_flags(event.modifierFlags()),
        location: Location::Standard,
        is_auto_repeating: event.isARepeat(),
        is_composing: false,
        state: if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        },
        text: characters,
    }
}

/// The text to attach when `event` is a paste shortcut (⌘V), read here on the
/// main thread that owns the system clipboard. Content never reads the system
/// clipboard itself, so a paste must carry its text with the event.
pub(super) fn paste_shortcut_clipboard_text(event: &BlitzKeyEvent) -> Option<String> {
    if !event.state.is_pressed() {
        return None;
    }
    if !event.modifiers.contains(KeyboardModifiers::SUPER) {
        return None;
    }
    match &event.key {
        Key::Character(character) if character.eq_ignore_ascii_case("v") => {
            match read_clipboard_text() {
                Ok(text) => Some(text),
                Err(error) => {
                    error!("failed to prefetch clipboard text for paste: {error}");
                    None
                }
            }
        }
        _ => None,
    }
}

/// Map an NSEvent mouse button number to a mouse button.
pub(super) fn button_from_number(button_number: NSInteger) -> MouseEventButton {
    match button_number {
        1 => MouseEventButton::Secondary,
        2 => MouseEventButton::Auxiliary,
        3 => MouseEventButton::Fourth,
        4 => MouseEventButton::Fifth,
        _ => MouseEventButton::Main,
    }
}

/// Build pointer coordinates for the web content from a point measured in
/// the web area's top-left coordinate space (points).
pub(super) fn content_coords(x: f64, y_from_top: f64) -> PointerCoords {
    PointerCoords {
        screen_x: x as f32,
        screen_y: y_from_top as f32,
        client_x: x as f32,
        client_y: y_from_top as f32,
        page_x: x as f32,
        page_y: y_from_top as f32,
    }
}

/// A pointer event template for the given state; used by mouse and touch
/// event synthesis.
pub(super) fn pointer_event(
    id: BlitzPointerId,
    is_primary: bool,
    coords: PointerCoords,
    button: MouseEventButton,
    buttons: MouseEventButtons,
    modifiers: KeyboardModifiers,
) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id,
        is_primary,
        coords,
        button,
        buttons,
        mods: modifiers,
        details: PointerDetails::default(),
    }
}

/// A wheel event from an NSEvent scroll: precise deltas become pixel deltas,
/// line deltas become line deltas.
pub(super) fn wheel_delta_from_event(event: &NSEvent) -> BlitzWheelDelta {
    if event.hasPreciseScrollingDeltas() {
        BlitzWheelDelta::Pixels(event.scrollingDeltaX(), event.scrollingDeltaY())
    } else {
        BlitzWheelDelta::Lines(event.deltaX(), event.deltaY())
    }
}
