//! WebSocket connections for content processes: one thread per connection
//! over tungstenite, driven by the content process's requests. The
//! connection's events go straight to the requesting content process as
//! `Command::WebSocket`.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use ipc_messages::content::{Command as ContentCommand, DocumentId};
use ipc_messages::websocket::{WebSocketData, WebSocketEvent, WebSocketId, WebSocketRequest};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error, Message};

/// What a connection's thread does for content after the handshake.
enum Outgoing {
    Send(WebSocketData),
    Close { code: Option<u16>, reason: String },
}

/// The open connections, by socket.
#[derive(Default)]
pub(crate) struct WebSocketConnections {
    connections: HashMap<WebSocketId, Sender<Outgoing>>,
}

impl WebSocketConnections {
    pub(crate) fn handle(&mut self, request: WebSocketRequest) {
        match request {
            WebSocketRequest::Connect {
                document_id,
                socket,
                url,
                protocols,
                reply_to,
            } => {
                let (sender, receiver) = crossbeam_channel::unbounded();
                self.connections.insert(socket, sender);
                let spawned = thread::Builder::new()
                    .name(format!("websocket-{}", socket.0))
                    .spawn(move || {
                        run_connection(document_id, socket, url, protocols, reply_to, receiver)
                    });
                if let Err(error) = spawned {
                    log::error!("[net] failed to start the WebSocket thread: {error}");
                    self.connections.remove(&socket);
                }
            }
            WebSocketRequest::Send { socket, data } => {
                self.forward(socket, Outgoing::Send(data));
            }
            WebSocketRequest::Close {
                socket,
                code,
                reason,
            } => {
                self.forward(socket, Outgoing::Close { code, reason });
            }
        }
    }

    fn forward(&mut self, socket: WebSocketId, outgoing: Outgoing) {
        let Some(sender) = self.connections.get(&socket) else {
            log::debug!("[net] WebSocket {:?} is gone, dropping the request", socket);
            return;
        };
        if sender.send(outgoing).is_err() {
            // The connection's thread ended: its last event told content.
            self.connections.remove(&socket);
        }
    }
}

/// <https://websockets.spec.whatwg.org/#feedback-from-the-protocol>
fn run_connection(
    document_id: DocumentId,
    socket: WebSocketId,
    url: String,
    protocols: Vec<String>,
    reply_to: ipc::IpcSender<ContentCommand>,
    receiver: Receiver<Outgoing>,
) {
    let report = |event: WebSocketEvent| {
        if let Err(error) = reply_to.send(ContentCommand::WebSocket {
            document_id,
            socket,
            event,
        }) {
            log::debug!("[net] content for WebSocket {:?} is gone: {error}", socket);
        }
    };

    // The opening handshake: the request carries the subprotocols content
    // asked for.
    let mut request = match url.as_str().into_client_request() {
        Ok(request) => request,
        Err(error) => {
            log::debug!("[net] WebSocket {url}: invalid request: {error}");
            report(WebSocketEvent::Failed);
            return;
        }
    };
    if !protocols.is_empty() {
        match HeaderValue::from_str(&protocols.join(", ")) {
            Ok(value) => {
                request
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", value);
            }
            Err(error) => {
                log::debug!("[net] WebSocket {url}: invalid protocols: {error}");
                report(WebSocketEvent::Failed);
                return;
            }
        }
    }
    let (mut connection, response) = match tungstenite::connect(request) {
        Ok(connected) => connected,
        Err(error) => {
            log::debug!("[net] WebSocket {url}: connection failed: {error}");
            report(WebSocketEvent::Failed);
            return;
        }
    };
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let protocol = header("Sec-WebSocket-Protocol");
    let extensions = header("Sec-WebSocket-Extensions");
    if let Some(protocol) = protocols.first().filter(|_| protocol.is_empty()) {
        // A server that ignores the subprotocol request: the connection fails.
        // <https://fetch.spec.whatwg.org/#websocket-opening-handshake>
        log::debug!("[net] WebSocket {url}: the server did not select a protocol ({protocol})");
        report(WebSocketEvent::Failed);
        return;
    }

    // The read loop polls the socket, so content's requests interleave with
    // the peer's messages on this one thread.
    let stream = match connection.get_ref() {
        MaybeTlsStream::Plain(stream) => Some(stream),
        MaybeTlsStream::Rustls(stream) => Some(stream.get_ref()),
        _ => None,
    };
    if let Some(stream) = stream
        && let Err(error) = stream.set_read_timeout(Some(Duration::from_millis(20)))
    {
        log::error!("[net] WebSocket {url}: failed to set the read timeout: {error}");
    }
    report(WebSocketEvent::Established {
        protocol,
        extensions,
    });

    let mut close_frame: Option<(u16, String)> = None;
    let mut closing = false;
    loop {
        loop {
            match receiver.try_recv() {
                Ok(Outgoing::Send(data)) => {
                    let (message, bytes) = match data {
                        WebSocketData::Text(text) => {
                            let bytes = text.len() as u64;
                            (Message::Text(text), bytes)
                        }
                        WebSocketData::Binary(binary) => {
                            let bytes = binary.len() as u64;
                            (Message::Binary(binary), bytes)
                        }
                    };
                    match connection.send(message) {
                        Ok(()) => report(WebSocketEvent::Transmitted { bytes }),
                        Err(error) => log::debug!("[net] WebSocket {url}: send failed: {error}"),
                    }
                }
                Ok(Outgoing::Close { code, reason }) => {
                    if !closing {
                        closing = true;
                        let frame = code.map(|code| CloseFrame {
                            code: CloseCode::from(code),
                            reason: reason.into(),
                        });
                        if let Err(error) = connection.close(frame) {
                            log::debug!("[net] WebSocket {url}: close failed: {error}");
                        }
                        report(WebSocketEvent::ClosingHandshakeStarted);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Content dropped the socket: close the connection.
                    if !closing {
                        closing = true;
                        if let Err(error) = connection.close(None) {
                            log::debug!("[net] WebSocket {url}: close failed: {error}");
                        }
                    }
                    break;
                }
            }
        }

        match connection.read() {
            Ok(Message::Text(text)) => report(WebSocketEvent::Message(WebSocketData::Text(text))),
            Ok(Message::Binary(binary)) => {
                report(WebSocketEvent::Message(WebSocketData::Binary(binary)))
            }
            Ok(Message::Close(frame)) => {
                close_frame = frame.map(|frame| (u16::from(frame.code), frame.reason.into_owned()));
                if !closing {
                    closing = true;
                    report(WebSocketEvent::ClosingHandshakeStarted);
                }
            }
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Err(Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                if let Err(error) = connection.flush() {
                    log::debug!("[net] WebSocket {url}: flush failed: {error}");
                }
            }
            Err(Error::ConnectionClosed) => {
                let (code, reason) = close_frame.unwrap_or((1005, String::new()));
                report(WebSocketEvent::Closed {
                    was_clean: true,
                    code,
                    reason,
                });
                return;
            }
            Err(Error::AlreadyClosed) => return,
            Err(error) => {
                log::debug!("[net] WebSocket {url}: connection error: {error}");
                report(WebSocketEvent::Closed {
                    was_clean: false,
                    code: 1006,
                    reason: String::new(),
                });
                return;
            }
        }
    }
}
