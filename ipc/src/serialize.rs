//! Serialization traits for IPC message types.
//!
//! On both backends, messages are serialized with postcard and carried
//! either through typed channels (ipc-channel) or as XPC dictionary data
//! (native XPC). This means `IpcSerialize` and `IpcDeserialize` are
//! the same as serde's `Serialize` and `DeserializeOwned` on both backends.

/// Trait for types that can be serialized for IPC transport.
pub use serde::Serialize as IpcSerialize;

/// Trait for types that can be deserialized from IPC transport.
pub use serde::de::DeserializeOwned as IpcDeserialize;
