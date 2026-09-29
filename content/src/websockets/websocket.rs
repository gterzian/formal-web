use std::cell::RefCell;
use std::rc::Rc;

use ipc_messages::network::Request as NetworkRequest;
use ipc_messages::websocket::{WebSocketData, WebSocketEvent, WebSocketId, WebSocketRequest};
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use log::error;
use url::Url;

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::dom::{fire_event, fire_event_using};
use crate::encoding::{utf_8_decode, utf_8_encode};
use crate::file_api::Blob;
use crate::html::{MessageEvent, MessageEventInit};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{
    create_array_buffer, invalid_access_error_value, invalid_state_error_value, syntax_error_value,
};

use super::close_event::{CloseEvent, CloseEventInit};

/// <https://websockets.spec.whatwg.org/#dom-websocket-connecting>
pub(crate) const CONNECTING: u16 = 0;
/// <https://websockets.spec.whatwg.org/#dom-websocket-open>
pub(crate) const OPEN: u16 = 1;
/// <https://websockets.spec.whatwg.org/#dom-websocket-closing>
pub(crate) const CLOSING: u16 = 2;
/// <https://websockets.spec.whatwg.org/#dom-websocket-closed>
pub(crate) const CLOSED: u16 = 3;

/// <https://websockets.spec.whatwg.org/#dom-websocket-readystate>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadyState {
    Connecting,
    Open,
    Closing,
    Closed,
}

impl ReadyState {
    pub(crate) fn as_idl(self) -> u16 {
        match self {
            Self::Connecting => CONNECTING,
            Self::Open => OPEN,
            Self::Closing => CLOSING,
            Self::Closed => CLOSED,
        }
    }
}

/// <https://websockets.spec.whatwg.org/#enumdef-binarytype>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryType {
    Blob,
    ArrayBuffer,
}

impl BinaryType {
    /// <https://webidl.spec.whatwg.org/#js-enumeration>
    pub(crate) fn from_idl(value: &str) -> Option<Self> {
        match value {
            "blob" => Some(Self::Blob),
            "arraybuffer" => Some(Self::ArrayBuffer),
            _ => None,
        }
    }

    pub(crate) fn as_idl(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::ArrayBuffer => "arraybuffer",
        }
    }
}

/// The `data` argument of send(): (BufferSource or Blob or USVString).
pub(crate) enum WebSocketSendData {
    String(String),
    Blob(Blob),
    ArrayBuffer(Vec<u8>),
    ArrayBufferView(Vec<u8>),
}

/// The WebSocket object's state, shared by its clones.
pub(crate) struct WebSocketSlots {
    /// <https://websockets.spec.whatwg.org/#concept-websocket-url>
    url: Url,
    /// <https://websockets.spec.whatwg.org/#concept-websocket-ready-state>
    ready_state: ReadyState,
    /// <https://websockets.spec.whatwg.org/#dom-websocket-bufferedamount>
    buffered_amount: u64,
    /// <https://websockets.spec.whatwg.org/#dom-websocket-extensions>
    extensions: String,
    /// <https://websockets.spec.whatwg.org/#dom-websocket-protocol>
    protocol: String,
    /// <https://websockets.spec.whatwg.org/#dom-websocket-binarytype>
    binary_type: BinaryType,
    /// Set once the user agent was required to fail the WebSocket
    /// connection, for the error event the closed steps fire.
    failed: bool,
}

/// <https://websockets.spec.whatwg.org/#websocket>
#[gc_struct]
pub(crate) struct WebSocket {
    pub(crate) event_target: EventTarget,

    #[ignore_trace]
    pub(crate) id: WebSocketId,

    #[ignore_trace]
    slots: Rc<RefCell<WebSocketSlots>>,
}

