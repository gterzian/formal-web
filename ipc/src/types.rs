use std::collections::HashMap;
use std::time::Duration;

use crate::IpcError;
use ipc_channel::ipc::{self as ipc_ch, IpcSharedMemory};
use ipc_channel::router::ROUTER;

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

    fn spawn(&self, _token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        Err(IpcError::Transport(
            "spawn not available on this backend; launchd manages lifecycle".into(),
        ))
    }
}

// ── IpcSender ───────────────────────────────────────────────────────────────

/// Transport-agnostic sender for messages to an extension process.
pub struct IpcSender<T: IpcSerialize + IpcDeserialize> {
    pub(crate) transport: IpcTransport<T>,
}

pub(crate) type IpcChannelMessage<T> = (T, HashMap<usize, IpcSharedMemory>);

pub(crate) enum IpcTransport<T: IpcSerialize + IpcDeserialize> {
    #[cfg_attr(not(feature = "ipc-channel-backend"), allow(dead_code))]
    IpcChannel(ipc_channel::ipc::IpcSender<IpcChannelMessage<T>>),
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    Xpc {
        connection: xpc_sys::XpcConnection,
        _marker: std::marker::PhantomData<T>,
    },
}

impl<T: IpcSerialize + IpcDeserialize + std::fmt::Debug> std::fmt::Debug for IpcTransport<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcTransport::IpcChannel(s) => write!(formatter, "IpcChannel({s:?})"),
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcTransport::Xpc { .. } => write!(formatter, "Xpc"),
        }
    }
}

impl<T: IpcSerialize + IpcDeserialize> Clone for IpcTransport<T> {
    fn clone(&self) -> Self {
        match self {
            IpcTransport::IpcChannel(s) => IpcTransport::IpcChannel(s.clone()),
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcTransport::Xpc { connection, .. } => IpcTransport::Xpc {
                connection: connection.clone(),
                _marker: std::marker::PhantomData,
            },
        }
    }
}

impl<T: IpcSerialize + IpcDeserialize + std::fmt::Debug> std::fmt::Debug for IpcSender<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.transport {
            IpcTransport::IpcChannel(sender) => write!(formatter, "IpcSender({sender:?})"),
        }
    }
}

impl<T: IpcSerialize + IpcDeserialize> Clone for IpcSender<T> {
    fn clone(&self) -> Self {
        IpcSender {
            transport: self.transport.clone(),
        }
    }
}

// IpcSender is serializable on the ipc-channel backend: the inner
// ipc-channel IpcSender serializes its Mach port right, which the
// receiver deserializes into a working sender in the target process.
#[cfg(feature = "ipc-channel-backend")]
impl<T: IpcSerialize + IpcDeserialize> serde::Serialize for IpcSender<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.transport {
            IpcTransport::IpcChannel(sender) => sender.serialize(serializer),
        }
    }
}

#[cfg(feature = "ipc-channel-backend")]
impl<'de, T: IpcSerialize + IpcDeserialize> serde::Deserialize<'de> for IpcSender<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let sender =
            ipc_channel::ipc::IpcSender::<IpcChannelMessage<T>>::deserialize(deserializer)?;
        Ok(IpcSender {
            transport: IpcTransport::IpcChannel(sender),
        })
    }
}

/// Create a paired sender and receiver for direct inter-process communication.
///
/// The sender and receiver form a channel pair.  One end can be sent
/// to another process via `IpcSender`'s Serialize impl (Mach port rights
/// are transferred through ipc-channel's serde layer).
pub fn channel<T: IpcSerialize + IpcDeserialize>()
-> Result<(IpcSender<T>, IpcReceiver<T>), IpcError> {
    let (tx, rx) = ipc_ch::channel::<IpcChannelMessage<T>>()
        .map_err(|error| IpcError::Transport(format!("failed to create IPC channel: {error}")))?;
    let sender = IpcSender {
        transport: IpcTransport::IpcChannel(tx),
    };
    let receiver = IpcReceiver::from_ipc_channel(rx);
    Ok((sender, receiver))
}

