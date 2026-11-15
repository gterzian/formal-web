#![cfg(target_os = "macos")]
//! The AppKit windowed embedder: the default macOS embedder. Runs an
//! NSApplication with NSWindow/NSView/CALayer display, presents composited
//! web content zero-copy by setting the content layer's `contents` to the
//! shared IOSurface from the graphics process, and paces animated content
//! with a CADisplayLink that requests the next frame via
//! `WebviewProvider::frame_needed`.
//!
//! The crate is self-contained: it shares nothing with the winit embedder
//! except the `webview` crate API. It has no winit, Blitz, or GPU
//! dependencies.
//!
//! Automation (WebDriver, CDP) never runs on this embedder: the winit
//! embedder is the single automation port (see `embedder/src/lib.rs`).

mod app;
mod events;
mod input;
mod platform;
mod window;

pub use app::run_windowed_app;
