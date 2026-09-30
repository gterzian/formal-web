use ipc_messages::webrtc::TrackKind;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::dom::fire_event_using;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

use super::events::MediaStreamTrackEvent;
use super::media_stream_track::{MediaStreamTrack, MediaStreamTrackState};

type JsObject = <Types as JsTypes>::JsObject;

/// The constructor's argument after overload resolution.
/// <https://w3c.github.io/mediacapture-main/#dom-mediastream>
pub(crate) enum MediaStreamInit {
    Empty,
    Stream(MediaStream),
    Tracks(Vec<MediaStreamTrack>),
}

/// <https://w3c.github.io/mediacapture-main/#dom-mediastream>
#[gc_struct]
pub(crate) struct MediaStream {
    /// The stream's EventTarget base.
    pub(crate) event_target: EventTarget,

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-id>
    #[ignore_trace]
    id: String,

    /// <https://w3c.github.io/mediacapture-main/#stream-track-set>
    tracks: GcCell<Vec<MediaStreamTrack>>,
}

impl EventTargetAccess for MediaStream {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl MediaStream {
    /// <https://w3c.github.io/mediacapture-main/#dfn-create-a-mediastream>
    pub(crate) fn create(
        tracks: Vec<MediaStreamTrack>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Self::create_with_id(uuid::Uuid::new_v4().to_string(), tracks, ec)
    }

