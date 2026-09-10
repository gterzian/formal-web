//! Wire types for the safe passing of structured data
//! (<https://html.spec.whatwg.org/#safe-passing-of-structured-data>), the
//! spec section that defines the structured serialization algorithms.
//!
//! [`SerializedRecord`] is the IPC-safe serialized representation of a
//! JavaScript value produced by StructuredSerializeInternal
//! (<https://html.spec.whatwg.org/#structuredserializeinternal>), and
//! [`TransferDataHolder`] is the per-transferable data holder produced by
//! StructuredSerializeWithTransfer
//! (<https://html.spec.whatwg.org/#structuredserializewithtransfer>).  Both
//! are pure data so they can cross the content-process / user-agent IPC
//! boundary in both directions (the source content process serializes, the
//! target content process deserializes).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::content::{CanvasId, MessageId, PortId};

/// A primitive JavaScript value in a portable, serializable form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrimitiveValue {
    /// The `undefined` value.
    Undefined,
    /// The `null` value.
    Null,
    /// A boolean.
    Boolean(bool),
    /// A 64-bit floating point number.
    Number(f64),
    /// A string, stored as UTF-16 code units to preserve unpaired surrogates.
    String(Vec<u16>),
    /// A BigInt, represented as its decimal string.
    BigInt(String),
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
///
/// A serialized representation of a JavaScript value (corresponds to a Record
/// in the spec). All fields are plain Rust types so the enum can cross IPC
/// boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializedRecord {
    /// { [[Type]]: "primitive", [[Value]]: value }
    Primitive(PrimitiveValue),
    /// { [[Type]]: "Boolean", [[BooleanData]]: bool }
    Boolean(bool),
    /// { [[Type]]: "Number", [[NumberData]]: f64 }
    Number(f64),
    /// { [[Type]]: "BigInt", [[BigIntData]]: string }
    BigInt(String),
    /// { [[Type]]: "String", [[StringData]]: string (as UTF-16 code units) }
    String(Vec<u16>),
    /// { [[Type]]: "Date", [[DateValue]]: f64 }
    Date(f64),
    /// { [[Type]]: "RegExp", [[OriginalSource]], [[OriginalFlags]] }
    RegExp { source: String, flags: String },
    /// { [[Type]]: "SharedArrayBuffer" }  — raw bytes + metadata
    SharedArrayBuffer {
        data: Vec<u8>,
        agent_cluster: String,
    },
    /// { [[Type]]: "ArrayBuffer", [[ArrayBufferData]]: dataCopy, [[ArrayBufferByteLength]]: size }
    ArrayBuffer {
        data: Vec<u8>,
        byte_length: u64,
        max_byte_length: Option<u64>,
    },
    /// { [[Type]]: "ArrayBufferView" }
    /// When [[Constructor]] is "DataView":
    ///   { [[ArrayBufferSerialized]]: bufferSerialized, [[ByteLength]]: byteLength, [[ByteOffset]]: byteOffset }
    /// Otherwise (typed array):
    ///   { [[Constructor]]: constructor, [[ArrayBufferSerialized]]: bufferSerialized,
    ///     [[ByteLength]]: byteLength, [[ByteOffset]]: byteOffset, [[ArrayLength]]: arrayLength }
    ArrayBufferView {
        constructor: String,
        buffer_serialized: Box<SerializedRecord>,
        byte_length: u64,
        byte_offset: u64,
        array_length: Option<u64>,
    },
    /// { [[Type]]: "Map" }
    Map(Vec<(SerializedRecord, SerializedRecord)>),
    /// { [[Type]]: "Set" }
    Set(Vec<SerializedRecord>),
    /// { [[Type]]: "Error" }
    Error {
        name: String,
        message: Option<String>,
        stack: String,
        cause: Option<Box<SerializedRecord>>,
    },
    /// { [[Type]]: "Array" }
    Array {
        length: u64,
        properties: Vec<(Vec<u16>, SerializedRecord)>,
    },
    /// Platform object implementing [Serializable].
    PlatformObject {
        interface_name: String,
        fields: HashMap<String, SerializedRecord>,
    },
    /// { [[Type]]: "Object" }
    Object(Vec<(Vec<u16>, SerializedRecord)>),
    /// A back-reference to a record that was serialized earlier in the
    /// graph, by its memory-map index: a cyclic or shared (DAG) object is
    /// serialized once and every other occurrence becomes this reference.
    /// On deserialization the reference resolves to the object already
    /// built for that record (the port message queue of the structured
    /// clone memory map keys by this index).
    /// <https://html.spec.whatwg.org/#structuredserializeinternal> step 2
    BackReference(usize),
    /// A reference to the transfer-list entry at the given (0-based) index.
    ///
    /// StructuredSerializeWithTransfer step 2.4 places this record in the
    /// memory map in place of a transferable, so every place the transferable
    /// appears in the message serializes as this reference; the corresponding
    /// [`TransferDataHolder`] carries the actual data.  On deserialization
    /// StructuredDeserializeWithTransfer resolves the index against the
    /// values it built from the transfer data holders.
    ///
    /// Note: The spec models this as an "uninitialized" dataHolder record that
    /// step 5 fills with the transferable's data and whose identity is shared
    /// between the serialized graph and the transfer data holder list.  Record
    /// identity cannot cross an IPC boundary, so the implementation uses the
    /// transfer-list index instead; the observable behavior is identical.
    TransferredValue(usize),
}

