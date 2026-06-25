//! Public types for the IPC abstraction layer.
//!
//! Each extension starts as an [`ExtensionHandle`] representing the launched
//! process. From the handle you create [`IpcConnection`]s — one per
//! bidirectional channel. The first connection carries the
//! [`BootstrapPayload`] with named endpoints for additional channels.
//!
//! ## Backend comparison
//!
//! | Concept | ipc-channel | XPC | BEK (future) |
//! |---|---|---|---|
//! | `ExtensionHandle` | `Option<Child>` + listener set | N/A (launchd) | BEWebContentProcess |
//! | `create_connection()` | new one-shot server handshake | `makeLibXPCConnection()` | BEK's system call |
//! | `IpcEndpoint` | bootstrap token string | `xpc_endpoint_t` bytes | `xpc_endpoint_t` bytes |
//!
//! ## Direct connections architecture
//!
//! Content processes talk directly to net and media processes instead of
//! routing through the user_agent. The user_agent owns all extension
//! handles and creates the connections, then sends net/media endpoints to
//! content via the bootstrap message on the first connection.

use std::collections::HashMap;
use std::time::Duration;

use crossbeam_channel::Receiver;
use ipc_channel::ipc::IpcSharedMemory;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::IpcError;

/// A serializable token that represents one end of an anonymous IPC
/// connection. The creator's handle holds the listener end; the recipient
/// calls [`IpcEndpoint::accept()`] to get the connecting end.
///
/// Backend-specific:
/// - **ipc-channel**: wraps a one-shot bootstrap server name.
/// - **XPC**: wraps serialized `xpc_endpoint_t` bytes.
/// - **BEK**: same as XPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEndpoint {
    pub(crate) data: Vec<u8>,
}

impl IpcEndpoint {
    /// Serialize this endpoint to bytes for transport inside a bootstrap
    /// message.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// Deserialize an endpoint from bytes received in a bootstrap message.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        IpcEndpoint { data }
    }

    /// Accept the connecting end of this endpoint, establishing a
    /// bidirectional connection to the creator. Called by the child
    /// process that received the endpoint in a bootstrap message.
    pub fn accept<Out, In>(&self) -> Result<IpcConnection<Out, In>, IpcError>
    where
        Out: IpcSerialize + DeserializeOwned + Send + 'static,
        In: IpcSerialize + DeserializeOwned + Send + 'static,
    {
        #[cfg(feature = "ipc-channel-backend")]
        {
            crate::backend::ipc_channel::accept_endpoint(self)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            crate::backend::xpc::accept_endpoint(self)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
        {
            Err(IpcError::Transport("no IPC backend available".into()))
        }
    }
}

// ── BootstrapToken ──────────────────────────────────────────────────────────

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

// ── BootstrapPayload ─────────────────────────────────────────────────────────

/// Data sent on the first connection to an extension process.
///
/// Contains named [`IpcEndpoint`]s that the child should connect to for
/// additional channels (e.g., content needs net and media endpoints).
///
/// The primary channel is already implicitly established by the connection
/// that carries this payload — only *extra* channels are listed here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPayload {
    /// Named endpoints for additional connections.
    /// Common names: "net", "media", "rendering".
    pub endpoints: HashMap<String, IpcEndpoint>,
    /// Protocol compatibility version.
    pub protocol_version: u32,
}

impl BootstrapPayload {
    pub fn new() -> Self {
        BootstrapPayload {
            endpoints: HashMap::new(),
            protocol_version: 1,
        }
    }

    /// Insert a named endpoint.
    pub fn with_endpoint(mut self, name: &str, endpoint: IpcEndpoint) -> Self {
        self.endpoints.insert(name.to_owned(), endpoint);
        self
    }
}

impl Default for BootstrapPayload {
    fn default() -> Self {
        Self::new()
    }
}

// ── Identifiers ─────────────────────────────────────────────────────────────

/// Identifies one content process instance among many.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentInstanceId {
    pub top_level_origin: String,
    pub webview_id: u64,
    pub event_loop_id: u64,
}

// ── ExtensionEndpoint ────────────────────────────────────────────────────────

