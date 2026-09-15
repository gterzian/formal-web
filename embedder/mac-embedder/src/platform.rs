//! Platform helpers for the AppKit embedder: clipboard access, screenshot
//! encoding, startup URL resolution, URL normalization, and the current
//! window viewport snapshot.

use crate::events::{FormalWebUserEvent, UserEventSink};
use log::error;
use std::sync::{LazyLock, Mutex};
use webview::ColorScheme;

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";

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

/// Hands a clipboard write to the app's main-thread run loop, which owns the
/// system clipboard. Fire-and-forget: the caller does not wait for the write.
pub fn clipboard_set_text(sink: &dyn UserEventSink, text: String) {
    if let Err(error) = sink.send(FormalWebUserEvent::ClipboardWrite { text }) {
        error!("failed to send clipboard write event: {error}");
    }
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
