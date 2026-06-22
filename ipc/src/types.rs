//! Public types for the IPC abstraction layer.
//!
//! These types are backend-agnostic. Both backends use serde + postcard
//! for serialization.

use crossbeam_channel::Receiver;
use serde::{Serialize, de::DeserializeOwned};

use crate::IpcError;

/// An opaque token representing a bootstrap server address.
#[derive(Debug, Clone)]
pub struct BootstrapToken {
    pub(crate) inner: String,
}

impl std::fmt::Display for BootstrapToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(formatter)
    }
}

/// Identifies one content process instance among many.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentInstanceId {
    pub top_level_origin: String,
    pub webview_id: u64,
    pub event_loop_id: u64,
}

/// Describes the IPC topology for an extension process.
pub enum ExtensionEndpoint {
    Singleton { service_name: &'static str },
    MultiInstance { service_name: &'static str },
}

/// Manifest that describes how to start and connect to an extension process.
pub trait ExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint;

    /// Spawn the extension process, passing the bootstrap token via argv.
    /// On the native backend, this is a no-op (launchd manages lifecycle).
    fn spawn(&self, _token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        Err(IpcError::Transport(
            "spawn not available on this backend; launchd manages lifecycle".into(),
        ))
    }
}

// ── IpcSender ───────────────────────────────────────────────────────────────

/// A sender for sending messages to an extension process.
///
/// On the ipc-channel backend, this wraps an `IpcChannelSender<T>`.
/// On the native backend, it wraps an `XpcConnection` and serializes
/// messages as postcard bytes in XPC dictionaries.
#[cfg(feature = "ipc-channel-backend")]
#[derive(Clone)]
pub struct IpcSender<T: Serialize + DeserializeOwned>(pub(crate) ipc_channel::ipc::IpcSender<T>);

#[cfg(feature = "ipc-channel-backend")]
impl<T: Serialize + DeserializeOwned> IpcSender<T> {
    pub fn send(&self, message: T) -> Result<(), IpcError> {
        self.0
            .send(message)
            .map_err(|error| IpcError::Transport(error.to_string()))
    }

    pub fn send_with_shmem(&self, message: T, _shmem: IpcSharedRegion) -> Result<(), IpcError> {
        self.send(message)
    }
}

#[cfg(not(feature = "ipc-channel-backend"))]
#[derive(Clone)]
pub struct IpcSender<T: Serialize + DeserializeOwned> {
    pub(crate) connection: xpc_sys::XpcConnection,
    pub(crate) _marker: std::marker::PhantomData<T>,
}

#[cfg(not(feature = "ipc-channel-backend"))]
impl<T: Serialize + DeserializeOwned> IpcSender<T> {
    pub fn send(&self, message: T) -> Result<(), IpcError> {
        let payload = postcard::to_allocvec(&message)
            .map_err(|error| IpcError::Serialize(error.to_string()))?;
        let mut dict = xpc_sys::XpcDictionary::new();
        dict.set_data("_p", &payload);
        self.connection.send_message(&dict);
        Ok(())
    }

    pub fn send_with_shmem(&self, message: T, _shmem: IpcSharedRegion) -> Result<(), IpcError> {
        let payload = postcard::to_allocvec(&message)
            .map_err(|error| IpcError::Serialize(error.to_string()))?;
        let mut dict = xpc_sys::XpcDictionary::new();
        dict.set_data("_p", &payload);
        // TODO: attach shmem when XPC shared memory is implemented
        self.connection.send_message(&dict);
        Ok(())
    }
}

// ── IpcIncoming ─────────────────────────────────────────────────────────────

/// An incoming message from an extension process.
pub struct IpcIncoming<T> {
    pub payload: T,
    pub shmem: Option<IpcSharedRegion>,
}

impl<T> IpcIncoming<T> {
    pub fn new(payload: T) -> Self {
        IpcIncoming {
            payload,
            shmem: None,
        }
    }
}

// ── IpcSharedRegion ─────────────────────────────────────────────────────────

/// A shared memory region for bulk data transport.
#[cfg(feature = "ipc-channel-backend")]
pub struct IpcSharedRegion(ipc_channel::ipc::IpcSharedMemory);

#[cfg(feature = "ipc-channel-backend")]
impl IpcSharedRegion {
    pub fn allocate(size: usize) -> Result<Self, IpcError> {
        let shmem = ipc_channel::ipc::IpcSharedMemory::from_byte(0, size);
        Ok(IpcSharedRegion(shmem))
    }

    pub fn as_slice(&self) -> &[u8] {
        use std::ops::Deref;
        self.0.deref()
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { self.0.deref_mut() }
    }

    pub fn size(&self) -> usize {
        self.0.len()
    }
}

#[cfg(not(feature = "ipc-channel-backend"))]
pub struct IpcSharedRegion;

#[cfg(not(feature = "ipc-channel-backend"))]
impl IpcSharedRegion {
    pub fn allocate(_size: usize) -> Result<Self, IpcError> {
        Err(IpcError::Transport(
            "native backend: shared memory not yet implemented".into(),
        ))
    }
    pub fn as_slice(&self) -> &[u8] {
        &[]
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut []
    }
    pub fn size(&self) -> usize {
        0
    }
}

// ── ExtensionClient ─────────────────────────────────────────────────────────

/// Client handle obtained by the parent process after starting an extension.
pub struct ExtensionClient<
    Out: Serialize + DeserializeOwned + 'static,
    In: DeserializeOwned + Serialize + 'static,
> {
    pub tx: IpcSender<Out>,
    pub rx: Receiver<IpcIncoming<In>>,
    pub child: Option<std::process::Child>,
}

// ── ExtensionServer ─────────────────────────────────────────────────────────

/// Server handle obtained by the extension process on startup.
pub struct ExtensionServer<
    Out: Serialize + DeserializeOwned + 'static,
    In: DeserializeOwned + Serialize + 'static,
> {
    pub tx: IpcSender<Out>,
    pub rx: Receiver<IpcIncoming<In>>,
    /// On the native XPC backend, the listener connection must be kept alive
    /// for the lifetime of the extension server. The ipc-channel backend
    /// does not use a listener.
    #[cfg(not(feature = "ipc-channel-backend"))]
    pub(crate) _listener: Option<xpc_sys::XpcConnection>,
}
