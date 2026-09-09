//! BrowserEngineKit backend for the abstract IPC API.
//!
//! Extension processes are launched by the OS via BrowserEngineKit, not by
//! `std::process::Command`: `ExtensionManifest::spawn` is never called on
//! this backend; `ExtensionManifest::bek_target` identifies which BEK
//! extension category and bundle ID to launch. Both `Singleton` and
//! `MultiInstance` manifests go through the same path — BEK's process model
//! is inherently multi-instance, so no special casing is needed.
//!
//! The launch is delegated to `bek-sys`, whose Swift shim talks to the real
//! `BrowserEngineKit` framework (iOS/iPadOS only; off-iOS `bek-sys`
//! functions return transport errors, which keeps this module compiling in
//! a macOS build that enables `bek` by mistake). Everything downstream of
//! "I have an `xpc_connection_t`" reuses `xpc-sys` unchanged: the connection
//! BrowserEngineKit hands over is wrapped with `XpcConnection::from_raw`
//! and messages travel as postcard-encoded `_p` data fields in XPC
//! dictionaries.
//!
//! ## Anonymous endpoints (create_endpoint / accept_endpoint)
//!
//! For content ↔ net and content ↔ rendering direct connections, Apple's
//! documented pattern is host-relayed anonymous endpoints:
//!
//! 1. The offering extension creates an anonymous listener
//!    (`xpc_connection_create(NULL, queue)` — [`create_endpoint`]) and
//!    wraps it as an `xpc_endpoint_t` ([`IpcEndpoint`]).
//! 2. The offering extension replies to the host over its existing
//!    connection, carrying the endpoint as a dictionary value
//!    (`XpcDictionary::set_endpoint`) — endpoints are transferable only
//!    inside XPC messages, never as byte blobs.
//! 3. The host relays that dictionary to the connecting extension, which
//!    reads the endpoint back out (`XpcDictionary::get_endpoint`) and calls
//!    [`accept_endpoint`] (`xpc_connection_create_from_endpoint`) to reach
//!    the offering extension directly.
//!
//! Step 2's reply framing runs in the extension's own message loop, which is
//! extension-binary code built on the same `xpc-sys` primitives; this module
//! provides the listener/endpoint mechanics on both sides.
//!
//! When `bek` and `ipc-channel-backend` are both enabled the ipc-channel
//! dispatch arm wins (backend choice per manifest is not yet a runtime
//! decision), so the whole module is compiled but unreachable and stays
//! marked dead there. The anonymous-endpoint surface also has no in-tree
//! callers yet — the extension binaries that reply to a host endpoint
//! request are Stage-2 work — so those items are marked dead in every `bek`
//! build until then.
#![cfg_attr(
    all(feature = "bek", feature = "ipc-channel-backend"),
    allow(dead_code)
)]

use std::marker::PhantomData;

use crossbeam_channel::unbounded;
use serde::de::DeserializeOwned;

use crate::IpcError;
use crate::types::{
    BekProcessKind, ExtensionHandle, ExtensionHandleImpl, ExtensionManifest, IpcConnection,
    IpcIncoming, IpcReceiver, IpcSender, IpcSerialize, IpcTransport,
};

use xpc_sys::{XpcConnection, XpcListenerEvent, XpcMessageEvent};

/// The BEK entitlement each extension kind must carry (§2.4 of the design):
/// the host asserts it on its connection to the extension before any message
/// is accepted.
fn kind_entitlement(kind: BekProcessKind) -> &'static str {
    match kind {
        BekProcessKind::Networking => "com.apple.developer.web-browser-engine.networking",
        BekProcessKind::Rendering => "com.apple.developer.web-browser-engine.rendering",
        BekProcessKind::WebContent => "com.apple.developer.web-browser-engine.webcontent",
    }
}

/// Decode an incoming XPC dictionary: the postcard payload lives under `_p`.
/// Shared-memory regions attached by the sender (§6.2 send side) are not yet
/// mapped on the receiving side.
fn forward_message<M>(
    dict: &xpc_sys::XpcDictionary,
    sender: &crossbeam_channel::Sender<IpcIncoming<M>>,
) where
    M: IpcSerialize + DeserializeOwned,
{
    if let Some(data) = dict.get_data("_p") {
        match postcard::from_bytes::<M>(data) {
            Ok(payload) => {
                if let Err(error) = sender.send(IpcIncoming::new(payload)) {
                    log::error!("bek backend: failed to forward incoming message: {error}");
                }
            }
            Err(error) => {
                log::error!("bek backend: deserialize error: {error}");
            }
        }
    }
}