/// A data holder for a transferred value (pure data, IPC-safe).
///
/// Corresponds to a dataHolder Record appended by StructuredSerializeWithTransfer
/// step 5.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferDataHolder {
    /// { [[Type]]: "ArrayBuffer", [[ArrayBufferData]]: dataCopy,
    ///   [[ArrayBufferByteLength]]: byteLength }
    /// or { [[Type]]: "ResizableArrayBuffer", [[ArrayBufferData]]: dataCopy,
    ///   [[ArrayBufferByteLength]]: byteLength, [[ArrayBufferMaxByteLength]]: maxByteLength }
    ArrayBuffer {
        data: Vec<u8>,
        byte_length: u64,
        max_byte_length: Option<u64>,
    },
    /// { [[Type]]: "MessagePort", [[PortMessageQueue]]: queue,
    ///   [[RemotePort]]: remotePort }
    /// A transferred MessagePort (see [`PortTransferData`]): the port id, its
    /// pending message queue (the tasks that are to fire message events,
    /// moved with the transfer per the transfer steps), and the id of the
    /// port it was entangled with, if any (re-entangled on
    /// transfer-receiving).
    /// <https://html.spec.whatwg.org/#message-ports:transfer-steps>
    MessagePort(PortTransferData),
    /// { [[Type]]: "OffscreenCanvas", [[Width]], [[Height]],
    ///   [[PlaceholderCanvas]] }
    /// A transferred OffscreenCanvas (see [`OffscreenCanvasTransferData`]):
    /// the canvas id linking it to its placeholder canvas element's embed
    /// site, and the bitmap dimensions.  The placeholder reference is the
    /// canvas id (a weak reference to the DOM element is not serializable).
    OffscreenCanvas(OffscreenCanvasTransferData),
}

/// The data holder of the OffscreenCanvas transfer steps (see
/// <https://html.spec.whatwg.org/multipage/canvas.html#offscreencanvas>),
/// carried as pure IPC-safe data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffscreenCanvasTransferData {
    /// The canvas id of the placeholder canvas element (dataHolder's
    /// placeholder canvas, realized as the shared CanvasId).
    pub canvas_id: CanvasId,
    /// dataHolder.[[Width]].
    pub width: u32,
    /// dataHolder.[[Height]].
    pub height: u32,
}

/// The data holder of the MessagePort transfer steps (the dataHolder of
/// <https://html.spec.whatwg.org/#message-ports:transfer-steps>), carried
/// as pure IPC-safe data so a transfer can cross processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortTransferData {
    /// The id of the transferred port.
    pub port_id: PortId,
    /// The port's pending message queue, moved with the transfer
    /// (dataHolder.[[PortMessageQueue]]).
    pub queue: Vec<PortMessagePayload>,
    /// The port the transferred port was entangled with, if any
    /// (dataHolder.[[RemotePort]]).
    pub remote_port: Option<PortId>,
    /// Routed messages still in flight toward the port (messages posted
    /// while the port was in transit or completing, not yet delivered to
    /// its queue).  Moves with the transfer so a direct delivery in the
    /// receiving process cannot jump ahead of them (the port message
    /// queue is FIFO).
    pub in_flight: u32,
}

/// One queued message on a port message queue (pure data, IPC-safe).
/// Carries the serialized message produced by the message port post message
/// steps plus a unique message id for the MessagePortExtraFG TLA trace.
/// <https://html.spec.whatwg.org/#message-port-post-message-steps>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMessagePayload {
    /// Unique id of this message, used by the TLA trace to identify the
    /// message in `MessagePortExtraFG.tla` (the `MessageId` of MessagePortExtraFG.tla).
    pub message_id: MessageId,
    /// `serializeWithTransferResult.[[Serialized]]` (step 5).
    pub serialized: SerializedRecord,
    /// `serializeWithTransferResult.[[TransferDataHolders]]` (step 5).
    pub transfer_data_holders: Vec<TransferDataHolder>,
}

/// Payload of the window post message steps carried between the source
/// content process (steps 1–7), the user agent (step 8), and the target
/// content process (the substeps of step 8).
///
/// <https://html.spec.whatwg.org/#window-post-message-steps>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostMessageRequest {
    /// <https://html.spec.whatwg.org/#navigable> holding the targetWindow.
    pub target_navigable_id: crate::content::NavigableId,
    /// `options["targetOrigin"]` after the window post message steps steps
    /// 3–5 (a serialized origin, or the single character "*").
    pub target_origin: String,
    /// <https://html.spec.whatwg.org/#navigable> holding the source window
    /// (the WindowProxy value of step 8.3).
    pub source_navigable_id: crate::content::NavigableId,
    /// <https://html.spec.whatwg.org/#concept-settings-object-origin> of the
    /// incumbent settings object, serialized (step 8.2).
    pub source_origin: String,
    /// `serializeWithTransferResult.[[Serialized]]` (step 7).
    pub serialized: SerializedRecord,
    /// `serializeWithTransferResult.[[TransferDataHolders]]` (step 7).
    pub transfer_data_holders: Vec<TransferDataHolder>,
}
