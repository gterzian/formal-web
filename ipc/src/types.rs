use std::collections::HashMap;
use std::time::Duration;

use crate::IpcError;
#[cfg(not(feature = "ipc-channel-backend"))]
use ipc_channel::ipc::IpcSharedMemory;
#[cfg(feature = "ipc-channel-backend")]
use ipc_channel::ipc::{self as ipc_ch, IpcSharedMemory};
#[cfg(feature = "ipc-channel-backend")]
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

/// Which BrowserEngineKit extension category a manifest maps to.
///
/// The discriminants are load-bearing: they are the `kind` values passed to
/// `bek-sys`'s shim functions and must match the `BekKind` constants in
/// `BekShim.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BekProcessKind {
    Networking = 0,
    Rendering = 1,
    WebContent = 2,
}

impl BekProcessKind {
    #[cfg(feature = "bek")]
    pub(crate) fn to_bek_kind(self) -> i32 {
        self as i32
    }
}

/// Capabilities that can be granted to a launched extension process for the
/// duration of a task. Mirrors `BrowserEngineKit.ProcessCapability`;
/// currently only the scheduling hints are mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCapability {
    /// The extension may run at foreground priority while the host is in the
    /// foreground.
    Foreground = 0,
    /// The extension may run in the background to finish work.
    Background = 1,
    /// The extension may remain resident in a suspended state.
    Suspended = 2,
}

impl ProcessCapability {
    #[cfg(feature = "bek")]
    pub(crate) fn to_bek_capability(self) -> i32 {
        self as i32
    }
}

/// A live capability grant on an extension process. Dropping the grant
/// without calling [`CapabilityGrant::invalidate`] leaves the capability in
/// place until the extension process stops.
pub struct CapabilityGrant {
    #[cfg(feature = "bek")]
    inner: bek_sys::GrantHandle,
}

impl CapabilityGrant {
    /// Release the capability from the process it was granted to.
    #[cfg(feature = "bek")]
    pub fn invalidate(self) {
        // SAFETY: `inner` is a live grant handle from `bek_sys::grant_capability`.
        unsafe { bek_sys::invalidate_grant(self.inner) };
    }
    /// Release the capability from the process it was granted to.
    ///
    /// No-op outside the `bek` backend, where grants cannot exist.
    #[cfg(not(feature = "bek"))]
    pub fn invalidate(self) {}
}

/// Manifest that describes how to start and connect to an extension process.
pub trait ExtensionManifest {
    fn endpoint(&self) -> ExtensionEndpoint;

    fn spawn(&self, _token: &BootstrapToken) -> Result<std::process::Child, IpcError> {
        Err(IpcError::Transport(
            "spawn not available on this backend".into(),
        ))
    }

    /// Only required for the `bek` backend. Identifies which BrowserEngineKit
    /// extension category this manifest maps to, and the bundle ID of the
    /// extension target Apple should launch.
    ///
    /// On the `bek` backend the OS launches the extension process — `spawn`
    /// is never called — and the per-instance identity of a
    /// `MultiInstance` extension travels over the resulting connection
    /// rather than as part of process launch.
    fn bek_target(&self) -> Result<(BekProcessKind, String), IpcError> {
        Err(IpcError::Transport(
            "no BrowserEngineKit target configured".into(),
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
    /// libxpc transport: postcard-encoded payloads carried as `_p` data
    /// fields in XPC dictionaries. The name describes the wire mechanism,
    /// not which backend created the connection: under the `bek` backend the
    /// connection comes from BrowserEngineKit, but everything downstream of
    /// "I have an `xpc_connection_t`" is the same.
    //
    // Dead when both backends are compiled in: the ipc-channel dispatch arm
    // wins until backend choice per manifest is a runtime decision (not
    // needed today — see ARCHITECTURE.md).
    #[cfg(feature = "bek")]
    #[cfg_attr(
        all(feature = "bek", feature = "ipc-channel-backend"),
        allow(dead_code)
    )]
    Xpc {
        connection: xpc_sys::XpcConnection,
        _marker: std::marker::PhantomData<T>,
    },
}

impl<T: IpcSerialize + IpcDeserialize + std::fmt::Debug> std::fmt::Debug for IpcTransport<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcTransport::IpcChannel(s) => write!(formatter, "IpcChannel({s:?})"),
            #[cfg(feature = "bek")]
            IpcTransport::Xpc { .. } => write!(formatter, "Xpc"),
        }
    }
}