impl<T: IpcSerialize + IpcDeserialize> IpcSender<T> {
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

    /// Convert this sender to an opaque sender that can be used for
    /// low-level Mach IPC operations (e.g., extracting the underlying
    /// Mach port for surface transport).
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
            IpcTransport::Xpc { .. } => self.send(message),
        }
    }
}

// ── IpcReceiver ─────────────────────────────────────────────────────────────

/// Transport-agnostic receiver for messages from an extension process.
///
/// On the ipc-channel backend this wraps the raw ipc-channel receiver.
/// Use [`crate::crossbeam_proxy`] to bridge to a crossbeam channel
/// if you need `select!`.
pub struct IpcReceiver<T: IpcSerialize + IpcDeserialize> {
    #[cfg(feature = "ipc-channel-backend")]
    inner: ipc_channel::ipc::IpcReceiver<IpcChannelMessage<T>>,
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    _xpc_unimplemented: std::marker::PhantomData<T>,
}

impl<T: IpcSerialize + IpcDeserialize> std::fmt::Debug for IpcReceiver<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "ipc-channel-backend")]
        {
            write!(formatter, "IpcReceiver(<ipc-channel>)")
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            write!(formatter, "IpcReceiver(<xpc-unimplemented>)")
        }
    }
}

// SAFETY: wraps a Mach port handle which is trivially Send.
#[cfg(feature = "ipc-channel-backend")]
unsafe impl<T: IpcSerialize + IpcDeserialize> Send for IpcReceiver<T> {}

impl<T: IpcSerialize + IpcDeserialize> IpcReceiver<T> {
    /// Block until a message arrives.
    pub fn recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        #[cfg(feature = "ipc-channel-backend")]
        {
            let (payload, shmem_map): (T, HashMap<usize, IpcSharedMemory>) =
                self.inner.recv().map_err(|_| IpcError::Disconnected)?;
            let regions: HashMap<usize, IpcSharedRegion> = shmem_map
                .into_iter()
                .map(|(key, raw)| (key, IpcSharedRegion::from_ipc_shmem(raw)))
                .collect();
            Ok(IpcIncoming {
                payload,
                shmem_regions: regions,
            })
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            Err(IpcError::Transport(
                "XPC receiver not yet implemented".into(),
            ))
        }
    }

    /// Block for up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<IpcIncoming<T>, IpcError> {
        #[cfg(feature = "ipc-channel-backend")]
        {
            let (payload, shmem_map): (T, HashMap<usize, IpcSharedMemory>) = self
                .inner
                .try_recv_timeout(timeout)
                .map_err(|_| IpcError::Disconnected)?;
            let regions: HashMap<usize, IpcSharedRegion> = shmem_map
                .into_iter()
                .map(|(key, raw)| (key, IpcSharedRegion::from_ipc_shmem(raw)))
                .collect();
            Ok(IpcIncoming {
                payload,
                shmem_regions: regions,
            })
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            Err(IpcError::Transport(
                "XPC receiver not yet implemented".into(),
            ))
        }
    }

    /// Non-blocking receive.
    pub fn try_recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        #[cfg(feature = "ipc-channel-backend")]
        {
            let (payload, shmem_map): (T, HashMap<usize, IpcSharedMemory>) =
                self.inner.try_recv().map_err(|_| IpcError::Disconnected)?;
            let regions: HashMap<usize, IpcSharedRegion> = shmem_map
                .into_iter()
                .map(|(key, raw)| (key, IpcSharedRegion::from_ipc_shmem(raw)))
                .collect();
            Ok(IpcIncoming {
                payload,
                shmem_regions: regions,
            })
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            Err(IpcError::Transport(
                "XPC receiver not yet implemented".into(),
            ))
        }
    }

    /// Internal: create from a raw ipc-channel receiver.
    #[cfg(feature = "ipc-channel-backend")]
    pub(crate) fn from_ipc_channel(
        rx: ipc_channel::ipc::IpcReceiver<IpcChannelMessage<T>>,
    ) -> Self {
        IpcReceiver { inner: rx }
    }

    /// Consume and return the inner ipc-channel receiver.
    #[cfg(feature = "ipc-channel-backend")]
    pub(crate) fn into_inner(self) -> ipc_channel::ipc::IpcReceiver<IpcChannelMessage<T>> {
        self.inner
    }

}