impl EventTargetAccess for WebSocket {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

/// <https://datatracker.ietf.org/doc/html/rfc6455#section-4.1>
fn is_valid_subprotocol(protocol: &str) -> bool {
    // A |Sec-WebSocket-Protocol| value is a token: one or more characters
    // from the range U+0021 to U+007E that are not separators.
    const SEPARATORS: &str = "()<>@,;:\\\"/[]?={} \t";
    !protocol.is_empty()
        && protocol.chars().all(|character| {
            ('\u{21}'..='\u{7E}').contains(&character) && !SEPARATORS.contains(character)
        })
}

impl WebSocket {
    /// <https://websockets.spec.whatwg.org/#dom-websocket-websocket>
    pub(crate) fn constructor(
        url: String,
        protocols: Vec<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let baseURL be this's relevant settings object's API base
        // URL.
        // Note: The API base URL is the creation URL of the realm's global
        // scope.
        let base_url = with_global_scope(ec, |global_scope, _ec| Ok(global_scope.creation_url()))?;

        // Step 2: Let urlRecord be the result of applying the URL parser to
        // url with baseURL.
        let url_record = Url::options().base_url(base_url.as_ref()).parse(&url);

        // Step 3: If urlRecord is failure, then throw a "SyntaxError"
        // DOMException.
        let Ok(mut url_record) = url_record else {
            return Err(syntax_error_value(ec));
        };

        // Step 4: If urlRecord's scheme is "http", then set urlRecord's scheme
        // to "ws".
        // Step 5: Otherwise, if urlRecord's scheme is "https", set urlRecord's
        // scheme to "wss".
        let replacement = match url_record.scheme() {
            "http" => Some("ws"),
            "https" => Some("wss"),
            _ => None,
        };
        if let Some(scheme) = replacement
            && url_record.set_scheme(scheme).is_err()
        {
            return Err(syntax_error_value(ec));
        }

        // Step 6: If urlRecord's scheme is not "ws" or "wss", then throw a
        // "SyntaxError" DOMException.
        if !matches!(url_record.scheme(), "ws" | "wss") {
            return Err(syntax_error_value(ec));
        }

        // Step 7: If urlRecord's fragment is non-null, then throw a
        // "SyntaxError" DOMException.
        if url_record.fragment().is_some() {
            return Err(syntax_error_value(ec));
        }

        // Step 8: If protocols is a string, set protocols to a sequence
        // consisting of just that string.
        // Note: The binding layer converts the (DOMString or
        // sequence<DOMString>) argument to the sequence.
        // Step 9: If any of the values in protocols occur more than once or
        // otherwise fail to match the requirements for elements that comprise
        // the value of `Sec-WebSocket-Protocol` fields as defined by The
        // WebSocket Protocol, then throw a "SyntaxError" DOMException.
        for (index, protocol) in protocols.iter().enumerate() {
            if !is_valid_subprotocol(protocol) || protocols[..index].contains(protocol) {
                return Err(syntax_error_value(ec));
            }
        }

        // Step 10: Set this's url to urlRecord.
        let socket = Self {
            event_target: EventTarget::new(ec),
            id: WebSocketId::new(),
            slots: Rc::new(RefCell::new(WebSocketSlots {
                url: url_record.clone(),
                ready_state: ReadyState::Connecting,
                buffered_amount: 0,
                extensions: String::new(),
                protocol: String::new(),
                binary_type: BinaryType::Blob,
                failed: false,
            })),
        };

        // Step 11: Let client be this's relevant settings object.
        // Step 12: Run this step in parallel:
        // Step 12.1: Establish a WebSocket connection given urlRecord,
        // protocols, and client.
        // Note: The net process establishes the connection and runs it; its
        // feedback arrives as tasks for this object, which the realm's global
        // scope routes here.
        let id = socket.id;
        let registered = socket.clone();
        with_global_scope(ec, move |global_scope, ec| {
            global_scope.register_web_socket(registered, ec);
            let (Some(document_id), Some(reply_to), Some(sender)) = (
                global_scope.document_id(),
                global_scope.content_command_sender(),
                global_scope.network_extension_sender(),
            ) else {
                return Err(
                    ec.new_type_error("this realm has no network connection for WebSockets")
                );
            };
            if let Err(error) = sender.send(NetworkRequest::WebSocket(WebSocketRequest::Connect {
                document_id,
                socket: id,
                url: url_record.to_string(),
                protocols,
                reply_to,
            })) {
                error!("[websocket] connect: {error}");
            }
            Ok(())
        })?;
        Ok(socket)
    }

