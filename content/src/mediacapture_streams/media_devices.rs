use ipc_messages::webrtc::TrackKind;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{named_dom_exception_value, rejected_promise, resolved_promise};

use super::media_stream::MediaStream;
use super::media_stream_track::{MediaStreamTrack, TrackSource};

type JsObject = <Types as JsTypes>::JsObject;

/// The device id, kind and label of the one audio input and the one audio
/// output the user agent exposes.
const AUDIO_INPUT_DEVICE_ID: &str = "default";
const AUDIO_INPUT_LABEL: &str = "Default - formal-web audio input";
const AUDIO_OUTPUT_DEVICE_ID: &str = "default";
const AUDIO_OUTPUT_LABEL: &str = "Default - formal-web audio output";

/// <https://w3c.github.io/mediacapture-main/#dom-mediadevicekind>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaDeviceKind {
    AudioInput,
    AudioOutput,
}

impl MediaDeviceKind {
    pub(crate) fn as_idl(self) -> &'static str {
        match self {
            Self::AudioInput => "audioinput",
            Self::AudioOutput => "audiooutput",
        }
    }
}

/// <https://w3c.github.io/mediacapture-main/#dom-mediadeviceinfo>
#[gc_struct]
pub(crate) struct MediaDeviceInfo {
    /// <https://w3c.github.io/mediacapture-main/#dom-mediadeviceinfo-deviceid>
    #[ignore_trace]
    pub(crate) device_id: String,
    /// <https://w3c.github.io/mediacapture-main/#dom-mediadeviceinfo-kind>
    #[ignore_trace]
    pub(crate) kind: MediaDeviceKind,
    /// <https://w3c.github.io/mediacapture-main/#dom-mediadeviceinfo-label>
    #[ignore_trace]
    pub(crate) label: String,
    /// <https://w3c.github.io/mediacapture-main/#dom-mediadeviceinfo-groupid>
    #[ignore_trace]
    pub(crate) group_id: String,
}

/// <https://w3c.github.io/mediacapture-main/#dom-mediastreamconstraints>
#[derive(Debug, Clone, Default)]
pub(crate) struct MediaStreamConstraints {
    /// <https://w3c.github.io/mediacapture-main/#dom-mediastreamconstraints-audio>
    pub(crate) audio: bool,
    /// <https://w3c.github.io/mediacapture-main/#dom-mediastreamconstraints-video>
    pub(crate) video: bool,
}

/// <https://w3c.github.io/mediacapture-main/#dom-mediadevices>
#[gc_struct]
pub(crate) struct MediaDevices {
    /// The object's EventTarget base.
    pub(crate) event_target: EventTarget,
}

impl EventTargetAccess for MediaDevices {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl MediaDevices {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            event_target: EventTarget::new(ec),
        }
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediadevices-enumeratedevices>
    pub(crate) fn enumerate_devices(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let p be a new promise.
        // Step 2: Let mediaDevices be this.
        // Step 3: Run the following steps in parallel:
        // Step 3.1: Let resultList be the result of creating a list of device
        // info objects with mediaDevices.
        // Step 3.2: Queue a task to resolve p with resultList.
        // Note: The device list is fixed: the user agent's default audio input
        // and default audio output, without a video input; it is known here,
        // so the promise resolves without a task.
        let devices = [
            MediaDeviceInfo {
                device_id: String::from(AUDIO_INPUT_DEVICE_ID),
                kind: MediaDeviceKind::AudioInput,
                label: String::from(AUDIO_INPUT_LABEL),
                group_id: String::from("default"),
            },
            MediaDeviceInfo {
                device_id: String::from(AUDIO_OUTPUT_DEVICE_ID),
                kind: MediaDeviceKind::AudioOutput,
                label: String::from(AUDIO_OUTPUT_LABEL),
                group_id: String::from("default"),
            },
        ];
        let result_list = ec.create_empty_array();
        for device in devices {
            let object = create_interface_instance::<Types, MediaDeviceInfo>(device, ec)?;
            ec.array_push(&result_list, Types::value_from_object(object))?;
        }

        // Step 4: Return p.
        resolved_promise(Types::value_from_object(result_list), ec)
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediadevices-getusermedia>
    pub(crate) fn get_user_media(
        &self,
        constraints: MediaStreamConstraints,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let mediaDevices be this.
        // Step 2: Let constraints be the method's first argument.
        // Step 3: Let requestedMediaTypes be the set of media types in
        // constraints with either a dictionary value or a value of true.
        // Step 4: If requestedMediaTypes is the empty set, return a promise
        // rejected with a TypeError.
        if !constraints.audio && !constraints.video {
            let error = ec.new_type_error("At least one of audio and video must be requested");
            return rejected_promise(error, ec);
        }

        // Step 5: Let document be the relevant global object's associated
        // Document.
        // Step 6: If document is not fully active, return a promise rejected
        // with a DOMException object whose name attribute has the value
        // "InvalidStateError".
        // Step 7: Let isInView be the result of running the is in view
        // algorithm.
        // Step 8: Let p be a new promise.
        // Step 9: Run the following steps in parallel:
        // Step 9.1: For each media type kind in requestedMediaTypes:
        // Step 9.1.1: If no sources of type kind are available, reject p
        // with a new DOMException object whose name attribute has the value
        // NotFoundError.
        if constraints.video {
            // Note: The user agent exposes no video input.
            let error = named_dom_exception_value(
                String::from("NotFoundError"),
                String::from("Requested device not found"),
                ec,
            );
            return rejected_promise(error, ec);
        }

        // Step 9.1.2: Read the current permission state for getUserMedia
        // with kind, and if it is "denied", reject p with a new DOMException
        // object whose name attribute has the value NotAllowedError.
        // Step 9.2: Let finalSet be an empty set.
        // Step 9.3: For each media type kind in requestedMediaTypes: ...
        // request permission to use a device, select settings, create a
        // MediaStreamTrack and add it to finalSet.
        // Note: Permission is granted for the default audio input; the
        // constraints are not applied.
        let track = MediaStreamTrack::create(
            TrackSource::Capture {
                device_id: String::from(AUDIO_INPUT_DEVICE_ID),
            },
            TrackKind::Audio,
            String::from(AUDIO_INPUT_LABEL),
            false,
            ec,
        )?;

        // Step 9.4: Let stream be the result of creating a MediaStream with
        // the tracks in finalSet.
        let stream = MediaStream::create(vec![track], ec)?;

        // Step 9.5: Queue a task to resolve p with stream.
        let stream_object = stream
            .object()
            .ok_or_else(|| ec.new_type_error("MediaStream without its object"))?;

        // Step 10: Return p.
        resolved_promise(Types::value_from_object(stream_object), ec)
    }

    /// <https://w3c.github.io/w3c/mediacapture-screen-share/#dom-mediadevices-getdisplaymedia>
    pub(crate) fn get_display_media(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let mediaDevices be this.
        // Step 2: If the relevant global object of this does not have
        // transient activation, return a promise rejected with a DOMException
        // object whose name attribute has the value InvalidStateError.
        // Steps 3 to 9: constraints, permission, the display surface the user
        // picks.
        // Note: No display capture source exists: the request is denied.
        let error = named_dom_exception_value(
            String::from("NotAllowedError"),
            String::from("Display capture is not available"),
            ec,
        );
        rejected_promise(error, ec)
    }
}