#[cfg(feature = "ipc-channel-backend")]
impl<T: IpcSerialize + IpcDeserialize> serde::Serialize for IpcReceiver<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.inner.serialize(serializer)
    }
}

#[cfg(feature = "ipc-channel-backend")]
impl<'de, T: IpcSerialize + IpcDeserialize> serde::Deserialize<'de> for IpcReceiver<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rx = ipc_channel::ipc::IpcReceiver::<IpcChannelMessage<T>>::deserialize(deserializer)?;
        Ok(IpcReceiver { inner: rx })
    }
}

/// Bridge an [`IpcReceiver`] to a `crossbeam_channel::Receiver` for use
/// with `select!`.
///
/// On the ipc-channel backend this uses the ipc-channel ROUTER to forward
/// messages without spawning a thread.  On the XPC backend this is not yet
/// implemented and panics.
/// Bridge an [`IpcReceiver`] to a `crossbeam_channel::Receiver` for use
/// with `select!`.
///
/// On the ipc-channel backend this registers the raw ipc-channel receiver
/// with the ipc-channel ROUTER (no thread).  On other backends a forwarding
/// thread is spawned.
#[cfg(feature = "ipc-channel-backend")]
pub fn crossbeam_proxy<T: IpcSerialize + IpcDeserialize + Send + 'static>(
    receiver: IpcReceiver<T>,
) -> crossbeam_channel::Receiver<IpcIncoming<T>> {
    let rx = receiver.into_inner();
    let (crossbeam_tx, crossbeam_rx) = crossbeam_channel::unbounded();
    ROUTER.add_typed_route(
        rx,
        Box::new(
            move |message: Result<(T, std::collections::HashMap<usize, IpcSharedMemory>), _>| {
                if let Ok((payload, shmem_map)) = message {
                    let regions: std::collections::HashMap<usize, IpcSharedRegion> = shmem_map
                        .into_iter()
                        .map(|(key, raw)| (key, IpcSharedRegion::from_ipc_shmem(raw)))
                        .collect();
                    let incoming = IpcIncoming {
                        payload,
                        shmem_regions: regions,
                    };
                    let _ = crossbeam_tx.send(incoming);
                }
            },
        ),
    );
    crossbeam_rx
}