impl<T: IpcSerialize + IpcDeserialize> Clone for IpcTransport<T> {
    fn clone(&self) -> Self {
        match self {
            IpcTransport::IpcChannel(s) => IpcTransport::IpcChannel(s.clone()),
            #[cfg(feature = "bek")]
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
            #[cfg(feature = "bek")]
            IpcTransport::Xpc { .. } => write!(formatter, "IpcSender(<xpc>)"),
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
            #[cfg(feature = "bek")]
            IpcTransport::Xpc { .. } => Err(serde::ser::Error::custom(
                "an XPC-backed IpcSender cannot be serialized",
            )),
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
#[cfg(feature = "ipc-channel-backend")]
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
            #[cfg(feature = "bek")]
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

    /// Send a message with a map of shared-memory regions attached.
    ///
    /// On the ipc-channel backend each region travels as a Mach port/fd with
    /// zero-copy semantics. On the XPC transport each region is wrapped with
    /// `xpc_shmem_create` and attached to the outgoing dictionary under its
    /// map key; mapping regions on the receiving side is not yet wired (see
    /// `ipc/ARCHITECTURE.md`).
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
            #[cfg(feature = "bek")]
            IpcTransport::Xpc { connection, .. } => {
                let payload = postcard::to_allocvec(&message)
                    .map_err(|error| IpcError::Serialize(error.to_string()))?;
                let mut dict = xpc_sys::XpcDictionary::new();
                dict.set_data("_p", &payload);
                for (key, region) in &shmem_map {
                    // xpc_shmem_create wraps the region's existing mapping;
                    // the object is released after the message is sent.
                    // SAFETY: `region` is a live shared mapping that stays
                    // alive for the duration of this send.
                    let shmem = unsafe {
                        xpc_sys::XpcSharedMemory::wrap(
                            region.as_slice().as_ptr() as *mut std::ffi::c_void,
                            region.size(),
                        )
                    };
                    dict.set_shmem(&key.to_string(), &shmem);
                }
                connection.send_message(&dict);
                Ok(())
            }
        }
    }
}

// ── IpcReceiver ─────────────────────────────────────────────────────────────

enum IpcReceiverInner<T: IpcSerialize + IpcDeserialize> {
    #[cfg(feature = "ipc-channel-backend")]
    IpcChannel(ipc_channel::ipc::IpcReceiver<IpcChannelMessage<T>>),
    /// Incoming messages forwarded from the XPC connection's message handler
    /// (which runs on a libxpc dispatch queue) onto a crossbeam channel.
    #[cfg(feature = "bek")]
    #[cfg_attr(
        all(feature = "bek", feature = "ipc-channel-backend"),
        allow(dead_code)
    )]
    Xpc(crossbeam_channel::Receiver<IpcIncoming<T>>),
}

/// Transport-agnostic receiver for messages from an extension process.
///
/// On the ipc-channel backend this wraps the raw ipc-channel receiver.
/// Use [`crate::crossbeam_proxy`] to bridge to a crossbeam channel
/// if you need `select!`.
pub struct IpcReceiver<T: IpcSerialize + IpcDeserialize> {
    inner: IpcReceiverInner<T>,
}

impl<T: IpcSerialize + IpcDeserialize> std::fmt::Debug for IpcReceiver<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverInner::IpcChannel(_) => write!(formatter, "IpcReceiver(<ipc-channel>)"),
            #[cfg(feature = "bek")]
            IpcReceiverInner::Xpc(_) => write!(formatter, "IpcReceiver(<xpc>)"),
        }
    }
}

