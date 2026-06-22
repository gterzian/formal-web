//! Public types for the IPC abstraction layer.
//!
//! These types are backend-agnostic. Both backends use serde + postcard
//! for serialization.

use std::collections::HashMap;

use crossbeam_channel::Receiver;
use ipc_channel::ipc::IpcSharedMemory;
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

/// Transport-agnostic sender for messages to an extension process.
///
/// Wraps either an ipc-channel sender (for content in mixed mode, or all
/// extensions in ipc-channel-backend mode) or an XPC connection (for net/media
/// in mixed mode).
#[derive(Clone)]
pub struct IpcSender<T: Serialize + DeserializeOwned> {
    pub(crate) transport: IpcTransport<T>,
}

/// Wrapped IPC message that carries a payload and a map of shared memory
/// regions indexed by `usize` keys.  On the ipc-channel backend the payload
/// and shmem map are sent as a single serde message; the receiver unwraps
/// them into `IpcIncoming`.
pub(crate) type IpcChannelMessage<T> = (T, HashMap<usize, IpcSharedMemory>);

#[derive(Clone)]
pub(crate) enum IpcTransport<T: Serialize + DeserializeOwned> {
    IpcChannel(ipc_channel::ipc::IpcSender<IpcChannelMessage<T>>),
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    Xpc {
        connection: xpc_sys::XpcConnection,
        _marker: std::marker::PhantomData<T>,
    },
}

impl<T: Serialize + DeserializeOwned> IpcSender<T> {
    pub fn send(&self, message: T) -> Result<(), IpcError> {
        match &self.transport {
            IpcTransport::IpcChannel(sender) => sender
                .send((message, HashMap::new()))
                .map_err(|error| IpcError::Transport(error.to_string())),
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcTransport::Xpc { connection, .. } => {
                let payload = postcard::to_allocvec(&message)
                    .map_err(|error| IpcError::Serialize(error.to_string()))?;
                let mut dict = xpc_sys::XpcDictionary::new();
                dict.set_data("_p", &payload);
                connection.send_message(&dict);
                Ok(())
            }
        }
    }

    /// Send a message with a map of shared memory regions.
    ///
    /// On the ipc-channel backend, the payload and shmem map are wrapped
    /// in a single serde message tuple `(T, HashMap<usize, IpcSharedMemory>)`
    /// which the ipc-channel infrastructure transfers as Mach port / fd
    /// handles (O(1) — no byte copying per region).
    ///
    /// On the XPC backend this is a fallback to `send()` — XPC shared memory
    /// via `xpc_shmem_create` is unimplemented (the XPC backend is not used
    /// for content or media, which are the only callers that transfer bulk
    /// scene or video frame data).
    pub fn send_with_shmem_map(
        &self,
        message: T,
        shmem_map: HashMap<usize, IpcSharedRegion>,
    ) -> Result<(), IpcError> {
        match &self.transport {
            IpcTransport::IpcChannel(sender) => {
                let raw_map: HashMap<usize, IpcSharedMemory> = shmem_map
                    .into_iter()
                    .map(|(key, region)| (key, region.into_inner()))
                    .collect();
                sender
                    .send((message, raw_map))
                    .map_err(|error| IpcError::Transport(error.to_string()))
            }
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcTransport::Xpc { .. } => {
                // XPC shared memory via xpc_shmem_create is unimplemented.
                self.send(message)
            }
        }
    }
}

// ── IpcIncoming ─────────────────────────────────────────────────────────────

/// An incoming message from an extension process.
pub struct IpcIncoming<T> {
    pub payload: T,
    /// Shared memory regions indexed by `usize` keys.  The message payload
    /// carries matching `usize` keys that the receiver uses to look up the
    /// corresponding data.
    pub shmem_regions: HashMap<usize, IpcSharedRegion>,
}

impl<T> IpcIncoming<T> {
    pub fn new(payload: T) -> Self {
        IpcIncoming {
            payload,
            shmem_regions: HashMap::new(),
        }
    }
}

// ── IpcSharedRegion ─────────────────────────────────────────────────────────

/// A shared memory region for bulk data transport.
#[derive(Clone, Debug, PartialEq)]
pub struct IpcSharedRegion(ipc_channel::ipc::IpcSharedMemory);

impl IpcSharedRegion {
    pub fn allocate(size: usize) -> Result<Self, IpcError> {
        let shmem = ipc_channel::ipc::IpcSharedMemory::from_byte(0, size);
        Ok(IpcSharedRegion(shmem))
    }

    /// Create from bytes by copying into a new shared memory region.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        IpcSharedRegion(ipc_channel::ipc::IpcSharedMemory::from_bytes(bytes))
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

    /// Consume and return the inner `IpcSharedMemory`.
    pub(crate) fn into_inner(self) -> ipc_channel::ipc::IpcSharedMemory {
        self.0
    }

    /// Wrap an `IpcSharedMemory` received from the ipc-channel backend.
    pub(crate) fn from_ipc_shmem(shmem: ipc_channel::ipc::IpcSharedMemory) -> Self {
        IpcSharedRegion(shmem)
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
    /// On the XPC backend, the listener connection must be kept alive
    /// for the lifetime of the extension server. The ipc-channel backend
    /// leaves this as None.
    #[allow(dead_code)]
    pub(crate) _listener: Option<xpc_sys::XpcConnection>,
}
