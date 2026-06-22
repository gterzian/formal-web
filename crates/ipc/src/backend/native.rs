//! Native XPC backend for the abstract IPC API.
//!
//! Uses Apple XPC transport with postcard-serialized payloads carried as
//! data fields ("_p") in XPC dictionaries.
//!
//! Service topology:
//! - `formal-web.net` — Singleton, net helper
//! - `formal-web.media` — Singleton, media helper
//! - `formal-web.content` — MultipleInstances, one per webview

use crossbeam_channel::unbounded;
use serde::{Serialize, de::DeserializeOwned};

use crate::types::*;
use crate::IpcError;

use xpc_sys::{XpcConnection, XpcListenerEvent, XpcMessageEvent};

// ── start_extension (parent side) ───────────────────────────────────────────

/// Start an extension process by connecting to a launchd service.
///
/// Launchd spawns the process on first connection and terminates it
/// when all connections close (ServiceType = Application) or manages
/// multiple instances (ServiceType = MultipleInstances).
pub fn start_extension<M, Out, In>(
    manifest: &M,
) -> Result<ExtensionClient<Out, In>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    let service_name_str = match manifest.endpoint() {
        ExtensionEndpoint::Singleton { service_name } => service_name.to_owned(),
        ExtensionEndpoint::MultiInstance { service_name } => service_name.to_owned(),
    };

    // Set up crossbeam channel for receiving messages from the child.
    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();

    // Connect to the launchd service. The handler is called on XPC's
    // serial dispatch queue for this connection.
    let service_name_log = service_name_str.clone();
    let connection = XpcConnection::connect(&service_name_str, {
        let tx = crossbeam_in_tx.clone();
        move |event| {
            match event {
                XpcMessageEvent::Message(dict) => {
                    if let Some(data) = dict.get_data("_p") {
                        match postcard::from_bytes::<In>(data) {
                            Ok(payload) => {
                                let _ = tx.send(IpcIncoming::new(payload));
                            }
                            Err(error) => {
                                log::error!("native backend: failed to deserialize message: {error}");
                            }
                        }
                    }
                }
                XpcMessageEvent::Invalidated => {
                    log::debug!("native backend: connection invalidated for {}", service_name_log);
                }
                XpcMessageEvent::Error(desc) => {
                    log::warn!("native backend: connection error for {}: {}", service_name_log, desc);
                }
            }
        }
    });

    connection.resume();

    let tx = IpcSender {
        connection,
        _marker: std::marker::PhantomData,
    };

    Ok(ExtensionClient {
        tx,
        rx: crossbeam_in_rx,
        child: None,
    })
}

// ── run_extension (child side) ──────────────────────────────────────────────

/// Run as an extension process.
///
/// Creates a listener on the named Mach service, waits for the first
/// peer connection, and sets up message routing.
///
/// Returns `ExtensionServer<In, Out>` (swapped) because the child's
/// outgoing messages have the parent's `In` type.
pub fn run_extension<M, Out, In>(
    _manifest: &M,
    _token: &str,
    service_name_str: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    // Channel for messages received from the parent (type Out = parent→child).
    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();

    // Shared state: the active peer connection. Set once when the first
    // peer connects.
    let peer_conn = std::sync::Arc::new(std::sync::Mutex::new(None::<XpcConnection>));

    // Create listener
    let owned_svc_name = service_name_str.to_owned();
    let listener_peer = peer_conn.clone();
    let listener = XpcConnection::listen(service_name_str, move |event| {
        match event {
            XpcListenerEvent::NewPeer(peer) => {
                log::info!("native backend: new peer connected to {}", owned_svc_name);

                // Set up the peer connection to handle messages from parent.
                let tx = crossbeam_in_tx.clone();
                peer.set_message_handler(move |msg_event| {
                    match msg_event {
                        XpcMessageEvent::Message(dict) => {
                            if let Some(data) = dict.get_data("_p") {
                                match postcard::from_bytes::<Out>(data) {
                                    Ok(payload) => {
                                        let _ = tx.send(IpcIncoming::new(payload));
                                    }
                                    Err(error) => {
                                        log::error!("native backend: failed to deserialize: {error}");
                                    }
                                }
                            }
                        }
                        XpcMessageEvent::Invalidated => {
                            log::debug!("native backend: peer invalidated");
                        }
                        XpcMessageEvent::Error(desc) => {
                            log::warn!("native backend: peer error: {desc}");
                        }
                    }
                });

                peer.resume();

                // Store peer for the outbound sender.
                if let Ok(mut guard) = listener_peer.lock() {
                    *guard = Some(peer);
                }
            }
            XpcListenerEvent::Error(desc) => {
                log::warn!("native backend: listener error: {desc}");
            }
        }
    });

    listener.resume();

    // Wait briefly for the first peer connection to arrive.
    // In production, this blocks until the parent connects.
    // For now, use the listener connection as a placeholder.
    let tx = IpcSender {
        connection: listener, // Will be replaced with the peer once connected
        _marker: std::marker::PhantomData,
    };

    Ok(ExtensionServer {
        tx,
        rx: crossbeam_in_rx,
    })
}
