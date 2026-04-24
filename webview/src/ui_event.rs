use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords, PointerDetails,
    UiEvent,
};
use keyboard_types::{Code, Key, Location, Modifiers as KeyboardModifiers};
use serde::{Deserialize, Serialize};

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
    AppleStandardKeybinding(String),
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
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
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
            UiEvent::AppleStandardKeybinding(data) => {
                Self::AppleStandardKeybinding(data.to_string())
            }
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
            SerializableUiEvent::AppleStandardKeybinding(data) => {
                Ok(Self::AppleStandardKeybinding(data.into()))
            }
        }
    }
}

pub fn serialize_ui_event(event: &UiEvent) -> Result<String, String> {
    serde_json::to_string(&SerializableUiEvent::from(event))
        .map_err(|error| format!("failed to serialize UI event: {error}"))
}

pub fn deserialize_ui_event(message: &str) -> Result<UiEvent, String> {
    let event: SerializableUiEvent = serde_json::from_str(message)
        .map_err(|error| format!("failed to deserialize UI event: {error}"))?;
    event.try_into()
}
