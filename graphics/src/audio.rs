//! The audio device paths of WebRTC: the default input captured for audio
//! senders, delivered to the net process as `webrtc::Request::PushPcm`, and
//! the decoded audio of remote tracks (`GraphicsCommand::PlayAudioPcm`)
//! played on the default output. Devices exist only with the AVFoundation
//! media backend.

use std::sync::{Arc, Mutex};

use ipc_messages::network::Request as NetworkRequest;
use ipc_messages::webrtc::{PeerConnectionId, Request as WebRtcRequest, TransceiverId};
use log::{debug, error, warn};

#[cfg(all(
    any(target_os = "macos", target_os = "ios"),
    any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
))]
use media::backend::avfoundation::audio_io::AudioIo;

pub(crate) struct AudioState {
    /// The net process, where captured frames go.
    net_sender: Option<ipc::IpcSender<NetworkRequest>>,
    #[cfg(all(
        any(target_os = "macos", target_os = "ios"),
        any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
    ))]
    io: Option<AudioIo>,
    /// The sender whose track is being captured; one capture at a time (the
    /// default input feeds every audio sender the same signal).
    capturing_for: Option<(PeerConnectionId, TransceiverId)>,
}

impl AudioState {
    pub(crate) fn new() -> Self {
        Self {
            net_sender: None,
            #[cfg(all(
                any(target_os = "macos", target_os = "ios"),
                any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
            ))]
            io: None,
            capturing_for: None,
        }
    }

    pub(crate) fn set_net_sender(&mut self, sender: ipc::IpcSender<NetworkRequest>) {
        self.net_sender = Some(sender);
    }

    #[cfg(all(
        any(target_os = "macos", target_os = "ios"),
        any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
    ))]
    fn io(&mut self) -> Option<&mut AudioIo> {
        if self.io.is_none() {
            match AudioIo::new() {
                Ok(io) => self.io = Some(io),
                Err(message) => error!("[graphics:audio] {message}"),
            }
        }
        self.io.as_mut()
    }

    pub(crate) fn start_capture(&mut self, peer: PeerConnectionId, transceiver: TransceiverId) {
        let Some(net_sender) = self.net_sender.clone() else {
            warn!("[graphics:audio] capture requested before the net sender arrived");
            return;
        };
        if self.capturing_for.is_some() {
            debug!("[graphics:audio] capture already running; the default input feeds one sender");
            return;
        }
        #[cfg(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        ))]
        {
            let net_sender = Mutex::new(net_sender);
            let sink = Arc::new(move |samples: Vec<i16>| {
                let request = NetworkRequest::WebRtc(WebRtcRequest::PushPcm {
                    peer,
                    transceiver,
                    samples,
                });
                let sent = match net_sender.lock() {
                    Ok(sender) => sender.send(request),
                    Err(poisoned) => poisoned.into_inner().send(request),
                };
                if let Err(error) = sent {
                    debug!("[graphics:audio] captured frame not delivered: {error}");
                }
            });
            let Some(io) = self.io() else {
                return;
            };
            match io.start_capture(sink) {
                Ok(()) => self.capturing_for = Some((peer, transceiver)),
                Err(message) => error!("[graphics:audio] capture: {message}"),
            }
        }
        #[cfg(not(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        )))]
        {
            drop(net_sender);
            warn!(
                "[graphics:audio] audio capture for {peer:?}/{transceiver:?} needs the AVFoundation media backend"
            );
        }
    }

    pub(crate) fn stop_capture(&mut self, peer: PeerConnectionId, transceiver: TransceiverId) {
        if self.capturing_for != Some((peer, transceiver)) {
            return;
        }
        self.capturing_for = None;
        #[cfg(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        ))]
        if let Some(io) = self.io.as_mut() {
            io.stop_capture();
        }
    }

    pub(crate) fn play(
        &mut self,
        peer: PeerConnectionId,
        transceiver: TransceiverId,
        samples: Vec<i16>,
    ) {
        #[cfg(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        ))]
        {
            let Some(io) = self.io() else {
                return;
            };
            if let Err(message) = io.push_playout((peer.0.as_u128(), transceiver.0), samples) {
                error!("[graphics:audio] playout: {message}");
            }
        }
        #[cfg(not(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        )))]
        debug!(
            "[graphics:audio] {} samples for {peer:?}/{transceiver:?} dropped: no audio output backend",
            samples.len()
        );
    }

    pub(crate) fn stop_playout(&mut self, peer: PeerConnectionId, transceiver: TransceiverId) {
        #[cfg(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        ))]
        if let Some(io) = self.io.as_mut() {
            io.stop_playout((peer.0.as_u128(), transceiver.0));
        }
        #[cfg(not(all(
            any(target_os = "macos", target_os = "ios"),
            any(feature = "backend-avfoundation", not(feature = "backend-gstreamer"))
        )))]
        debug!(
            "[graphics:audio] playout of {peer:?}/{transceiver:?} stopped: no audio output backend"
        );
    }
}