/// Bridge an [`IpcReceiver`] to a `crossbeam_channel::Receiver` for use
/// with `select!`.
///
/// On non-ipc-channel backends this spawns a forwarding thread.
#[cfg(not(feature = "ipc-channel-backend"))]
pub fn crossbeam_proxy<T: IpcSerialize + IpcDeserialize + Send + 'static>(
    receiver: IpcReceiver<T>,
) -> crossbeam_channel::Receiver<IpcIncoming<T>> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
        .name("formal-web:ipc-crossbeam-proxy".into())
        .spawn(move || {
            loop {
                match receiver.recv() {
                    Ok(msg) => {
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("failed to spawn crossbeam proxy thread");
    rx
}

// ── IpcConnection ──────────────────────────────────────────────────────────

/// A single bidirectional IPC connection to an extension process.
pub struct IpcConnection<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> {
    pub sender: IpcSender<Out>,
    pub receiver: IpcReceiver<In>,
}

impl<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> IpcConnection<Out, In> {
    pub fn new(sender: IpcSender<Out>, receiver: IpcReceiver<In>) -> Self {
        IpcConnection { sender, receiver }
    }

    pub fn into_split(self) -> (IpcSender<Out>, IpcReceiver<In>) {
        (self.sender, self.receiver)
    }
}

// ── ExtensionHandle ─────────────────────────────────────────────────────────

/// A handle to a launched extension process.
pub struct ExtensionHandle {
    pub(crate) inner: ExtensionHandleImpl,
}

pub(crate) enum ExtensionHandleImpl {
    IpcChannel {
        child: Option<std::process::Child>,
        _bootstrap_token: String,
    },
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    XpcSingleton { service_name: &'static str },
    #[cfg(feature = "bek")]
    Bek,
}

impl ExtensionHandle {
    /// Start an extension process from its manifest.
    pub fn launch<M, Out, In>(manifest: &M) -> Result<(Self, IpcConnection<Out, In>), IpcError>
    where
        M: ExtensionManifest,
        Out: IpcSerialize + IpcDeserialize + Send + 'static,
        In: IpcSerialize + IpcDeserialize + Send + 'static,
    {
        #[cfg(feature = "ipc-channel-backend")]
        {
            crate::backend::ipc_channel::launch_extension(manifest)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            crate::backend::xpc::launch_extension(manifest)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
        {
            let _ = manifest;
            Err(IpcError::Transport(
                "no IPC backend available on this platform".into(),
            ))
        }
    }

    /// Extract the child process handle, if any.
    ///
    /// Note: prefer using [`invalidate`](Self::invalidate) for shutdown.
    /// This method exists because some callers need the raw `Child` handle
    /// for status polling during error recovery.
    pub fn take_child(&mut self) -> Option<std::process::Child> {
        match &mut self.inner {
            ExtensionHandleImpl::IpcChannel { child, .. } => child.take(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Stop the extension process.
    pub fn invalidate(self) {
        match self.inner {
            ExtensionHandleImpl::IpcChannel { child, .. } => {
                if let Some(mut child) = child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            ExtensionHandleImpl::XpcSingleton { .. } => {}
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek => {}
        }
    }
}

// ── IpcIncoming ─────────────────────────────────────────────────────────────

/// An incoming message from an extension process.
pub struct IpcIncoming<T> {
    pub payload: T,
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

#[derive(Clone, Debug, PartialEq)]
pub struct IpcSharedRegion(ipc_channel::ipc::IpcSharedMemory);

impl IpcSharedRegion {
    pub fn allocate(size: usize) -> Result<Self, IpcError> {
        let shmem = ipc_channel::ipc::IpcSharedMemory::from_byte(0, size);
        Ok(IpcSharedRegion(shmem))
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        IpcSharedRegion(ipc_channel::ipc::IpcSharedMemory::from_bytes(bytes))
    }

    pub fn as_slice(&self) -> &[u8] {
        use std::ops::Deref;
        self.0.deref()
    }

    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn into_inner(self) -> ipc_channel::ipc::IpcSharedMemory {
        self.0
    }

    #[cfg_attr(not(feature = "ipc-channel-backend"), allow(dead_code))]
    pub(crate) fn from_ipc_shmem(shmem: ipc_channel::ipc::IpcSharedMemory) -> Self {
        IpcSharedRegion(shmem)
    }
}

// ── ExtensionServer ─────────────────────────────────────────────────────────

/// Server handle obtained by the extension process on startup.
pub struct ExtensionServer<
    In: IpcSerialize + IpcDeserialize + 'static,
    Out: IpcSerialize + IpcDeserialize + 'static,
> {
    pub connection: IpcConnection<In, Out>,
}

impl<In: IpcSerialize + IpcDeserialize + 'static, Out: IpcSerialize + IpcDeserialize + 'static>
    ExtensionServer<In, Out>
{
    pub fn new(connection: IpcConnection<In, Out>) -> Self {
        ExtensionServer { connection }
    }

    pub fn sender(&self) -> &IpcSender<In> {
        &self.connection.sender
    }

    pub fn receiver(&self) -> &IpcReceiver<Out> {
        &self.connection.receiver
    }
}

// Re-export serde traits.
pub use serde::Serialize as IpcSerialize;
pub use serde::de::DeserializeOwned as IpcDeserialize;
