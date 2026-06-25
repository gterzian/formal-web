//! Serialization traits for IPC message types.
//!
//! On both backends, messages are serialized with postcard.
//! `IpcSerialize` and `IpcDeserialize` are re-exports of serde's
//! `Serialize` and `DeserializeOwned` respectively.

/// Trait for types that can be serialized for IPC transport.
pub use serde::Serialize as IpcSerialize;

/// Trait for types that can be deserialized from IPC transport.
pub use serde::de::DeserializeOwned as IpcDeserialize;
