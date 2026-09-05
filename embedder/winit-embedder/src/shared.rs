//! Helpers shared by the winit embedder's windowed and headless apps:
//! clipboard access, screenshot encoding, startup URL resolution, URL
//! normalization, the current window viewport snapshot, and Apple-standard
//! text-editing keybindings.

use crate::events::{FormalWebUserEvent, send_user_event};
use std::sync::{LazyLock, Mutex, mpsc};
use std::time::Duration;
use webview::{BlitzKeyEvent, ColorScheme};

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";

/// How long the caller thread waits for the embedder event loop to answer a
/// clipboard read/write request.  The request is handed to the event loop
/// (which owns the platform clipboard) over a user event and the reply is
/// awaited synchronously; the bound keeps a busy or unresponsive event loop
/// from hanging the calling thread.
const CLIPBOARD_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

pub fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("failed to access clipboard: {error}"))?;
    clipboard
        .get_text()
        .map_err(|error| format!("failed to read clipboard text: {error}"))
}

pub fn write_clipboard_text(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("failed to access clipboard: {error}"))?;
    clipboard
        .set_text(text)
        .map_err(|error| format!("failed to write clipboard text: {error}"))
}

pub fn clipboard_get_text() -> Result<String, String> {
    let (reply, receiver) = mpsc::channel();
    send_user_event(FormalWebUserEvent::ClipboardRead { reply })?;
    receiver
        .recv_timeout(CLIPBOARD_REPLY_TIMEOUT)
        .map_err(|error| {
            format!(
                "timed out after {} ms waiting for clipboard text: {error}",
                CLIPBOARD_REPLY_TIMEOUT.as_millis()
            )
        })?
}

pub fn clipboard_set_text(text: String) -> Result<(), String> {
    let (reply, receiver) = mpsc::channel();
    send_user_event(FormalWebUserEvent::ClipboardWrite { text, reply })?;
    receiver
        .recv_timeout(CLIPBOARD_REPLY_TIMEOUT)
        .map_err(|error| {
            format!(
                "timed out after {} ms waiting to write clipboard text: {error}",
                CLIPBOARD_REPLY_TIMEOUT.as_millis()
            )
        })?
}

type ViewportSnapshot = Option<(u32, u32, f32, ColorScheme)>;
static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<ViewportSnapshot>> =
    LazyLock::new(|| Mutex::new(None));

pub fn update_window_viewport_snapshot(snapshot: Option<(u32, u32, f32, ColorScheme)>) {
    *WINDOW_VIEWPORT_SNAPSHOT.lock().expect("poisoned") = snapshot;
}

pub fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    *WINDOW_VIEWPORT_SNAPSHOT.lock().expect("poisoned")
}

/// A white placeholder screenshot. The headless app has no composited
/// pixels to capture, so automation screenshots in headless mode are a
/// white canvas at the current viewport size.
pub fn automation_screenshot_png() -> Result<Vec<u8>, String> {
    let Some((width, height, _, _)) = window_viewport_snapshot() else {
        return Err(String::from("content viewport is not initialized"));
    };
    if width == 0 || height == 0 {
        return Err(String::from("content viewport is zero-sized"));
    }
    let rgba = vec![255u8; (width as usize) * (height as usize) * 4];
    encode_png_rgba(&rgba, width, height)
}

pub fn encode_png_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
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

pub fn startup_destination_url(startup_url: Option<&str>) -> Result<String, String> {
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

pub fn normalize_browser_destination(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") || trimmed.starts_with("about:") {
        return Some(trimmed.to_owned());
    }
    Some(format!("https://{trimmed}"))
}

/// Apple-standard text-editing keybindings (⌘←, ⌥⌫, ^A, …), used by the
/// winit windowed backend for the chrome's text input.
pub fn apple_standard_keybinding_for_key_down(event: &BlitzKeyEvent) -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        use keyboard_types::{Key, Modifiers as KeyboardModifiers};

        if !event.state.is_pressed() {
            return None;
        }

        let command_mod = event.modifiers.contains(KeyboardModifiers::SUPER);
        let control_mod = event.modifiers.contains(KeyboardModifiers::CONTROL);
        let option_mod = event.modifiers.contains(KeyboardModifiers::ALT);
        let shift_mod = event.modifiers.contains(KeyboardModifiers::SHIFT);

        if command_mod {
            match &event.key {
                Key::Backspace => return Some("deleteToBeginningOfLine:"),
                Key::Delete => return Some("deleteToEndOfLine:"),
                Key::ArrowLeft if shift_mod => {
                    return Some("moveToBeginningOfLineAndModifySelection:");
                }
                Key::ArrowLeft => return Some("moveToBeginningOfLine:"),
                Key::ArrowRight if shift_mod => return Some("moveToEndOfLineAndModifySelection:"),
                Key::ArrowRight => return Some("moveToEndOfLine:"),
                Key::ArrowUp if shift_mod => {
                    return Some("moveToBeginningOfDocumentAndModifySelection:");
                }
                Key::ArrowUp => return Some("moveToBeginningOfDocument:"),
                Key::ArrowDown if shift_mod => {
                    return Some("moveToEndOfDocumentAndModifySelection:");
                }
                Key::ArrowDown => return Some("moveToEndOfDocument:"),
                _ => {}
            }
        }

        if option_mod {
            match &event.key {
                Key::Backspace => return Some("deleteWordBackward:"),
                Key::Delete => return Some("deleteWordForward:"),
                Key::ArrowLeft if shift_mod => return Some("moveWordLeftAndModifySelection:"),
                Key::ArrowLeft => return Some("moveWordLeft:"),
                Key::ArrowRight if shift_mod => return Some("moveWordRightAndModifySelection:"),
                Key::ArrowRight => return Some("moveWordRight:"),
                _ => {}
            }
        }

        if control_mod && let Key::Character(value) = &event.key {
            return match value.to_lowercase().as_str() {
                "a" if shift_mod => Some("moveToBeginningOfParagraphAndModifySelection:"),
                "a" => Some("moveToBeginningOfParagraph:"),
                "b" if shift_mod => Some("moveBackwardAndModifySelection:"),
                "b" => Some("moveBackward:"),
                "d" => Some("deleteForward:"),
                "e" if shift_mod => Some("moveToEndOfParagraphAndModifySelection:"),
                "e" => Some("moveToEndOfParagraph:"),
                "f" if shift_mod => Some("moveForwardAndModifySelection:"),
                "f" => Some("moveForward:"),
                "h" => Some("deleteBackward:"),
                "k" => Some("deleteToEndOfParagraph:"),
                "n" if shift_mod => Some("moveDownAndModifySelection:"),
                "n" => Some("moveDown:"),
                "o" => Some("insertNewlineIgnoringFieldEditor:"),
                "p" if shift_mod => Some("moveUpAndModifySelection:"),
                "p" => Some("moveUp:"),
                _ => None,
            };
        }

        match &event.key {
            Key::Backspace => Some("deleteBackward:"),
            _ => None,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = event;
        None
    }
}
