//! Native XPC backend for the abstract IPC API.
//!
//! On the native XPC backend, the helper processes are registered as launchd
//! XPC services (see `xpc-services/`). The parent connects to the service,
//! launchd starts the helper process, and the helper creates a listener that
//! receives the parent's connection.
//!
//! ## Usage
//!
//! 1. Build & install helper binaries + plists:
//!    ```
//!    ./xpc-services/install.sh ./target/release
//!    launchctl load ~/Library/LaunchAgents/formal-web.net.plist
//!    launchctl load ~/Library/LaunchAgents/formal-web.media.plist
//!    launchctl load ~/Library/LaunchAgents/formal-web.content.plist
//!    ```
//! 2. Run:
//!    ```
//!    cargo run --release
//!    ```

use crossbeam_channel::unbounded;
use serde::{Serialize, de::DeserializeOwned};

use crate::types::IpcTransport;
use crate::types::{ExtensionClient, ExtensionEndpoint, ExtensionManifest, ExtensionServer};
use crate::{IpcError, IpcIncoming, IpcSender};

use xpc_sys::{XpcConnection, XpcListenerEvent, XpcMessageEvent};

// ── start_extension (parent side) ───────────────────────────────────────────

pub fn start_extension<M, Out, In>(manifest: &M) -> Result<ExtensionClient<Out, In>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    let service_name = match manifest.endpoint() {
        ExtensionEndpoint::Singleton { service_name } => service_name,
        ExtensionEndpoint::MultiInstance { service_name } => service_name,
    };

    let is_multi_instance = matches!(manifest.endpoint(), ExtensionEndpoint::MultiInstance { .. });

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

    let connection = if is_multi_instance {
        // Embedded XPC service in app bundle's XPCServices/.
        // Uses xpc_connection_create to bypass launchd and its watchdog.
        XpcConnection::connect_embedded(service_name, handler)
    } else {
        // Global launchd-registered Mach service.
        XpcConnection::connect(service_name, handler)
    };

    connection.resume();

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection,
            _marker: std::marker::PhantomData,
        },
    };

    Ok(ExtensionClient {
        tx,
        rx: crossbeam_in_rx,
        child: None,
    })
}

// ── run_extension (child side) ──────────────────────────────────────────────
//
// Dispatches to `listen()`-based approach for Singleton (launchd) services,
// and to `xpc_main`-based approach for MultiInstance (embedded XPC) services.

pub fn run_extension<M, Out, In>(
    _manifest: &M,
    _token: &str,
    service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    run_listen_extension::<Out, In>(service_name)
}

/// For launchd-registered services (Singleton): listen on the Mach service
/// name and accept the first peer connection.
fn run_listen_extension<Out, In>(service_name: &str) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    // Channel for receiving messages from the parent (type Out = parent→child).
    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded::<IpcIncoming<Out>>();

    // Channel to hand back the fully-configured peer to the main thread.
    let (peer_tx, peer_rx) = std::sync::mpsc::sync_channel::<XpcConnection>(1);
    let owned_name = service_name.to_owned();

    // Listen on the service name. launchd delivers the parent's connection here.
    let listener = XpcConnection::listen(service_name, move |event| {
        match event {
            XpcListenerEvent::NewPeer(peer) => {
                log::info!("native backend: new peer connected to {owned_name}");

                // Clone the sender for this peer's message handler so it owns
                // its own copy and can be `'static`.
                let sender = crossbeam_in_tx.clone();

                // Configure the peer IMMEDIATELY inside the listener callback,
                // per Apple's XPC lifecycle contract.
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
                        log::info!("native backend: child peer invalidated — closing channel");
                    }
                    XpcMessageEvent::Error(desc) => {
                        log::warn!("native backend: child peer error: {desc}");
                    }
                });

                // Resume the peer BEFORE returning from the listener callback.
                peer.resume();

                // Hand the fully-configured peer back to the main thread.
                let _ = peer_tx.send(peer);
            }
            XpcListenerEvent::Error(desc) => {
                log::warn!("native backend: listener error for {owned_name}: {desc}");
            }
        }
    });

    listener.resume();

    // Wait for the listener to finish configuring the peer connection.
    let peer_conn = peer_rx.recv().map_err(|error| {
        IpcError::Transport(format!("failed to receive peer connection: {error}"))
    })?;

    let tx = IpcSender {
        transport: IpcTransport::Xpc {
            connection: peer_conn,
            _marker: std::marker::PhantomData,
        },
    };

    Ok(ExtensionServer {
        tx,
        rx: crossbeam_in_rx,
        // Keep the listener alive for the server's lifetime so new peers can
        // still connect (e.g. parent reconnects after restart).
        _listener: Some(listener),
    })
}
