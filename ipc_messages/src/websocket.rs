//! Messages between content processes and the net process for WebSocket
//! connections (<https://websockets.spec.whatwg.org/>).
//!
//! Content sends `WebSocketRequest`s inside `network::Request::WebSocket`;
//! the net process answers on the content process's own command channel with
//! `Command::WebSocket`, whose events are the feedback from the WebSocket
//! connection that the WebSocket object's algorithms queue tasks for.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::content::{Command as ContentCommand, DocumentId};

/// Identifies one WebSocket across the content and net processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WebSocketId(pub Uuid);

impl WebSocketId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WebSocketId {
    fn default() -> Self {
        Self::new()
    }
}

/// A WebSocket message's data: text or binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketData {
    Text(String),
    Binary(Vec<u8>),
}

/// Content to the net process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketRequest {
    /// The WebSocket constructor's step 12: establish a WebSocket connection
    /// given urlRecord, protocols, and client.
    Connect {
        document_id: DocumentId,
        socket: WebSocketId,
        url: String,
        protocols: Vec<String>,
        /// The content process's command sender, where the connection's
        /// events go.
        reply_to: ipc::IpcSender<ContentCommand>,
    },
    /// send(data): send a WebSocket Message comprised of data.
    Send {
        socket: WebSocketId,
        data: WebSocketData,
    },
    /// close(code, reason): start the WebSocket closing handshake, or fail
    /// the connection when it is not yet established.
    Close {
        socket: WebSocketId,
        code: Option<u16>,
        reason: String,
    },
}

/// The net process to content, inside `Command::WebSocket`: the feedback from
/// the WebSocket connection.
/// <https://websockets.spec.whatwg.org/#feedback-from-the-protocol>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketEvent {
    /// The WebSocket connection is established.
    Established {
        protocol: String,
        extensions: String,
    },
    /// A WebSocket message has been received.
    Message(WebSocketData),
    /// Bytes of application data queued by send() have been transmitted to
    /// the network.
    Transmitted { bytes: u64 },
    /// The WebSocket closing handshake is started.
    ClosingHandshakeStarted,
    /// The WebSocket connection is closed, possibly cleanly.
    Closed {
        was_clean: bool,
        code: u16,
        reason: String,
    },
    /// The user agent was required to fail the WebSocket connection: the
    /// connection was never established.
    Failed,
}
