//! Native XPC backend for the abstract IPC API.
//!
//! Only Singleton launchd services (net, media) are supported. Content
//! (MultiInstance) cannot use XPC because macOS AMFI rejects ad-hoc-signed
//! embedded XPC services.
//!
//! See `xpc-services/README.md` for setup instructions.
//!
//! ## Architecture
//!
//! Parent: connects to the launchd-registered XPC service name. launchd
//! starts the helper process on first connection and delivers the peer.
//!
//! Child: creates a listener on the XPC service name. launchd delivers
//! the parent's connection to the listener callback.
//!
//! ## Anonymous endpoints (create_endpoint / accept_endpoint)
//!
//! For direct-connection patterns (content ↔ net, content ↔ media),
//! the parent creates an anonymous XPC listener
//! (`xpc_connection_create(NULL, queue)`), extracts a serializable
//! endpoint token (`xpc_endpoint_create`), and sends it to the child
//! process via the bootstrap message. The child creates a connection
//! from the endpoint (`xpc_connection_create_from_endpoint`) which
//! reaches back to the parent's anonymous listener.

use std::collections::HashMap;

use crossbeam_channel::unbounded;
use serde::{Serialize, de::DeserializeOwned};

use crate::IpcError;
use crate::types::{
    BootstrapPayload, ExtensionEndpoint, ExtensionHandle, ExtensionHandleImpl, ExtensionManifest,
    ExtensionServer, IpcConnection, IpcEndpoint, IpcIncoming, IpcReceiver, IpcSender, IpcSerialize,
    IpcTransport,
};

use xpc_sys::{XpcConnection, XpcListenerEvent, XpcMessageEvent, XpcQueue};

// ── launch_extension (parent side) ──────────────────────────────────────────

pub fn launch_extension<M, Out, In>(
    manifest: &M,
    _bootstrap: BootstrapPayload,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    // Only Singleton (launchd-registered) services reach this module.
    let ExtensionEndpoint::Singleton { service_name } = manifest.endpoint() else {
        unreachable!("MultiInstance should be handled before reaching xpc::launch_extension")
    };

    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();
    let handler = move |event| match event {
        XpcMessageEvent::Message(dict) => {
            if let Some(data) = dict.get_data("_p") {
                match postcard::from_bytes::<In>(data) {
                    Ok(payload) => {
                        if let Err(error) = crossbeam_in_tx.send(IpcIncoming::new(payload)) {
                            log::error!(
                                "native backend: failed to forward incoming message: {error}"
                            );
                        }
                    }
                    Err(error) => {
                        log::error!("native backend: deserialize error: {error}");
                    }
                }
            }
        }
        XpcMessageEvent::Invalidated => {
            log::info!("native backend: connection invalidated for {service_name}");
        }
        XpcMessageEvent::Error(desc) => {
            log::warn!("native backend: connection error for {service_name}: {desc}");
        }
    };

    let connection = XpcConnection::connect(service_name, handler);
    connection.resume();

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection,
            _marker: std::marker::PhantomData,
        },
    };

    let handle = ExtensionHandle {
        inner: ExtensionHandleImpl::XpcSingleton { service_name },
    };

    Ok((
        handle,
        IpcConnection::new(tx, IpcReceiver::from_crossbeam(crossbeam_in_rx)),
    ))
}

// ── run_extension (child side) ──────────────────────────────────────────────

pub fn run_extension<Out, In>(
    _token: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let service_name = "formal-web.net";
    run_listen_extension::<Out, In>(service_name)
}

/// For launchd-registered services: listen on the Mach service name
/// and accept the first peer connection.
fn run_listen_extension<Out, In>(service_name: &str) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded::<IpcIncoming<Out>>();
    let (peer_tx, peer_rx) = std::sync::mpsc::sync_channel::<XpcConnection>(1);
    let owned_name = service_name.to_owned();

    let listener = XpcConnection::listen(service_name, move |event| match event {
        XpcListenerEvent::NewPeer(peer) => {
            log::info!("native backend: new peer connected to {owned_name}");

            let sender = crossbeam_in_tx.clone();
            peer.set_message_handler(move |msg_event| match msg_event {
                XpcMessageEvent::Message(dict) => {
                    if let Some(data) = dict.get_data("_p") {
                        match postcard::from_bytes::<Out>(data) {
                            Ok(payload) => {
                                if let Err(error) = sender.send(IpcIncoming::new(payload)) {
                                    log::error!("native backend: child send error: {error}");
                                }
                            }
                            Err(error) => {
                                log::error!("native backend: child deserialize error: {error}");
                            }
                        }
                    }
                }
                XpcMessageEvent::Invalidated => {
                    log::info!("native backend: child peer invalidated");
                }
                XpcMessageEvent::Error(desc) => {
                    log::warn!("native backend: child peer error: {desc}");
                }
            });
            peer.resume();
            let _ = peer_tx.send(peer);
        }
        XpcListenerEvent::Error(desc) => {
            log::warn!("native backend: listener error for {owned_name}: {desc}");
        }
    });

    listener.resume();

    let peer_conn = peer_rx.recv().map_err(|error| {
        IpcError::Transport(format!("failed to receive peer connection: {error}"))
    })?;

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection: peer_conn,
            _marker: std::marker::PhantomData,
        },
    };

    Ok(ExtensionServer::new(IpcConnection::new(tx, crossbeam_rx)))
}
}

// ── create_connection (additional connection to the same service) ───────────