// SAFETY: wraps a Mach port handle which is trivially Send (ipc-channel), or
// a crossbeam receiver whose payload type is Send.
#[cfg(feature = "ipc-channel-backend")]
unsafe impl<T: IpcSerialize + IpcDeserialize> Send for IpcReceiver<T> {}
#[cfg(all(not(feature = "ipc-channel-backend"), feature = "bek"))]
unsafe impl<T: IpcSerialize + IpcDeserialize + Send> Send for IpcReceiver<T> {}

impl<T: IpcSerialize + IpcDeserialize> IpcReceiver<T> {
    fn recv_from_inner(&self) -> Result<IpcIncoming<T>, IpcError> {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverInner::IpcChannel(rx) => {
                let (payload, shmem_map): (T, HashMap<usize, IpcSharedMemory>) =
                    rx.recv().map_err(|_| IpcError::Disconnected)?;
                Ok(incoming_with_regions(payload, shmem_map))
            }
            #[cfg(feature = "bek")]
            IpcReceiverInner::Xpc(rx) => rx.recv().map_err(|_| IpcError::Disconnected),
        }
    }

    fn recv_timeout_from_inner(&self, timeout: Duration) -> Result<IpcIncoming<T>, IpcError> {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverInner::IpcChannel(rx) => {
                let (payload, shmem_map): (T, HashMap<usize, IpcSharedMemory>) = rx
                    .try_recv_timeout(timeout)
                    .map_err(|_| IpcError::Disconnected)?;
                Ok(incoming_with_regions(payload, shmem_map))
            }
            #[cfg(feature = "bek")]
            IpcReceiverInner::Xpc(rx) => {
                rx.recv_timeout(timeout).map_err(|_| IpcError::Disconnected)
            }
        }
    }

    fn try_recv_from_inner(&self) -> Result<IpcIncoming<T>, IpcError> {
        match &self.inner {
            #[cfg(feature = "ipc-channel-backend")]
            IpcReceiverInner::IpcChannel(rx) => {
                let (payload, shmem_map): (T, HashMap<usize, IpcSharedMemory>) =
                    rx.try_recv().map_err(|_| IpcError::Disconnected)?;
                Ok(incoming_with_regions(payload, shmem_map))
            }
            #[cfg(feature = "bek")]
            IpcReceiverInner::Xpc(rx) => rx.try_recv().map_err(|_| IpcError::Disconnected),
        }
    }

    /// Block until a message arrives.
    pub fn recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        self.recv_from_inner()
    }

    /// Block for up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<IpcIncoming<T>, IpcError> {
        self.recv_timeout_from_inner(timeout)
    }

    /// Non-blocking receive.
    pub fn try_recv(&self) -> Result<IpcIncoming<T>, IpcError> {
        self.try_recv_from_inner()
    }

    /// Internal: create from a raw ipc-channel receiver.
    #[cfg(feature = "ipc-channel-backend")]
    pub(crate) fn from_ipc_channel(
        rx: ipc_channel::ipc::IpcReceiver<IpcChannelMessage<T>>,
    ) -> Self {
        IpcReceiver {
            inner: IpcReceiverInner::IpcChannel(rx),
        }
    }

    /// Internal: create from a crossbeam receiver fed by an XPC message
    /// handler.
    #[cfg(feature = "bek")]
    #[cfg_attr(
        all(feature = "bek", feature = "ipc-channel-backend"),
        allow(dead_code)
    )]
    pub(crate) fn from_crossbeam(rx: crossbeam_channel::Receiver<IpcIncoming<T>>) -> Self {
        IpcReceiver {
            inner: IpcReceiverInner::Xpc(rx),
        }
    }

    /// Consume and return the inner ipc-channel receiver.
    #[cfg(feature = "ipc-channel-backend")]
    pub(crate) fn into_inner(self) -> ipc_channel::ipc::IpcReceiver<IpcChannelMessage<T>> {
        match self.inner {
            IpcReceiverInner::IpcChannel(rx) => rx,
            #[cfg(feature = "bek")]
            IpcReceiverInner::Xpc(_) => unreachable!("not an ipc-channel receiver"),
        }
    }
}

