# content/src/websockets

The net process owns the connection (`net/src/websocket.rs`, one thread per
socket over tungstenite); this module owns the WebSocket object and runs the
feedback from the protocol as `Task::WebSocket` tasks.

## Remaining work

- `WebSocket` is registered in Window realms only; the spec exposes it in
  workers too, which needs the feedback routed to the worker agent's event
  loop.
- `bufferedAmount` decreases when the net process has handed the message to
  the socket, not "as of the last time the event loop reached step 1".