    fn request(&self, request: WebSocketRequest, ec: &mut dyn ExecutionContext<Types>) {
        let sent = with_global_scope(ec, move |global_scope, _ec| {
            if let Some(sender) = global_scope.network_extension_sender()
                && let Err(error) = sender.send(NetworkRequest::WebSocket(request))
            {
                error!("[websocket] request: {error}");
            }
            Ok(())
        });
        if let Err(error) = sent {
            error!("[websocket] request: {}", error.display());
        }
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-url>
    pub(crate) fn url(&self) -> String {
        // The url getter steps are to return this's url, serialized.
        self.slots.borrow().url.to_string()
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-readystate>
    pub(crate) fn ready_state(&self) -> u16 {
        // The readyState getter steps are to return this's ready state.
        self.slots.borrow().ready_state.as_idl()
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-bufferedamount>
    pub(crate) fn buffered_amount(&self) -> u64 {
        // The bufferedAmount getter steps are to return the number of bytes of
        // application data (UTF-8 text and binary data) that have been queued
        // using send() but that, as of the last time the event loop reached
        // step 1, had not yet been transmitted to the network. (This thus
        // includes any text sent during the execution of the current task,
        // regardless of whether the user agent is able to transmit text in the
        // background in parallel with script execution.) This does not include
        // framing overhead incurred by the protocol, or buffering done by the
        // operating system or network hardware.
        self.slots.borrow().buffered_amount
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-extensions>
    pub(crate) fn extensions(&self) -> String {
        // The extensions getter steps are to return this's extensions.
        self.slots.borrow().extensions.clone()
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-protocol>
    pub(crate) fn protocol(&self) -> String {
        // The protocol getter steps are to return this's protocol.
        self.slots.borrow().protocol.clone()
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-binarytype>
    pub(crate) fn binary_type(&self) -> BinaryType {
        // The binaryType getter steps are to return this's binary type.
        self.slots.borrow().binary_type
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-binarytype>
    pub(crate) fn set_binary_type(&self, value: Option<BinaryType>) {
        // The binaryType setter steps are:
        // Step 1: If the given value is either "blob" or "arraybuffer", then
        // set this's binary type to the given value.
        // Step 2: Otherwise, do nothing.
        if let Some(value) = value {
            self.slots.borrow_mut().binary_type = value;
        }
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-send>
    pub(crate) fn send(
        &self,
        data: WebSocketSendData,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If this's ready state is CONNECTING, then throw an
        // "InvalidStateError" DOMException.
        if self.slots.borrow().ready_state == ReadyState::Connecting {
            return Err(invalid_state_error_value(ec));
        }

        // Step 2: Run the appropriate set of steps from the following list:
        let (message, bytes) = match data {
            // If data is a string: If the WebSocket connection is established
            // and the WebSocket closing handshake has not yet started, then
            // the user agent must send a WebSocket Message comprised of the
            // data argument using a text frame opcode; if the data cannot be
            // sent, e.g. because it would need to be buffered but the buffer
            // is full, the user agent must flag the WebSocket as full and then
            // close the WebSocket connection. Any invocation of this method
            // with a string argument that does not throw an exception must
            // increase the bufferedAmount attribute by the number of bytes
            // needed to express the argument as UTF-8.
            WebSocketSendData::String(string) => {
                let bytes = utf_8_encode(&string).len() as u64;
                (WebSocketData::Text(string), bytes)
            }
            // If data is a Blob object: If the WebSocket connection is
            // established, and the WebSocket closing handshake has not yet
            // started, then the user agent must send a WebSocket Message
            // comprised of data using a binary frame opcode; if the data
            // cannot be sent, e.g. because it would need to be buffered but
            // the buffer is full, the user agent must flag the WebSocket as
            // full and then close the WebSocket connection. The data to be
            // sent is the raw data represented by the Blob object. Any
            // invocation of this method with a Blob argument that does not
            // throw an exception must increase the bufferedAmount attribute by
            // the size of the Blob object's raw data, in bytes.
            WebSocketSendData::Blob(blob) => {
                let bytes = blob.bytes().to_vec();
                let length = bytes.len() as u64;
                (WebSocketData::Binary(bytes), length)
            }
            // If data is an ArrayBuffer: ... The data to be sent is the data
            // stored in the buffer described by the ArrayBuffer object. Any
            // invocation of this method with an ArrayBuffer argument that does
            // not throw an exception must increase the bufferedAmount
            // attribute by the length of the ArrayBuffer in bytes.
            // If data is an ArrayBufferView: ... The data to be sent is the
            // data stored in the section of the buffer described by the
            // ArrayBuffer object that the ArrayBufferView object references.
            // Any invocation of this method with this kind of argument that
            // does not throw an exception must increase the bufferedAmount
            // attribute by the length of the ArrayBufferView in bytes.
            WebSocketSendData::ArrayBuffer(bytes) | WebSocketSendData::ArrayBufferView(bytes) => {
                let length = bytes.len() as u64;
                (WebSocketData::Binary(bytes), length)
            }
        };
        let established = self.slots.borrow().ready_state == ReadyState::Open;
        self.slots.borrow_mut().buffered_amount += bytes;
        if established {
            self.request(
                WebSocketRequest::Send {
                    socket: self.id,
                    data: message,
                },
                ec,
            );
        }
        Ok(())
    }

    /// <https://websockets.spec.whatwg.org/#dom-websocket-close>
    pub(crate) fn close(
        &self,
        code: Option<u16>,
        reason: Option<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If code is the special value "missing", then set code to
        // null.
        // Step 2: If reason is the special value "missing", then set reason to
        // the empty string.
        let reason = reason.unwrap_or_default();

        // Step 3: If code is present, but is neither an integer equal to 1000
        // nor an integer in the range 3000 to 4999, inclusive, throw an
        // "InvalidAccessError" DOMException.
        if let Some(code) = code
            && code != 1000
            && !(3000..=4999).contains(&code)
        {
            return Err(invalid_access_error_value(
                String::from("The close code must be either 1000, or between 3000 and 4999"),
                ec,
            ));
        }

        // Step 4: If reason is present, then run these substeps:
        // Step 4.1: Let reasonBytes be the result of UTF-8 encoding reason.
        // Step 4.2: If reasonBytes is longer than 123 bytes, then throw a
        // "SyntaxError" DOMException.
        if utf_8_encode(&reason).len() > 123 {
            return Err(syntax_error_value(ec));
        }

        // Step 5: Run the first matching steps from the following list:
        let ready_state = self.slots.borrow().ready_state;
        match ready_state {
            // If this's ready state is CLOSING (2) or CLOSED (3): Do nothing.
            ReadyState::Closing | ReadyState::Closed => {}
            // If the WebSocket connection is not yet established: Fail the
            // WebSocket connection and set this's ready state to CLOSING (2).
            ReadyState::Connecting => {
                self.slots.borrow_mut().ready_state = ReadyState::Closing;
                self.slots.borrow_mut().failed = true;
                self.request(
                    WebSocketRequest::Close {
                        socket: self.id,
                        code,
                        reason,
                    },
                    ec,
                );
            }
            // If the WebSocket closing handshake has not yet been started:
            // Start the WebSocket closing handshake and set this's ready state
            // to CLOSING (2). If neither code nor reason is present, the
            // WebSocket Close message must not have a body. The WebSocket
            // Protocol erroneously states that the status code is required for
            // the start the WebSocket closing handshake algorithm. If code is
            // present, then the status code to use in the WebSocket Close
            // message must be the integer given by code. If reason is also
            // present, then reasonBytes must be provided in the Close message
            // after the status code.
            // Otherwise: Set this's ready state to CLOSING (2).
            ReadyState::Open => {
                self.slots.borrow_mut().ready_state = ReadyState::Closing;
                self.request(
                    WebSocketRequest::Close {
                        socket: self.id,
                        code,
                        reason,
                    },
                    ec,
                );
            }
        }
        Ok(())
    }

    /// <https://websockets.spec.whatwg.org/#feedback-from-the-protocol>
    pub(crate) fn feedback(
        &self,
        event: WebSocketEvent,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        match event {
            WebSocketEvent::Established {
                protocol,
                extensions,
            } => self.connection_established(protocol, extensions, time_millis, ec),
            WebSocketEvent::Message(data) => self.message_received(data, time_millis, ec),
            WebSocketEvent::Transmitted { bytes } => {
                let mut slots = self.slots.borrow_mut();
                slots.buffered_amount = slots.buffered_amount.saturating_sub(bytes);
                Ok(())
            }
            WebSocketEvent::ClosingHandshakeStarted => {
                // When the WebSocket closing handshake is started, the user
                // agent must queue a task to change the ready state to CLOSING
                // (2).
                self.slots.borrow_mut().ready_state = ReadyState::Closing;
                Ok(())
            }
            WebSocketEvent::Closed {
                was_clean,
                code,
                reason,
            } => self.connection_closed(was_clean, code, reason, time_millis, ec),
            WebSocketEvent::Failed => {
                self.slots.borrow_mut().failed = true;
                self.connection_closed(false, 1006, String::new(), time_millis, ec)
            }
        }
    }

    /// <https://websockets.spec.whatwg.org/#feedback-from-the-protocol>
    fn connection_established(
        &self,
        protocol: String,
        extensions: String,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // When the WebSocket connection is established, the user agent must
        // queue a task to run these steps:
        {
            let mut slots = self.slots.borrow_mut();
            // Step 1: Change the ready state to OPEN (1).
            slots.ready_state = ReadyState::Open;

            // Step 2: Change the extensions attribute's value to the
            // extensions in use, if it is not the null value.
            slots.extensions = extensions;

            // Step 3: Change the protocol attribute's value to the subprotocol
            // in use, if it is not the null value.
            slots.protocol = protocol;
        }

        // Step 4: Fire an event named open at the WebSocket object.
        fire_event(ec, self, "open", time_millis, false).map(|_| ())
    }

    /// <https://websockets.spec.whatwg.org/#feedback-from-the-protocol>
    fn message_received(
        &self,
        data: WebSocketData,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // When a WebSocket message has been received with type type and data
        // data, the user agent must queue a task to follow these steps:
        // Step 1: If ready state is not OPEN (1), then return.
        let (ready_state, binary_type, origin) = {
            let slots = self.slots.borrow();
            (
                slots.ready_state,
                slots.binary_type,
                slots.url.origin().ascii_serialization(),
            )
        };
        if ready_state != ReadyState::Open {
            return Ok(());
        }

        // Step 2: Let dataForEvent be determined by switching on type and
        // binary type:
        let data_for_event = match (data, binary_type) {
            // type indicates that the data is Text: a new DOMString containing
            // data
            (WebSocketData::Text(text), _) => {
                let string = ec.js_string_from_str(&text);
                ec.value_from_string(string)
            }
            // type indicates that the data is Binary and binary type is
            // "blob": a new Blob object, created in the relevant Realm of the
            // WebSocket object, that represents data as its raw data
            (WebSocketData::Binary(bytes), BinaryType::Blob) => {
                let blob = Blob::from_bytes(bytes);
                let object = create_interface_instance::<Types, Blob>(blob, ec)?;
                Types::value_from_object(object)
            }
            // type indicates that the data is Binary and binary type is
            // "arraybuffer": a new ArrayBuffer object, created in the relevant
            // Realm of the WebSocket object, whose contents are data
            (WebSocketData::Binary(bytes), BinaryType::ArrayBuffer) => {
                Types::value_from_object(create_array_buffer(&bytes, ec)?)
            }
        };

        // Step 3: Fire an event named message at the WebSocket object, using
        // MessageEvent, with the origin attribute initialized to the
        // serialization of the WebSocket object's url's origin, and the data
        // attribute initialized to dataForEvent.
        let event = MessageEvent::new(
            String::from("message"),
            MessageEventInit {
                bubbles: false,
                cancelable: false,
                composed: false,
                data: data_for_event,
                origin,
                last_event_id: String::new(),
                source: None,
                ports: Vec::new(),
            },
            ec,
        );
        fire_event_using(&self.event_target, event, time_millis, ec).map(|_| ())
    }

    /// <https://websockets.spec.whatwg.org/#feedback-from-the-protocol>
    fn connection_closed(
        &self,
        was_clean: bool,
        code: u16,
        reason: String,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // When the WebSocket connection is closed, possibly cleanly, the user
        // agent must queue a task to run the following substeps:
        // Step 1: Change the ready state to CLOSED (3).
        let failed = {
            let mut slots = self.slots.borrow_mut();
            slots.ready_state = ReadyState::Closed;
            slots.failed
        };

        // Step 2: If the user agent was required to fail the WebSocket
        // connection, or if the WebSocket connection was closed after being
        // flagged as full, fire an event named error at the WebSocket object.
        if failed || !was_clean {
            fire_event(ec, self, "error", time_millis, false)?;
        }

        // Step 3: Fire an event named close at the WebSocket object, using
        // CloseEvent, with the wasClean attribute initialized to true if the
        // connection closed cleanly and false otherwise, the code attribute
        // initialized to the WebSocket connection close code, and the reason
        // attribute initialized to the result of applying UTF-8 decode without
        // BOM to the WebSocket connection close reason.
        let event = CloseEvent::new(
            String::from("close"),
            CloseEventInit {
                bubbles: false,
                cancelable: false,
                composed: false,
                was_clean,
                code,
                reason: utf_8_decode(reason.as_bytes()),
            },
            ec,
        );
        fire_event_using(&self.event_target, event, time_millis, ec)?;

        // The realm no longer routes tasks to the socket.
        let id = self.id;
        if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
            global_scope.unregister_web_socket(id, ec);
            Ok(())
        }) {
            error!("[websocket] unregister: {}", error.display());
        }
        Ok(())
    }

    /// <https://websockets.spec.whatwg.org/#make-disappear>
    pub(crate) fn make_disappear(&self, ec: &mut dyn ExecutionContext<Types>) {
        // When a user agent is to make disappear a WebSocket object (this
        // happens when a Document object goes away), the user agent must
        // follow the first appropriate set of steps from the following list:
        let ready_state = self.slots.borrow().ready_state;
        match ready_state {
            // If the WebSocket connection is not yet established: Fail the
            // WebSocket connection.
            ReadyState::Connecting => self.request(
                WebSocketRequest::Close {
                    socket: self.id,
                    code: None,
                    reason: String::new(),
                },
                ec,
            ),
            // If the WebSocket closing handshake has not yet been started:
            // Start the WebSocket closing handshake, with the status code to
            // use in the WebSocket Close message being 1001.
            ReadyState::Open => self.request(
                WebSocketRequest::Close {
                    socket: self.id,
                    code: Some(1001),
                    reason: String::new(),
                },
                ec,
            ),
            // Otherwise: Do nothing.
            ReadyState::Closing | ReadyState::Closed => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_subprotocol;

    #[test]
    fn subprotocols_are_tokens() {
        assert!(is_valid_subprotocol("echo"));
        assert!(is_valid_subprotocol("chat.v2"));
        assert!(!is_valid_subprotocol(""));
        assert!(!is_valid_subprotocol("with space"));
        assert!(!is_valid_subprotocol("a,b"));
        assert!(!is_valid_subprotocol("caf\u{e9}"));
    }
}