pub fn create_connection<Out, In>(service_name: &str) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();
    let owned_name = service_name.to_owned();
    let handler = move |event| match event {
        XpcMessageEvent::Message(dict) => {
            if let Some(data) = dict.get_data("_p") {
                match postcard::from_bytes::<In>(data) {
                    Ok(payload) => {
                        if let Err(error) = crossbeam_in_tx.send(IpcIncoming::new(payload)) {
                            log::error!("native backend: create_connection send error: {error}");
                        }
                    }
                    Err(error) => {
                        log::error!("native backend: create_connection deserialize error: {error}");
                    }
                }
            }
        }
        XpcMessageEvent::Invalidated => {
            log::info!("native backend: extra connection invalidated for {owned_name}");
        }
        XpcMessageEvent::Error(desc) => {
            log::warn!("native backend: extra connection error for {owned_name}: {desc}");
        }
    };

    let connection = XpcConnection::connect(service_name, handler);
    connection.resume();

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection,
            _marker: std::marker::PhantomData,
        },
    };

    Ok(IpcConnection::new(
        tx,
        IpcReceiver::from_crossbeam(crossbeam_in_rx),
    ))
}

// ── create_endpoint (anonymous listener + endpoint) ─────────────────────────

pub fn create_endpoint<Out, In>() -> Result<(IpcConnection<Out, In>, IpcEndpoint), IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    // Create an anonymous listener.
    // The listener doesn't have a service name (NULL = anonymous).
    let queue = xpc_sys::create_queue("com.formal-web.xpc-anon-endpoint");

    let (crossbeam_sender, crossbeam_receiver) = unbounded::<IpcIncoming<In>>();
    let (peer_tx, peer_rx) = std::sync::mpsc::sync_channel::<XpcConnection>(1);

    let listener = XpcConnection::from_raw(
        unsafe { xpc_sys::xpc_connection_create(std::ptr::null(), queue.inner) },
        queue,
    );

    {
        let sender = crossbeam_sender.clone();
        listener.set_listener_handler(move |event| match event {
            XpcListenerEvent::NewPeer(peer) => {
                log::info!("native backend: anonymous endpoint accepted");

                // Set up message handler on the peer.
                let s = sender.clone();
                peer.set_message_handler(move |msg_event| match msg_event {
                    XpcMessageEvent::Message(dict) => {
                        if let Some(data) = dict.get_data("_p") {
                            match postcard::from_bytes::<In>(data) {
                                Ok(payload) => {
                                    if let Err(error) = s.send(IpcIncoming::new(payload)) {
                                        log::error!(
                                            "native backend: endpoint peer send error: {error}"
                                        );
                                    }
                                }
                                Err(error) => {
                                    log::error!(
                                        "native backend: endpoint peer deserialize error: {error}"
                                    );
                                }
                            }
                        }
                    }
                    XpcMessageEvent::Invalidated => {
                        log::info!("native backend: endpoint peer invalidated");
                    }
                    XpcMessageEvent::Error(desc) => {
                        log::warn!("native backend: endpoint peer error: {desc}");
                    }
                });
                peer.resume();
                let _ = peer_tx.send(peer);
            }
            XpcListenerEvent::Error(desc) => {
                log::warn!("native backend: anonymous endpoint error: {desc}");
            }
        });
    }

    listener.resume();

    // Extract endpoint.
    let endpoint_obj = unsafe { xpc_sys::xpc_endpoint_create(listener.as_raw()) };
    let endpoint_bytes = unsafe {
        /* serialize endpoint to bytes */
        Vec::new()
    };
    // Note: proper XPC endpoint serialization requires putting the raw
    // xpc_endpoint_t into an XPC dictionary and extracting the bytes.
    // For now, this is a placeholder.

    let _endpoint = IpcEndpoint {
        data: endpoint_bytes,
    };

    // Block until a peer connects.
    let _peer_conn = peer_rx.recv().map_err(|error| {
        IpcError::Transport(format!(
            "failed to receive endpoint peer connection: {error}"
        ))
    })?;

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection: _peer_conn,
            _marker: std::marker::PhantomData,
        },
    };

    Ok((
        IpcConnection::new(tx, IpcReceiver::from_crossbeam(crossbeam_receiver)),
        IpcEndpoint {
            data: endpoint_bytes,
        },
    ))
}

// ── accept_endpoint (child side) ────────────────────────────────────────────

pub fn accept_endpoint<Out, In>(endpoint: &IpcEndpoint) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();
    let handler = move |event| match event {
        XpcMessageEvent::Message(dict) => {
            if let Some(data) = dict.get_data("_p") {
                match postcard::from_bytes::<In>(data) {
                    Ok(payload) => {
                        if let Err(error) = crossbeam_in_tx.send(IpcIncoming::new(payload)) {
                            log::error!("native backend: accept_endpoint send error: {error}");
                        }
                    }
                    Err(error) => {
                        log::error!("native backend: accept_endpoint deserialize error: {error}");
                    }
                }
            }
        }
        XpcMessageEvent::Invalidated => {
            log::info!("native backend: accepted endpoint invalidated");
        }
        XpcMessageEvent::Error(desc) => {
            log::warn!("native backend: accepted endpoint error: {desc}");
        }
    };

    // Create connection from endpoint data.
    // Note: this requires xpc_connection_create_from_endpoint which we
    // have in FFI but the endpoint data needs to be deserialized first.
    // Placeholder: just connect to the service name stored in endpoint.
    let service_name = String::from_utf8(endpoint.data.clone())
        .map_err(|_| IpcError::Transport("invalid endpoint data".into()))?;
    let connection = XpcConnection::connect(&service_name, handler);
    connection.resume();

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection,
            _marker: std::marker::PhantomData,
        },
    };

    Ok(IpcConnection::new(
        tx,
        IpcReceiver::from_crossbeam(crossbeam_in_rx),
    ))
}
