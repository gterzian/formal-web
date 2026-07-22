//! Extension manifests for formal-web helper processes.
//!
//! Defines `ExtensionManifest` implementations for net, media, and content,
//! wrapping the existing process-spawning logic.

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Command as ProcessCommand;

use ipc::{BootstrapToken, ExtensionEndpoint, ExtensionManifest, IpcError};

use crate::sidecar_executable_path;

// ── Net extension manifest ──────────────────────────────────────────────────

pub struct NetExtensionManifest;

impl ExtensionManifest for NetExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint {
        ExtensionEndpoint::Singleton {
            service_name: "formal-web.net",
        }
    }

    fn spawn(&self, token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        let executable_path = sidecar_executable_path("formal-web-net")
            .map_err(|error| IpcError::Transport(error))?;

        let mut child_process = ProcessCommand::new(&executable_path);
        #[cfg(unix)]
        child_process.arg0("formal-web-net");
        child_process.arg("--net-token").arg(&token.to_string());

        child_process
            .spawn()
            .map_err(|error| IpcError::Transport(format!("failed to start net process: {error}")))
    }
}

// ── Graphics extension manifest ─────────────────────────────────────────────

pub struct GraphicsExtensionManifest;

impl ExtensionManifest for GraphicsExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint {
        ExtensionEndpoint::Singleton {
            service_name: "formal-web.graphics",
        }
    }

    fn spawn(&self, token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        let executable_path = sidecar_executable_path("formal-web-graphics")
            .map_err(|error| IpcError::Transport(error))?;

        let mut child_process = ProcessCommand::new(&executable_path);
        #[cfg(unix)]
        child_process.arg0("formal-web-graphics");
        child_process
            .arg("--graphics-token")
            .arg(&token.to_string());

        child_process.spawn().map_err(|error| {
            IpcError::Transport(format!("failed to start graphics process: {error}"))
        })
    }
}

// ── Media extension manifest ────────────────────────────────────────────────

pub struct MediaExtensionManifest;

impl ExtensionManifest for MediaExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint {
        ExtensionEndpoint::Singleton {
            service_name: "formal-web.media",
        }
    }

    fn spawn(&self, token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        let executable_path = sidecar_executable_path("formal-web-media")
            .map_err(|error| IpcError::Transport(error))?;

        let mut child_process = ProcessCommand::new(&executable_path);
        #[cfg(unix)]
        child_process.arg0("formal-web-media");
        child_process.arg("--media-token").arg(&token.to_string());

        child_process
            .spawn()
            .map_err(|error| IpcError::Transport(format!("failed to start media process: {error}")))
    }
}

// ── Content extension manifest ──────────────────────────────────────────────

/// Manifest for one content process instance.
pub struct ContentExtensionManifest {
    pub process_label: String,
}

impl ContentExtensionManifest {
    pub fn new(process_label: String) -> Self {
        Self { process_label }
    }
}

impl ExtensionManifest for ContentExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint {
        ExtensionEndpoint::MultiInstance {
            service_name: "com.formal-web.app.content",
        }
    }

    fn spawn(&self, token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        let executable_path = sidecar_executable_path("formal-web-content")
            .map_err(|error| IpcError::Transport(error))?;

        let sanitized_label = self
            .process_label
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.') {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();

        let mut child_process = ProcessCommand::new(&executable_path);
        #[cfg(unix)]
        child_process.arg0(format!("formal-web-content:{sanitized_label}"));
        child_process.arg("--content-token").arg(&token.to_string());
        child_process
            .arg("--content-label")
            .arg(&self.process_label);

        child_process.spawn().map_err(|error| {
            IpcError::Transport(format!("failed to start content process: {error}"))
        })
    }
}