    /// <https://w3c.github.io/mediacapture-main/#dfn-create-a-mediastream>
    pub(crate) fn create_with_id(
        id: String,
        tracks: Vec<MediaStreamTrack>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let stream be a newly constructed MediaStream object.
        // Step 2: Initialize stream.id attribute to a newly generated value.
        // Note: A remote stream keeps the id the remote peer gave it.
        // Step 3: If the tracks argument is not null, add each of the
        // MediaStreamTrack objects in tracks to stream's track set.
        let stream = Self {
            event_target: EventTarget::new(ec),
            id,
            tracks: gc_cell_new(tracks, ec),
        };

        // Step 4: Return stream.
        let object = create_interface_instance::<Types, MediaStream>(stream, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<MediaStream>().cloned())
            .ok_or_else(|| ec.new_type_error("MediaStream instance is not a MediaStream"))
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream>
    pub(crate) fn constructor(
        init: MediaStreamInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let stream be the result of creating a MediaStream.
        // Step 2: If the constructor's argument is present, run the following
        // steps:
        let tracks = match init {
            // Step 2.1: Construct a set of tracks tokens as follows: If the
            // argument is a MediaStream object, let tokens be a copy of its
            // track set; if the argument is a sequence of MediaStreamTrack
            // objects, let tokens be its elements.
            MediaStreamInit::Empty => Vec::new(),
            MediaStreamInit::Stream(stream) => stream.tracks.borrow(ec).clone(),
            MediaStreamInit::Tracks(tracks) => tracks,
        };

        // Step 2.2: For each MediaStreamTrack track in tokens, add track to
        // stream's track set.
        // Note: The set holds each track once.
        let mut unique: Vec<MediaStreamTrack> = Vec::new();
        for track in tracks {
            if !unique.iter().any(|existing| existing.id() == track.id()) {
                unique.push(track);
            }
        }

        // Step 3: Return stream.
        Self::create(unique, ec)
    }

    /// The stream's platform object.
    pub(crate) fn object(&self) -> Option<JsObject> {
        self.event_target.reflector.clone()
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-id>
    pub(crate) fn id(&self) -> String {
        self.id.clone()
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-active>
    pub(crate) fn active(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        // A MediaStream object is said to be active when it has at least one
        // MediaStreamTrack that has not ended.
        self.tracks
            .borrow(ec)
            .iter()
            .any(|track| track.ready_state() == MediaStreamTrackState::Live)
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-gettracks>
    pub(crate) fn get_tracks(&self, ec: &mut dyn ExecutionContext<Types>) -> Vec<MediaStreamTrack> {
        // Returns a sequence of MediaStreamTrack objects representing all the
        // tracks in this stream's track set.
        self.tracks.borrow(ec).clone()
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-getaudiotracks>
    pub(crate) fn get_audio_tracks(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<MediaStreamTrack> {
        // Returns a sequence of MediaStreamTrack objects representing the
        // audio tracks in this stream's track set.
        self.tracks_of_kind(TrackKind::Audio, ec)
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-getvideotracks>
    pub(crate) fn get_video_tracks(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<MediaStreamTrack> {
        // Returns a sequence of MediaStreamTrack objects representing the
        // video tracks in this stream's track set.
        self.tracks_of_kind(TrackKind::Video, ec)
    }

    fn tracks_of_kind(
        &self,
        kind: TrackKind,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<MediaStreamTrack> {
        self.tracks
            .borrow(ec)
            .iter()
            .filter(|track| track.kind() == kind)
            .cloned()
            .collect()
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-gettrackbyid>
    pub(crate) fn get_track_by_id(
        &self,
        track_id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<MediaStreamTrack> {
        // The getTrackById() method MUST return either a MediaStreamTrack
        // object from this stream's track set whose id is equal to trackId,
        // or null, if no such track exists.
        self.tracks
            .borrow(ec)
            .iter()
            .find(|track| track.id() == track_id)
            .cloned()
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-addtrack>
    pub(crate) fn add_track(&self, track: MediaStreamTrack, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: Let track be the MediaStreamTrack argument and stream this
        // MediaStream object.
        // Step 2: If track is already in stream's track set, then abort these
        // steps.
        if self.contains(&track, ec) {
            return;
        }

        // Step 3: Add track to stream's track set.
        self.tracks.borrow_mut(ec).push(track);
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-removetrack>
    pub(crate) fn remove_track(
        &self,
        track: &MediaStreamTrack,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 1: Let track be the MediaStreamTrack argument and stream this
        // MediaStream object.
        // Step 2: If track is not in stream's track set, then abort these
        // steps.
        // Step 3: Remove track from stream's track set.
        let id = track.id();
        self.tracks
            .borrow_mut(ec)
            .retain(|existing| existing.id() != id);
    }

    pub(crate) fn contains(
        &self,
        track: &MediaStreamTrack,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> bool {
        let id = track.id();
        self.tracks
            .borrow(ec)
            .iter()
            .any(|existing| existing.id() == id)
    }

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastream-clone>
    pub(crate) fn clone_stream(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let streamClone be the result of creating a MediaStream.
        // Step 2: For each track in this's track set: let trackClone be the
        // result of cloning a track given track; add trackClone to
        // streamClone's track set.
        let tracks = self.tracks.borrow(ec).clone();
        let mut clones = Vec::with_capacity(tracks.len());
        for track in tracks {
            clones.push(track.clone_track(ec)?);
        }

        // Step 3: Return streamClone.
        Self::create(clones, ec)
    }

    /// <https://w3c.github.io/mediacapture-main/#dfn-add-a-track>
    pub(crate) fn add_a_track(
        &self,
        track: MediaStreamTrack,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If track is already in stream's track set, then abort these
        // steps.
        if self.contains(&track, ec) {
            return Ok(());
        }

        // Step 2: Add track to stream's track set.
        self.tracks.borrow_mut(ec).push(track.clone());

        // Step 3: Fire a track event named addtrack with track at stream.
        let event =
            MediaStreamTrackEvent::new(String::from("addtrack"), false, false, false, track, ec);
        fire_event_using(&self.event_target, event, time_millis, ec)?;
        Ok(())
    }

    /// <https://w3c.github.io/mediacapture-main/#dfn-remove-a-track>
    pub(crate) fn remove_a_track(
        &self,
        track: MediaStreamTrack,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If track is not in stream's track set, then abort these
        // steps.
        if !self.contains(&track, ec) {
            return Ok(());
        }

        // Step 2: Remove track from stream's track set.
        self.remove_track(&track, ec);

        // Step 3: Fire a track event named removetrack with track at stream.
        let event =
            MediaStreamTrackEvent::new(String::from("removetrack"), false, false, false, track, ec);
        fire_event_using(&self.event_target, event, time_millis, ec)?;
        Ok(())
    }
}