/// Describes the IPC topology for an extension process.
pub enum ExtensionEndpoint {
    Singleton { service_name: &'static str },
    MultiInstance { service_name: &'static str },
}

// ── ExtensionManifest ────────────────────────────────────────────────────────

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
pub struct IpcSender<T: IpcSerialize + IpcDeserialize> {
    pub(crate) transport: IpcTransport<T>,
}

/// Wrapped IPC message that carries a payload and a map of shared memory
/// regions indexed by `usize` keys.
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

// Manual Clone — the inner channel/connection types are Clone without T: Clone.
impl<T: IpcSerialize + IpcDeserialize> Clone for IpcTransport<T> {
    fn clone(&self) -> Self {
        match self {
            IpcTransport::IpcChannel(sender) => IpcTransport::IpcChannel(sender.clone()),
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

    /// Send a message with a map of shared memory regions.
    ///
    /// On the ipc-channel backend, the payload and shmem map are wrapped
    /// in a single serde message tuple which the ipc-channel infrastructure
    /// transfers as Mach port / fd handles (O(1) — no byte copying per region).
    ///
    /// On the XPC backend this is a fallback to `send()` — XPC shared memory
    /// via `xpc_shmem_create` is unimplemented.
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
/// Not tied to any particular concurrency primitive. Callers that need
/// `crossbeam_channel::select!` convert with [`IpcReceiver::into_crossbeam`].
pub struct IpcReceiver<T> {
    inner: Receiver<IpcIncoming<T>>,
}

impl<T> IpcReceiver<T> {
    /// Create from a crossbeam channel (used by backends).
    pub(crate) fn from_crossbeam(rx: Receiver<IpcIncoming<T>>) -> Self {
        IpcReceiver { inner: rx }
    }

    /// Block until a message arrives.
    pub fn recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        self.inner.recv().map_err(|_| IpcError::Disconnected)
    }

    /// Block for up to `timeout` duration.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<IpcIncoming<T>, IpcError> {
        self.inner
            .recv_timeout(timeout)
            .map_err(|_| IpcError::Disconnected)
    }

    /// Try to receive without blocking.
    pub fn try_recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        self.inner.try_recv().map_err(|error| match error {
            crossbeam_channel::TryRecvError::Empty => IpcError::Disconnected,
            crossbeam_channel::TryRecvError::Disconnected => IpcError::Disconnected,
        })
    }

    /// Convert to a crossbeam channel for use in `select!`.
    /// Consumes the receiver.
    pub fn into_crossbeam(self) -> Receiver<IpcIncoming<T>> {
        self.inner
    }

    /// Borrow the inner crossbeam channel without consuming.
    pub fn crossbeam(&self) -> &Receiver<IpcIncoming<T>> {
        &self.inner
    }
}

impl<T> Clone for IpcReceiver<T> {
    fn clone(&self) -> Self {
        IpcReceiver {
            inner: self.inner.clone(),
        }
    }
}

// ── IpcConnection ──────────────────────────────────────────────────────────

/// A single bidirectional IPC connection to an extension process.
///
/// On BEK this wraps one `xpc_connection_t` obtained from
/// `makeLibXPCConnection()`. On ipc-channel it wraps a pair of
/// `IpcSender`/`IpcReceiver` established through a bootstrap handshake.
pub struct IpcConnection<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> {
    pub sender: IpcSender<Out>,
    pub receiver: IpcReceiver<In>,
}

// Manual Clone impl — the type params are phantom at the Clone level.
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

impl<Out: IpcSerialize + IpcDeserialize, In: IpcSerialize + IpcDeserialize> IpcConnection<Out, In> {
    pub fn new(sender: IpcSender<Out>, receiver: IpcReceiver<In>) -> Self {
        IpcConnection { sender, receiver }
    }

    /// Split into sender and receiver halves.
    pub fn into_split(self) -> (IpcSender<Out>, IpcReceiver<In>) {
        (self.sender, self.receiver)
    }
}

// ── ExtensionHandle ─────────────────────────────────────────────────────────

/// A handle to a launched extension process.
///
/// On BEK this wraps a `BEWebContentProcess` / `BERenderingProcess` /
/// `BENetworkingProcess`.  On ipc-channel it holds the child process handle.
/// On launchd XPC it is a unit type (launchd manages lifecycle).
///
/// The handle can create multiple [`IpcConnection`]s to the same extension.
/// The first connection typically carries the [`BootstrapPayload`] with
/// named endpoints for additional channels.
pub struct ExtensionHandle {
    pub(crate) inner: ExtensionHandleImpl,
}

pub(crate) enum ExtensionHandleImpl {
    /// ipc-channel backend: child process handle.
    /// The child was spawned with a bootstrap token; the first connection
    /// has already been established.
    IpcChannel { child: Option<std::process::Child> },
    /// XPC backend (launchd singleton): no process handle, launchd
    /// manages lifecycle. Connection creation re-connects to the
    /// Mach service.
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    XpcSingleton { service_name: &'static str },
    /// BEK: opaque process handle (future).
    #[cfg(feature = "bek")]
    Bek,
}

impl ExtensionHandle {
    /// Start an extension process from its manifest.
    /// This is the primary entry point for the parent process.
    /// Returns a handle and the first connection to the extension.
    pub fn launch<M, Out, In>(
        manifest: &M,
        bootstrap: BootstrapPayload,
    ) -> Result<(Self, IpcConnection<Out, In>), IpcError>
    where
        M: ExtensionManifest,
        Out: IpcSerialize + IpcDeserialize + Send + 'static,
        In: IpcSerialize + IpcDeserialize + Send + 'static,
    {
        #[cfg(feature = "ipc-channel-backend")]
        {
            crate::backend::ipc_channel::launch_extension(manifest, bootstrap)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
        {
            crate::backend::xpc::launch_extension(manifest, bootstrap)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
        {
            let _ = manifest;
            let _ = bootstrap;
            Err(IpcError::Transport(
                "no IPC backend available on this platform".into(),
            ))
        }
    }

    /// Create a new connection to this extension.
    ///
    /// BEK: calls `makeLibXPCConnection()` and wraps the returned
    /// `xpc_connection_t`.
    ///
    /// ipc-channel: creates a new one-shot bootstrap handshake. The child
    /// must be listening for additional connections (set up during the
    /// initial bootstrap).
    ///
    /// XPC launchd: creates a new connection to the Mach service by calling
    /// `xpc_connection_create`.  The child process receives a new call to
    /// its `handle(xpcConnection:)` entry point.
    pub fn create_connection<Out, In>(&self) -> Result<IpcConnection<Out, In>, IpcError>
    where
        Out: IpcSerialize + IpcDeserialize + Send + 'static,
        In: IpcSerialize + IpcDeserialize + Send + 'static,
    {
        match &self.inner {
            ExtensionHandleImpl::IpcChannel { .. } => {
                #[cfg(feature = "ipc-channel-backend")]
                {
                    crate::backend::ipc_channel::create_connection(self)
                }
                #[cfg(not(feature = "ipc-channel-backend"))]
                {
                    let _ = self;
                    Err(IpcError::Transport(
                        "create_connection not supported with this backend".into(),
                    ))
                }
            }
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            ExtensionHandleImpl::XpcSingleton { service_name } => {
                crate::backend::xpc::create_connection::<Out, In>(service_name)
            }
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek => Err(IpcError::Transport(
                "BEK connection creation not yet implemented".into(),
            )),
        }
    }

    /// Create an anonymous endpoint that another process can connect to.
    ///
    /// Returns a connection on the caller's side and a serializable
    /// endpoint that can be sent to a child process. The child calls
    /// [`IpcEndpoint::accept()`] to obtain the other end.
    ///
    /// Used to give content processes direct connections to net and media
    /// processes: the user_agent creates an endpoint from the net handle,
    /// sends it in the bootstrap payload to content, and content accepts it.
    pub fn create_endpoint<Out, In>(
        &self,
    ) -> Result<(IpcConnection<Out, In>, IpcEndpoint), IpcError>
    where
        Out: IpcSerialize + IpcDeserialize + Send + 'static,
        In: IpcSerialize + IpcDeserialize + Send + 'static,
    {
        match &self.inner {
            ExtensionHandleImpl::IpcChannel { .. } => {
                #[cfg(feature = "ipc-channel-backend")]
                {
                    crate::backend::ipc_channel::create_endpoint::<Out, In>(self)
                }
                #[cfg(not(feature = "ipc-channel-backend"))]
                {
                    let _ = self;
                    Err(IpcError::Transport(
                        "create_endpoint not supported with this backend".into(),
                    ))
                }
            }
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            ExtensionHandleImpl::XpcSingleton { .. } => {
                crate::backend::xpc::create_endpoint::<Out, In>()
            }
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek => Err(IpcError::Transport(
                "BEK endpoint creation not yet implemented".into(),
            )),
        }
    }

    /// Stop the extension process.
    ///
    /// BEK: calls `invalidate()` on the process handle.
    /// ipc-channel: sends SIGTERM and waits for the child to exit.
    /// XPC launchd: sends the Shutdown message on the connection;
    /// the process terminates when it is done.
    pub fn invalidate(self) {
        match self.inner {
            ExtensionHandleImpl::IpcChannel { child } => {
                if let Some(mut child) = child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
            ExtensionHandleImpl::XpcSingleton { .. } => {
                // launchd-managed; nothing to do.
            }
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
    #[cfg_attr(not(feature = "ipc-channel-backend"), allow(dead_code))]
    pub(crate) fn from_ipc_shmem(shmem: ipc_channel::ipc::IpcSharedMemory) -> Self {
        IpcSharedRegion(shmem)
    }
}

// ── ExtensionClient (legacy adapter) ────────────────────────────────────────

/// Adapter returned by the legacy [`crate::start_extension`] function.
///
/// Wraps the new [`ExtensionHandle`] + [`IpcConnection`] into the
/// old shape for callers that have not yet migrated.
pub struct ExtensionClient<
    Out: IpcSerialize + IpcDeserialize + 'static,
    In: IpcSerialize + IpcDeserialize + 'static,
> {
    pub handle: ExtensionHandle,
    pub connection: IpcConnection<Out, In>,
}

impl<Out: IpcSerialize + IpcDeserialize + 'static, In: IpcSerialize + IpcDeserialize + 'static>
    ExtensionClient<Out, In>
{
    /// The sender half of the primary connection.
    pub fn sender(&self) -> &IpcSender<Out> {
        &self.connection.sender
    }

    /// The receiver half of the primary connection.
    pub fn receiver(&self) -> &IpcReceiver<In> {
        &self.connection.receiver
    }

    /// The child process handle, if any.
    pub fn child(&self) -> Option<&std::process::Child> {
        match &self.handle.inner {
            ExtensionHandleImpl::IpcChannel { child } => child.as_ref(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Consume and return the child process handle, if any.
    pub fn take_child(&mut self) -> Option<std::process::Child> {
        match &mut self.handle.inner {
            ExtensionHandleImpl::IpcChannel { child } => child.take(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

// Deref to IpcConnection so `client.sender` and `client.receiver` still work.
impl<Out: IpcSerialize + IpcDeserialize + 'static, In: IpcSerialize + IpcDeserialize + 'static>
    std::ops::Deref for ExtensionClient<Out, In>
{
    type Target = IpcConnection<Out, In>;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

// ── ExtensionServer ─────────────────────────────────────────────────────────

/// Server handle obtained by the extension process on startup.
///
/// Wraps the primary channel plus a map of named connections established
/// from bootstrap endpoints (e.g., content's direct net and media channels).
pub struct ExtensionServer<
    In: IpcSerialize + IpcDeserialize + 'static,
    Out: IpcSerialize + IpcDeserialize + 'static,
> {
    /// Primary connection to the parent process.
    pub connection: IpcConnection<In, Out>,
    /// Additional connections established from bootstrap endpoints.
    /// Keyed by the name from [`BootstrapPayload::endpoints`].
    /// Common keys: "net", "media", "rendering".
    pub endpoints: HashMap<String, IpcConnection<In, Out>>,
    /// On the XPC backend, the listener connection must be kept alive.
    #[allow(dead_code)]
    pub(crate) _listener: Option<xpc_sys::XpcConnection>,
}

impl<In: IpcSerialize + IpcDeserialize + 'static, Out: IpcSerialize + IpcDeserialize + 'static>
    ExtensionServer<In, Out>
{
    /// Create a new extension server with only the primary connection.
    pub fn new(connection: IpcConnection<In, Out>) -> Self {
        ExtensionServer {
            connection,
            endpoints: HashMap::new(),
            _listener: None,
        }
    }

    /// Convenience accessor: sender to the parent (sends `In`-typed messages).
    pub fn sender(&self) -> &IpcSender<In> {
        &self.connection.sender
    }

    /// Convenience accessor: receiver from the parent (receives `Out`-typed messages).
    pub fn receiver(&self) -> &IpcReceiver<Out> {
        &self.connection.receiver
    }
}

// Re-export serde traits used by IpcSender type bounds.
pub use serde::Serialize as IpcSerialize;
pub use serde::de::DeserializeOwned as IpcDeserialize;
