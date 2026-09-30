//! WebSockets Standard (<https://websockets.spec.whatwg.org/>): the WebSocket
//! and CloseEvent interfaces. The connection itself (the opening handshake,
//! framing, the closing handshake) runs in the net process; content sends it
//! `ipc_messages::websocket::WebSocketRequest`s and receives the feedback
//! from the WebSocket connection as `Command::WebSocket`, which becomes a
//! `Task::WebSocket` here, so the steps the spec queues as tasks run on this
//! event loop.

pub(crate) mod close_event;
pub(crate) mod websocket;

pub(crate) use close_event::{CloseEvent, CloseEventInit};
pub(crate) use websocket::{BinaryType, WebSocket, WebSocketSendData};