#[cfg(feature = "ipc-channel-backend")]
fn incoming_with_regions<T>(
    payload: T,
    shmem_map: HashMap<usize, IpcSharedMemory>,
) -> IpcIncoming<T> {
    let regions: HashMap<usize, IpcSharedRegion> = shmem_map
        .into_iter()
        .map(|(key, raw)| (key, IpcSharedRegion::from_ipc_shmem(raw)))
        .collect();
    IpcIncoming {
        payload,
        shmem_regions: regions,
    }
}

#[cfg(feature = "ipc-channel-backend")]
impl<T: IpcSerialize + IpcDeserialize> serde::Serialize for IpcReceiver<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.inner {
            IpcReceiverInner::IpcChannel(rx) => rx.serialize(serializer),
            #[cfg(feature = "bek")]
            IpcReceiverInner::Xpc(_) => Err(serde::ser::Error::custom(
                "an XPC-backed IpcReceiver cannot be serialized",
            )),
        }
    }
}

#[cfg(feature = "ipc-channel-backend")]
impl<'de, T: IpcSerialize + IpcDeserialize> serde::Deserialize<'de> for IpcReceiver<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rx = ipc_channel::ipc::IpcReceiver::<IpcChannelMessage<T>>::deserialize(deserializer)?;
        Ok(IpcReceiver {
            inner: IpcReceiverInner::IpcChannel(rx),
        })
    }
}

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
            while let Ok(msg) = receiver.recv() {
                if tx.send(msg).is_err() {
                    break;
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
    #[cfg_attr(not(feature = "ipc-channel-backend"), allow(dead_code))]
    IpcChannel {
        child: Option<std::process::Child>,
        _bootstrap_token: String,
    },
    #[cfg(feature = "bek")]
    #[cfg_attr(
        all(feature = "bek", feature = "ipc-channel-backend"),
        allow(dead_code)
    )]
    Bek {
        /// Opaque handle to the OS-managed extension process (the Swift
        /// `ProcessBox` retained by `bek-sys`).
        process_handle: bek_sys::ProcessHandle,
        kind: BekProcessKind,
    },
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
        #[cfg(all(not(feature = "ipc-channel-backend"), feature = "bek"))]
        {
            crate::backend::bek::launch_extension(manifest)
        }
        #[cfg(all(not(feature = "ipc-channel-backend"), not(feature = "bek")))]
        {
            let _ = manifest;
            Err(IpcError::Transport(
                "no IPC backend enabled: enable the ipc-channel-backend feature, \
                 or the bek feature on iOS/iPadOS"
                    .into(),
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
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek { .. } => None,
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
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek {
                process_handle,
                kind,
            } => {
                // SAFETY: `process_handle` is a live handle from
                // `bek_sys::launch_process`; invalidate consumes it.
                unsafe { bek_sys::invalidate(process_handle, kind.to_bek_kind()) };
            }
        }
    }

    /// Request a capability grant on the extension process (no-op backend
    /// unless the process was launched by BrowserEngineKit).
    pub fn grant_capability(
        &self,
        capability: ProcessCapability,
    ) -> Result<CapabilityGrant, IpcError> {
        match &self.inner {
            #[cfg(feature = "bek")]
            ExtensionHandleImpl::Bek {
                process_handle,
                kind,
            } => {
                // SAFETY: `process_handle` is a live handle from
                // `bek_sys::launch_process`.
                let grant = unsafe {
                    bek_sys::grant_capability(
                        *process_handle,
                        kind.to_bek_kind(),
                        capability.to_bek_capability(),
                    )
                }
                .map_err(IpcError::Transport)?;
                Ok(CapabilityGrant { inner: grant })
            }
            ExtensionHandleImpl::IpcChannel { .. } => Err(IpcError::Transport(format!(
                "capability grants require the bek backend (requested {capability:?})"
            ))),
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

    /// Returns a mutable view of the region's bytes.
    ///
    /// # Safety
    ///
    /// Only safe when no other party is concurrently reading or writing the
    /// same underlying shared pages.
    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: mirrors the `ipc_channel` contract — the caller guarantees
        // there is only one reader/writer on the data.
        unsafe { self.0.deref_mut() }
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
