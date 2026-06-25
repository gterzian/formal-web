use std::collections::HashMap;
use std::time::Duration;

use ipc_channel::ipc::IpcSharedMemory;


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

impl<T: IpcSerialize + IpcDeserialize> Clone for IpcSender<T> {
    fn clone(&self) -> Self {
        IpcSender {
            transport: self.transport.clone(),
        }
    }
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

    /// Send a message with shared memory regions (ipc-channel backend only;
    /// XPC falls back to plain send).
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

/// Transport-agnostic receiver for messages from an extension process.
///
/// Provides blocking and non-blocking receive.  Use
/// [`ipc::crossbeam_proxy`](crate::crossbeam_proxy) to convert into a
/// `crossbeam_channel::Receiver` for use with `select!`.
pub struct IpcReceiver<T> {
    pub(crate) inner: IpcReceiverImpl<T>,
}

pub(crate) enum IpcReceiverImpl<T> {
    /// ipc-channel backend: backed by a crossbeam channel internally
    /// (the ROUTER callback pushes to it).
    #[cfg(feature = "ipc-channel-backend")]
    Crossbeam(crossbeam_channel::Receiver<IpcIncoming<T>>),
    /// XPC backend: backed by a crossbeam channel (handler pushes to it).
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    Crossbeam(crossbeam_channel::Receiver<IpcIncoming<T>>),
}

impl<T> IpcReceiver<T> {
    /// Block until a message arrives.
    pub fn recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverImpl::Crossbeam(ch) => ch.recv().map_err(|_| IpcError::Disconnected),
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcReceiverImpl::Crossbeam(ch) => ch.recv().map_err(|_| IpcError::Disconnected),
        }
    }

    /// Block for up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<IpcIncoming<T>, IpcError> {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverImpl::Crossbeam(ch) => ch
                .recv_timeout(timeout)
                .map_err(|_| IpcError::Disconnected),
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcReceiverImpl::Crossbeam(ch) => ch
                .recv_timeout(timeout)
                .map_err(|_| IpcError::Disconnected),
        }
    }

    /// Non-blocking receive.
    pub fn try_recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverImpl::Crossbeam(ch) => ch.try_recv().map_err(|e| match e {
                crossbeam_channel::TryRecvError::Empty => IpcError::Disconnected,
                crossbeam_channel::TryRecvError::Disconnected => IpcError::Disconnected,
            }),
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcReceiverImpl::Crossbeam(ch) => ch.try_recv().map_err(|e| match e {
                crossbeam_channel::TryRecvError::Empty => IpcError::Disconnected,
                crossbeam_channel::TryRecvError::Disconnected => IpcError::Disconnected,
            }),
        }
    }
}

impl<T> Clone for IpcReceiver<T> {
    fn clone(&self) -> Self {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverImpl::Crossbeam(ch) => IpcReceiver {
                inner: IpcReceiverImpl::Crossbeam(ch.clone()),
            },
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            IpcReceiverImpl::Crossbeam(ch) => IpcReceiver {
                inner: IpcReceiverImpl::Crossbeam(ch.clone()),
            },
        }
    }
}

/// Convert an [`IpcReceiver`] into a `crossbeam_channel::Receiver` for use
/// with `select!`.
///
/// On the ipc-channel backend the conversion is zero-cost: the inner
/// crossbeam channel is extracted directly.  On the XPC backend a
/// background thread is spawned to bridge the XPC event handler.
pub fn crossbeam_proxy<T>(receiver: IpcReceiver<T>) -> crossbeam_channel::Receiver<IpcIncoming<T>> {
    match receiver.inner {
        #[cfg(feature = "ipc-channel-backend")]
        IpcReceiverImpl::Crossbeam(ch) => ch,
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        IpcReceiverImpl::Crossbeam(ch) => ch,
    }
}

// ── IpcConnection ──────────────────────────────────────────────────────────

/// A single bidirectional IPC connection to an extension process.
///
/// On BEK this wraps one `xpc_connection_t` obtained from
/// `makeLibXPCConnection()`.  On ipc-channel it wraps a pair of
/// `IpcSender`/`IpcReceiver` established through a bootstrap handshake.
pub struct IpcConnection<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> {
    pub sender: IpcSender<Out>,
    pub receiver: IpcReceiver<In>,
}

impl<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> IpcConnection<Out, In> {
    pub fn new(sender: IpcSender<Out>, receiver: IpcReceiver<In>) -> Self {
        IpcConnection { sender, receiver }
    }

    /// Split into sender and receiver halves.
    pub fn into_split(self) -> (IpcSender<Out>, IpcReceiver<In>) {
        (self.sender, self.receiver)
    }
}

impl<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> Clone
    for IpcConnection<Out, In>
{
    fn clone(&self) -> Self {
        IpcConnection {
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
        }
    }
}

// ── ExtensionHandle ─────────────────────────────────────────────────────────

/// A handle to a launched extension process.
///
/// On BEK this wraps a `BEWebContentProcess` / `BERenderingProcess` /
/// `BENetworkingProcess`.  On ipc-channel it holds the child process handle.
/// On launchd XPC it is a unit type (launchd manages lifecycle).
///
/// The handle can create additional [`IpcConnection`]s to the same extension
/// by sending a connection request message over an existing connection.
pub struct ExtensionHandle {
    pub(crate) inner: ExtensionHandleImpl,
}