/// Launch an extension process via BrowserEngineKit and return its handle
/// plus the first connection. The OS launches the process; this blocks only
/// until BrowserEngineKit confirms the process object exists.
pub fn launch_extension<M, Out, In>(
    manifest: &M,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (kind, bundle_id) = manifest.bek_target()?;

    let process_handle = bek_sys::launch_process(kind.to_bek_kind(), Some(&bundle_id))
        .map_err(IpcError::Transport)?;

    let raw_connection =
        unsafe { bek_sys::make_xpc_connection(process_handle, kind.to_bek_kind()) }
            .map_err(IpcError::Transport)?;

    // The connection comes from BrowserEngineKit, but everything downstream
    // is the same libxpc machinery the transport always used.
    let connection = unsafe {
        XpcConnection::from_raw(
            raw_connection as xpc_sys::xpc_connection_t,
            xpc_sys::create_queue("com.formal-web.bek"),
        )
    };

    // Require the peer to hold the entitlement matching its extension kind
    // before any message is accepted. Only one peer-requirement may be set
    // per connection, and it must be set before the connection is resumed.
    let expected = xpc_sys::XpcObject::new_bool(true);
    connection
        .require_entitlement_value(kind_entitlement(kind), &expected)
        .map_err(|code| {
            IpcError::Transport(format!(
                "failed to require entitlement {} on the bek connection (code {code})",
                kind_entitlement(kind)
            ))
        })?;

    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();
    connection.set_message_handler(move |event| match event {
        XpcMessageEvent::Message(dict) => forward_message(&dict, &crossbeam_in_tx),
        XpcMessageEvent::Invalidated => {
            log::info!("bek backend: connection invalidated for {kind:?}");
        }
        XpcMessageEvent::Error(desc) => {
            log::warn!("bek backend: connection error for {kind:?}: {desc}");
        }
    });
    connection.resume();

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection,
            _marker: PhantomData,
        },
    };

    let handle = ExtensionHandle {
        inner: ExtensionHandleImpl::Bek {
            process_handle,
            kind,
        },
    };

    Ok((
        handle,
        IpcConnection::new(tx, IpcReceiver::from_crossbeam(crossbeam_in_rx)),
    ))
}

// ── Anonymous endpoints ────────────────────────────────────────────────────

/// An anonymous XPC endpoint: the serializable half of an anonymous
/// listener, relayed to a peer inside an XPC message
/// (`XpcDictionary::set_endpoint` on the sending side,
/// `XpcDictionary::get_endpoint` on the receiving side) and turned back into
/// a live connection by [`accept_endpoint`].
#[cfg_attr(feature = "bek", allow(dead_code))]
pub struct IpcEndpoint {
    endpoint: xpc_sys::XpcEndpoint,
}

impl IpcEndpoint {
    #[cfg_attr(feature = "bek", allow(dead_code))]
    pub fn as_raw(&self) -> xpc_sys::xpc_object_t {
        self.endpoint.as_raw()
    }
}

/// The offering side of an anonymous-endpoint pair: create an anonymous
/// listener, block until the first peer connects through the endpoint, and
/// return the resulting direct connection plus the endpoint to relay.
#[cfg_attr(feature = "bek", allow(dead_code))]
pub fn create_endpoint<Out, In>() -> Result<(IpcConnection<Out, In>, IpcEndpoint), IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let queue = xpc_sys::create_queue("com.formal-web.bek-anon-endpoint");

    let (crossbeam_tx, crossbeam_rx) = unbounded::<IpcIncoming<In>>();
    let (peer_tx, peer_rx) = std::sync::mpsc::sync_channel::<XpcConnection>(1);

    let raw_listener = unsafe { xpc_sys::xpc_connection_create(std::ptr::null(), queue.inner) };
    let listener = unsafe { XpcConnection::from_raw(raw_listener, queue) };

    let sender = crossbeam_tx.clone();
    listener.set_listener_handler(move |event| match event {
        XpcListenerEvent::NewPeer(peer) => {
            let s = sender.clone();
            peer.set_message_handler(move |msg_event| match msg_event {
                XpcMessageEvent::Message(dict) => forward_message(&dict, &s),
                XpcMessageEvent::Invalidated => {
                    log::info!("bek backend: endpoint peer invalidated");
                }
                XpcMessageEvent::Error(desc) => {
                    log::warn!("bek backend: endpoint peer error: {desc}");
                }
            });
            peer.resume();
            let _ = peer_tx.send(peer);
        }
        XpcListenerEvent::Error(desc) => {
            log::warn!("bek backend: anonymous endpoint error: {desc}");
        }
    });
    listener.resume();

    let endpoint = IpcEndpoint {
        endpoint: xpc_sys::XpcEndpoint::from_connection(&listener),
    };

    let peer_conn = peer_rx.recv().map_err(|error| {
        IpcError::Transport(format!(
            "failed to receive endpoint peer connection: {error}"
        ))
    })?;

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection: peer_conn,
            _marker: PhantomData,
        },
    };

    Ok((
        IpcConnection::new(tx, IpcReceiver::from_crossbeam(crossbeam_rx)),
        endpoint,
    ))
}

/// The connecting side of an anonymous-endpoint pair: create a connection
/// that reaches the offering extension's anonymous listener directly.
#[cfg_attr(feature = "bek", allow(dead_code))]
pub fn accept_endpoint<Out, In>(endpoint: &IpcEndpoint) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let queue = xpc_sys::create_queue("com.formal-web.bek-accepted-endpoint");

    let raw_connection =
        unsafe { xpc_sys::xpc_connection_create_from_endpoint(endpoint.as_raw(), queue.inner) };
    if raw_connection.is_null() {
        return Err(IpcError::Transport(
            "xpc_connection_create_from_endpoint failed".into(),
        ));
    }

    let connection = unsafe { XpcConnection::from_raw(raw_connection, queue) };

    let (crossbeam_tx, crossbeam_rx) = unbounded();
    connection.set_message_handler(move |event| match event {
        XpcMessageEvent::Message(dict) => forward_message(&dict, &crossbeam_tx),
        XpcMessageEvent::Invalidated => {
            log::info!("bek backend: accepted endpoint invalidated");
        }
        XpcMessageEvent::Error(desc) => {
            log::warn!("bek backend: accepted endpoint error: {desc}");
        }
    });
    connection.resume();

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection,
            _marker: PhantomData,
        },
    };

    Ok(IpcConnection::new(
        tx,
        IpcReceiver::from_crossbeam(crossbeam_rx),
    ))
}