pub(crate) enum ExtensionHandleImpl {
    /// ipc-channel backend: child process handle.
    IpcChannel {
        child: Option<std::process::Child>,
        /// The bootstrap token used for the initial connection, reused for
        /// additional connection requests sent over the existing connection.
        bootstrap_token: String,
    },
    /// XPC backend (launchd singleton): no process handle.
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    XpcSingleton {
        service_name: &'static str,
    },
    /// BEK: opaque process handle (future).
    #[cfg(feature = "bek")]
    Bek,
}

impl ExtensionHandle {
    /// Start an extension process from its manifest.
    /// Returns a handle and the first connection to the extension.
    pub fn launch<M, Out, In>(
        manifest: &M,
    ) -> Result<(Self, IpcConnection<Out, In>), IpcError>
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

    /// Create a new connection to this extension.
    ///
    /// On XPC this creates a new `xpc_connection_t` to the same Mach service.
    /// On ipc-channel this sends a new one-shot bootstrap token over an
    /// existing connection to the child process.
    ///
    /// # Panics
    ///
    /// Panics if no existing connection exists to send the bootstrap request
    /// over.  Call [`Self::launch`] first to establish at least one connection.
    pub fn create_connection<Out, In>(&self) -> Result<IpcConnection<Out, In>, IpcError>
    where
        Out: IpcSerialize + IpcDeserialize + Send + 'static,
        In: IpcSerialize + IpcDeserialize + Send + 'static,
    {
        match &self.inner {
            ExtensionHandleImpl::IpcChannel { bootstrap_token, .. } => {
                #[cfg(feature = "ipc-channel-backend")]
                {
                    crate::backend::ipc_channel::create_connection::<Out, In>(bootstrap_token)
                }
                #[cfg(not(feature = "ipc-channel-backend"))]
                {
                    let _ = bootstrap_token;
                    Err(IpcError::Transport(
                        "create_connection not supported on this backend".into(),
                    ))
                }
            }
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            ExtensionHandleImpl::XpcSingleton { service_name } => {
                crate::backend::xpc::create_connection::<Out, In>(service_name)
            }
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek => {
                Err(IpcError::Transport(
                    "BEK connection creation not yet implemented".into(),
                ))
            }
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
    /// Shared memory regions indexed by `usize` keys.
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

    pub fn from_bytes(bytes: &[u8]) -> Self {
        IpcSharedRegion(ipc_channel::ipc::IpcSharedMemory::from_bytes(bytes))
    }

    pub fn as_slice(&self) -> &[u8] {
        use std::ops::Deref;
        self.0.deref()
    }

    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        self.0.deref_mut()
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

// ── ExtensionClient (temporary adapter) ─────────────────────────────────────

/// Adapter returned by [`crate::start_extension`] (legacy).
///
/// Provides accessors for the primary connection and child handle.
/// Will be removed once all callers migrate to [`ExtensionHandle::launch`].
pub struct ExtensionClient<
    Out: IpcSerialize + IpcDeserialize + 'static,
    In: IpcSerialize + IpcDeserialize + 'static,
> {
    pub handle: ExtensionHandle,
    pub connection: IpcConnection<Out, In>,
}

impl<
        Out: IpcSerialize + IpcDeserialize + 'static,
        In: IpcSerialize + IpcDeserialize + 'static,
    > ExtensionClient<Out, In>
{
    pub fn sender(&self) -> &IpcSender<Out> {
        &self.connection.sender
    }

    pub fn receiver(&self) -> &IpcReceiver<In> {
        &self.connection.receiver
    }

    pub fn child(&self) -> Option<&std::process::Child> {
        match &self.handle.inner {
            ExtensionHandleImpl::IpcChannel { child, .. } => child.as_ref(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    pub fn take_child(&mut self) -> Option<std::process::Child> {
        match &mut self.handle.inner {
            ExtensionHandleImpl::IpcChannel { child, .. } => child.take(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl<
        Out: IpcSerialize + IpcDeserialize + 'static,
        In: IpcSerialize + IpcDeserialize + 'static,
    > std::ops::Deref for ExtensionClient<Out, In>
{
    type Target = IpcConnection<Out, In>;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

// ── ExtensionServer ─────────────────────────────────────────────────────────

/// Server handle obtained by the extension process on startup.
pub struct ExtensionServer<
    In: IpcSerialize + IpcDeserialize + 'static,
    Out: IpcSerialize + IpcDeserialize + 'static,
> {
    /// Primary connection to the parent process.
    pub connection: IpcConnection<In, Out>,
    /// On the XPC backend, the listener connection must be kept alive.
    #[allow(dead_code)]
    pub(crate) _listener: Option<xpc_sys::XpcConnection>,
}

impl<
        In: IpcSerialize + IpcDeserialize + 'static,
        Out: IpcSerialize + IpcDeserialize + 'static,
    > ExtensionServer<In, Out>
{
    pub fn new(connection: IpcConnection<In, Out>) -> Self {
        ExtensionServer {
            connection,
            _listener: None,
        }
    }

    /// Sender to the parent (sends `In`-typed messages).
    pub fn sender(&self) -> &IpcSender<In> {
        &self.connection.sender
    }

    /// Receiver from the parent (receives `Out`-typed messages).
    pub fn receiver(&self) -> &IpcReceiver<Out> {
        &self.connection.receiver
    }
}

// Re-export serde traits used by IpcSender type bounds.
pub use serde::Serialize as IpcSerialize;
pub use serde::de::DeserializeOwned as IpcDeserialize;
