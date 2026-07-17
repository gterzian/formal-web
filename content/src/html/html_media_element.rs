use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use log::{debug, error};

use crate::js::Types;

use crate::html::{HTMLElement, await_a_stable_state};
use crate::js::platform_objects::with_global_scope;
use crate::webidl::resolved_promise;
use ipc_messages::content::{Event as ContentEvent, RegisterMediaPipeline};
use ipc_messages::media::VideoPaintId;
use js_engine::gc_struct;

use js_engine::{Completion, ExecutionContext, JsTypes};

/// <https://html.spec.whatwg.org/#media-elements>
#[gc_struct]
pub struct HTMLMediaElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,

    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub network_state: u16,

    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub ready_state: u16,

    /// <https://html.spec.whatwg.org/#the-src-attribute>
    pub current_src: String,

    /// <https://html.spec.whatwg.org/#error-status>
    error: Option<MediaError>,

    /// <https://html.spec.whatwg.org/#dom-media-paused>
    paused: bool,

    /// <https://html.spec.whatwg.org/#dom-media-seeking>
    seeking: bool,

    /// <https://html.spec.whatwg.org/#current-playback-position>
    current_playback_position: f64,

    /// <https://html.spec.whatwg.org/#official-playback-position>
    official_playback_position: f64,

    /// <https://html.spec.whatwg.org/#default-playback-start-position>
    default_playback_start_position: f64,

    /// <https://html.spec.whatwg.org/#dom-media-duration>
    duration: f64,

    /// <https://html.spec.whatwg.org/#can-autoplay-flag>
    can_autoplay: bool,

    /// <https://html.spec.whatwg.org/#delaying-the-load-event-flag>
    delaying_the_load_event: bool,

    /// <https://html.spec.whatwg.org/#is-currently-stalled>
    is_currently_stalled: bool,

    /// <https://html.spec.whatwg.org/#show-poster-flag>
    show_poster: bool,

    /// Globally-unique paint-layer identifier for this video element (UUID v4).
    /// Not traced — this is an internal identifier, not a JS value or GC-managed object.
    #[ignore_trace]
    video_paint_id: VideoPaintId,
}

/// <https://html.spec.whatwg.org/#mediaerror>
#[gc_struct]
pub struct MediaError {
    /// <https://html.spec.whatwg.org/#dom-mediaerror-code>
    pub code: u16,
    /// <https://html.spec.whatwg.org/#dom-mediaerror-message>
    pub message: String,
}

impl MediaError {
    /// <https://html.spec.whatwg.org/#creating-a-mediaerror>
    pub fn new(code: u16, message: String) -> Self {
        Self { code, message }
    }
}

// Network state constants.
impl HTMLMediaElement {
    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub const NETWORK_EMPTY: u16 = 0;
    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub const NETWORK_IDLE: u16 = 1;
    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub const NETWORK_LOADING: u16 = 2;
    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub const NETWORK_NO_SOURCE: u16 = 3;

    // Ready state constants.
    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub const HAVE_NOTHING: u16 = 0;
    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub const HAVE_METADATA: u16 = 1;
    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub const HAVE_CURRENT_DATA: u16 = 2;
    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub const HAVE_FUTURE_DATA: u16 = 3;
    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub const HAVE_ENOUGH_DATA: u16 = 4;

    // Error code constants.
    /// <https://html.spec.whatwg.org/#dom-mediaerror-media_err_aborted>
    pub const MEDIA_ERR_ABORTED: u16 = 1;
    /// <https://html.spec.whatwg.org/#dom-mediaerror-media_err_network>
    pub const MEDIA_ERR_NETWORK: u16 = 2;
    /// <https://html.spec.whatwg.org/#dom-mediaerror-media_err_decode>
    pub const MEDIA_ERR_DECODE: u16 = 3;
    /// <https://html.spec.whatwg.org/#dom-mediaerror-media_err_src_not_supported>
    pub const MEDIA_ERR_SRC_NOT_SUPPORTED: u16 = 4;
}

impl HTMLMediaElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id),
            network_state: Self::NETWORK_EMPTY,
            ready_state: Self::HAVE_NOTHING,
            current_src: String::new(),
            error: None,
            paused: true,
            seeking: false,
            current_playback_position: 0.0,
            official_playback_position: 0.0,
            default_playback_start_position: 0.0,
            duration: f64::NAN,
            can_autoplay: true,
            delaying_the_load_event: false,
            is_currently_stalled: false,
            show_poster: true,
            video_paint_id: VideoPaintId::new(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub(crate) fn network_state(&self) -> u16 {
        self.network_state
    }

    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub(crate) fn ready_state(&self) -> u16 {
        self.ready_state
    }

    /// <https://html.spec.whatwg.org/#dom-media-src>
    pub(crate) fn src(&self) -> String {
        self.html_element
            .element
            .get_attribute("src")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-media-src>
    pub(crate) fn set_src(&mut self, src: &str, ec: &mut dyn ExecutionContext<crate::js::Types>) {

        // Step 1: Set this's src content attribute to the given value.
        self.html_element.element.set_attribute("src", src);

        // Step 2: Invoke the element's media element load algorithm.
        self.media_element_load_algorithm(ec);
    }

    /// <https://html.spec.whatwg.org/#dom-media-currentsrc>
    pub(crate) fn current_src(&self) -> String {
        self.current_src.clone()
    }

    /// Globally-unique paint-layer identifier for this media element.
    #[allow(dead_code)]
    // Note: Reserved for use by future code that needs to read the paint_id from
    // the element (e.g., pipeline teardown cleanup).
    pub(crate) fn video_paint_id(&self) -> VideoPaintId {
        self.video_paint_id
    }

    /// <https://html.spec.whatwg.org/#dom-media-duration>
    pub(crate) fn duration(&self) -> f64 {
        self.duration
    }

    /// <https://html.spec.whatwg.org/#dom-media-paused>
    pub(crate) fn paused(&self) -> bool {
        self.paused
    }

    /// <https://html.spec.whatwg.org/#dom-media-seeking>
    pub(crate) fn seeking(&self) -> bool {
        self.seeking
    }

    /// <https://html.spec.whatwg.org/#dom-media-currenttime>
    pub(crate) fn current_time(&self) -> f64 {
        if self.default_playback_start_position != 0.0 {
            self.default_playback_start_position
        } else {
            self.official_playback_position
        }
    }

    #[allow(dead_code)]
    /// <https://html.spec.whatwg.org/#dom-media-currenttime>
    pub(crate) fn set_current_time(&mut self, time: f64) {
        if self.ready_state == Self::HAVE_NOTHING {
            self.default_playback_start_position = time;
        } else {
            self.official_playback_position = time;
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-error>
    pub(crate) fn error(&self) -> Option<MediaError> {
        self.error.clone()
    }

    /// <https://html.spec.whatwg.org/#media-element-load-algorithm>
    ///
    /// Note: Steps 2–5 (pending task management, abort event) and step 8 (playbackRate)
    /// are no-ops until promise-based play() and the media element event task source
    /// are implemented.  Step 6 (abort event for NETWORK_LOADING/IDLE) is deferred
    /// to event dispatch.
    pub(crate) fn media_element_load_algorithm(
        &mut self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {

        // Step 1: Set this element's is currently stalled to false.
        self.is_currently_stalled = false;

        // Step 2: Abort any already-running instance of the resource selection algorithm
        // for this element.
        // Note: No-op — resource selection runs only once.
        // Step 3: Let pending tasks be a list of all tasks from the media element's media
        // element event task source in one of the task queues.
        // Note: No-op — media element event task source not yet implemented.

        // Step 4: For each task in pending tasks that would resolve pending play promises
        // or reject pending play promises, immediately resolve or reject those promises.
        // Note: No-op — promise-based play() not yet implemented.
        // Step 5: Remove each task in pending tasks from its task queue.
        // Note: No-op — no task queue management yet.

        // Step 6: If networkState is NETWORK_LOADING or NETWORK_IDLE, fire an event named
        // abort at the media element.
        // Note: Deferred to event dispatch — media element event task source not wired.
        // Step 7: If networkState is not set to NETWORK_EMPTY:
        if self.network_state != Self::NETWORK_EMPTY {

            // Step 7.1: Queue a media element task to fire emptied at the media element.
            // Note: Deferred to event dispatch.
            // Step 7.2: If a fetching process is in progress, stop it.
            // Note: No-op — no fetch in progress.

            // Step 7.3: If the assigned media provider object is a MediaSource, detach it.
            // Note: No-op — MediaSource not yet implemented.
            // Step 7.4: Forget the media element's media-resource-specific tracks.
            // Note: No-op — no track support yet.

            // Step 7.5: If readyState is not HAVE_NOTHING, set it to HAVE_NOTHING.
            if self.ready_state != Self::HAVE_NOTHING {
                self.ready_state = Self::HAVE_NOTHING;
            }

            // Step 7.6: If paused is false, set paused to true and reject pending play promises.
            if !self.paused {
                self.paused = true;
                // Note: Reject pending play promises is a no-op — not yet implemented.
            }

            // Step 7.7: If seeking is true, set it to false.
            if self.seeking {
                self.seeking = false;
            }

            // Step 7.8: Set current playback position to 0, official playback position to 0.
            self.current_playback_position = 0.0;
            self.official_playback_position = 0.0;
            // Note: If this changed the official playback position, queue a timeupdate event.
            // Deferred to event dispatch.

            // Step 7.9: Set the timeline offset to NaN.
            // Note: No-op — timeline offset not tracked.
            // Step 7.10: Update the duration attribute to NaN.
            self.duration = f64::NAN;
        }

        // Step 8: Set playbackRate to defaultPlaybackRate.
        // Note: No-op — defaultPlaybackRate is always 1.0.
        // Step 9: Set error to null and can autoplay flag to true.
        self.error = None;
        self.can_autoplay = true;

        // Step 10: Invoke the resource selection algorithm.
        self.resource_selection_algorithm(ec);

        // Step 11: Playback of any previously playing media resource stops.
        // Note: No-op — no active playback in the initial cut.
    }

    /// <https://html.spec.whatwg.org/#resource-selection-algorithm>
    pub(crate) fn resource_selection_algorithm(
        &mut self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {

        // Step 1: Set networkState to NETWORK_NO_SOURCE.
        self.network_state = Self::NETWORK_NO_SOURCE;

        // Step 2: Set show poster flag to true.
        self.show_poster = true;

        // Step 3: If lazy loading is Eager or scripting is disabled, set
        // delaying-the-load-event flag to true.
        self.delaying_the_load_event = true;

        // Step 4: Await a stable state. The synchronous section consists of all remaining
        // steps until the algorithm says the synchronous section has ended.
        // Extract the data needed by the synchronous section before the closure captures it.
        let src_attr = self.html_element.element.get_attribute("src");
        let src = src_attr.filter(|s| !s.is_empty());

        // Resolve the src attribute value to an absolute URL against the document's
        // base URL (creation URL), as required by the spec's current_src definition.
        let resolved_src = src.as_ref().and_then(|s| {
            with_global_scope(ec, |global_scope| Ok(global_scope.creation_url()))
                .ok()
                .flatten()
                .and_then(|base_url| base_url.join(s).ok().map(|url| url.to_string()))
        });

        // Register VideoPaintId in the global registry so that
        // build_frame_composition_metadata can find the same ID.
        let node_id = self.html_element.element.node.node_id;
        let video_paint_id = self.video_paint_id;

        // Extract document_id and navigable_id from the GlobalScope.
        let global_scope_data = with_global_scope(ec, |global_scope| {
            Ok((
                global_scope.document_id(),
                global_scope.source_navigable_id(),
                global_scope.event_sender(),
            ))
        });
        let (document_id, traversable_id, event_sender) = match global_scope_data {
            Ok(values) => values,
            Err(error) => {
                error!("[media] failed to read GlobalScope state: {error:?}");
                (None, None, None)
            }
        };

        // Register the paint_id via GlobalScope so the composition
        // metadata builder can find the same UUID for this video element.
        if let Some(document_id) = document_id {
            let _ = with_global_scope(ec, |global_scope| {
                global_scope.register_video_paint_id(document_id, node_id, video_paint_id);
                Ok(())
            });
        }

        await_a_stable_state(ec, move |job_ec| {

            // Step 5: ⌛ If blocked-on-parser flag is false, populate list of pending text tracks.
            // Note: No-op — text track support not yet implemented.
            // Step 6: ⌛ Determine the mode.
            if let Some(resolved_url) = resolved_src {
                // mode = attribute
                // Step 7: ⌛ Set networkState to NETWORK_LOADING.
                // Note: networkState was already mutated before await_a_stable_state
                // (step 1). This step would set it to NETWORK_LOADING but the closure
                // cannot access &mut self.  State mutations that happen in the
                // synchronous section are tracked as a gap until the media element
                // state is stored behind interior mutability or moved to the microtask.
                // Step 8: ⌛ Queue a media element task to fire loadstart at the media element.
                // Note: Deferred to event dispatch — media element event task source not wired.

                // Step 9 (mode = attribute): Fetch the media resource via the user agent.
                // Send CreatePipeline directly to the media extension, then notify the
                // user agent of the pipeline→webview mapping for video frame routing.
                if let (Some(event_sender), Some(traversable_id), Some(document_id)) =
                    (event_sender, traversable_id, document_id)
                {
                    // Allocate pipeline ID and send CreatePipeline+Play directly to media.
                    let pipeline_id = with_global_scope(job_ec, |global_scope| {
                        Ok(global_scope.allocate_media_pipeline_id())
                    })
                    .ok();

                    if let Some(pipeline_id) = pipeline_id {
                        // Send CreatePipeline + Play directly to the media extension.
                        let media_sender = with_global_scope(job_ec, |global_scope| {
                            Ok(global_scope.media_extension_sender())
                        })
                        .ok()
                        .flatten();

                        if let Some(ref media_sender) = media_sender {
                            if let Err(error) = media_sender.send(
                                ipc_messages::media::MediaCommand::CreatePipeline {
                                    pipeline_id,
                                    url: resolved_url.clone(),
                                },
                            ) {
                                error!("[media] failed to send CreatePipeline: {error}");
                            }
                            if let Err(error) = media_sender
                                .send(ipc_messages::media::MediaCommand::Play { pipeline_id })
                            {
                                error!("[media] failed to send Play: {error}");
                            }
                        }

                        // Notify the UA of the pipeline→webview mapping.
                        let request = RegisterMediaPipeline {
                            url: resolved_url.clone(),
                            document_id,
                            traversable_id,
                            pipeline_id,
                            video_paint_id,
                        };
                        debug!(
                            "[media] registering pipeline with UA url={} traversable={}",
                            resolved_url, traversable_id
                        );
                        if let Err(error) =
                            event_sender.send(ContentEvent::RegisterMediaPipeline(request))
                        {
                            error!("[media] failed to send RegisterMediaPipeline: {error}");
                        }
                    }
                }
            } else {
                // No src attribute and no source children — mode = none.
                // Note: networkState and delaying-the-load-event flag were already set by
                // steps 1–3 above.  Steps 6.1–6.2 (set to NETWORK_EMPTY, clear flag)
                // require access to the media element and are tracked as a gap.
            }

            Ok(job_ec.value_undefined())
        })
    }

    /// <https://html.spec.whatwg.org/#dom-media-autoplay>
    pub(crate) fn autoplay(&self) -> bool {
        self.html_element.element.has_attribute("autoplay")
    }

    /// <https://html.spec.whatwg.org/#dom-media-autoplay>
    pub(crate) fn set_autoplay(&self, value: bool) {
        if value {
            self.html_element.element.set_attribute("autoplay", "");
        } else {
            self.html_element.element.remove_attribute("autoplay");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-loop>
    pub(crate) fn loop_(&self) -> bool {
        self.html_element.element.has_attribute("loop")
    }

    /// <https://html.spec.whatwg.org/#dom-media-loop>
    pub(crate) fn set_loop(&self, value: bool) {
        if value {
            self.html_element.element.set_attribute("loop", "");
        } else {
            self.html_element.element.remove_attribute("loop");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-controls>
    pub(crate) fn controls(&self) -> bool {
        self.html_element.element.has_attribute("controls")
    }

    /// <https://html.spec.whatwg.org/#dom-media-controls>
    pub(crate) fn set_controls(&self, value: bool) {
        if value {
            self.html_element.element.set_attribute("controls", "");
        } else {
            self.html_element.element.remove_attribute("controls");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-muted>
    pub(crate) fn muted(&self) -> bool {
        self.html_element.element.has_attribute("muted")
    }

    /// <https://html.spec.whatwg.org/#dom-media-muted>
    pub(crate) fn set_muted(&self, value: bool) {
        if value {
            self.html_element.element.set_attribute("muted", "");
        } else {
            self.html_element.element.remove_attribute("muted");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-volume>
    pub(crate) fn volume(&self) -> f64 {
        1.0
    }

    /// <https://html.spec.whatwg.org/#dom-media-volume>
    pub(crate) fn set_volume(&self, _volume: f64) {}

    /// <https://html.spec.whatwg.org/#dom-media-preload>
    pub(crate) fn preload(&self) -> String {
        self.html_element
            .element
            .get_attribute("preload")
            .unwrap_or_else(|| String::from("metadata"))
    }

    /// <https://html.spec.whatwg.org/#dom-media-preload>
    pub(crate) fn set_preload(&self, value: &str) {
        self.html_element.element.set_attribute("preload", value);
    }

    /// <https://html.spec.whatwg.org/#dom-media-play>
    pub(crate) fn play(
        &mut self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<<Types as JsTypes>::JsValue, crate::js::Types> {

        // Step 1: If the media element is not allowed to play, then return
        // a promise rejected with a "NotAllowedError" DOMException.
        // Note: Simplified — always allowed to play for now.
        // Step 2: If the media element's error attribute is not null and
        // its code is MEDIA_ERR_SRC_NOT_SUPPORTED...
        // Note: Not yet implemented — error handling is simplified.

        // Step 3: Let resumptionSteps be the media element's lazy load
        // resumption steps.
        // Step 4: If resumptionSteps is not null...
        // Note: Lazy load resumption steps not yet implemented.
        // Step 5: Let promise be a new promise and append promise to the
        // list of pending play promises.
        let promise = resolved_promise(ec.value_undefined(), ec)?.into();
        // Note: The list of pending play promises is not yet tracked.

        // Step 6: Run the internal play steps for the media element.
        self.internal_play_steps(ec);

        // Step 7: Return promise.
        Ok(promise)
    }

    /// <https://html.spec.whatwg.org/#internal-play-steps>
    pub(crate) fn internal_play_steps(&mut self, ec: &mut dyn ExecutionContext<crate::js::Types>) {

        // Step 1: If the media element's networkState attribute has the
        // value NETWORK_EMPTY, invoke the media element's resource
        // selection algorithm.
        if self.network_state == Self::NETWORK_EMPTY {
            self.resource_selection_algorithm(ec);
        }

        // Step 2: If the playback has ended and the direction of playback
        // is forwards, seek to the earliest possible position.
        // Note: Not yet implemented — ended state and seeking not tracked.
        // Step 3: If the media element's paused attribute is true:
        if self.paused {

            // Step 3.1: Change the value of paused to false.
            self.paused = false;

            // Step 3.2: If the show poster flag is true, set the element's
            // show poster flag to false and run the time marches on steps.
            if self.show_poster {
                self.show_poster = false;
                // Note: time marches on steps not yet implemented.
            }

            // Step 3.3: Queue a media element task to fire an event
            // named play at the element.
            // Note: Deferred — event dispatch not yet wired.
            // Step 3.4: If readyState is HAVE_NOTHING, HAVE_METADATA,
            // or HAVE_CURRENT_DATA, queue a task to fire waiting.
            // Otherwise, notify about playing.
            // Note: Deferred — event dispatch not yet wired.
        }

        // Step 4: Otherwise (not paused), if readyState is HAVE_FUTURE_DATA
        // or HAVE_ENOUGH_DATA, resolve pending play promises.
        // Note: Not yet implemented.
        // Step 5: Set the media element's can autoplay flag to false.
        self.can_autoplay = false;
    }

    /// <https://html.spec.whatwg.org/#dom-media-pause>
    pub(crate) fn pause(&mut self, ec: &mut dyn ExecutionContext<crate::js::Types>) {

        // Step 1: If the media element's networkState attribute has the
        // value NETWORK_EMPTY, invoke the media element's resource
        // selection algorithm.
        if self.network_state == Self::NETWORK_EMPTY {
            self.resource_selection_algorithm(ec);
        }

        // Step 2: Run the internal pause steps for the media element.
        self.internal_pause_steps();
    }

    /// <https://html.spec.whatwg.org/#internal-pause-steps>
    pub(crate) fn internal_pause_steps(&mut self) {

        // Step 1: Set the media element's can autoplay flag to false.
        self.can_autoplay = false;

        // Step 2: If the media element's paused attribute is false:
        if !self.paused {

            // Step 2.1: Change the value of paused to true.
            self.paused = true;

            // Step 2.2: Take pending play promises...
            // Note: Not yet implemented.
            // Step 2.3: Queue a media element task to fire timeupdate,
            // then pause, then reject pending play promises.
            // Note: Deferred — event dispatch not yet wired.

            // Step 2.4: Set the official playback position to the
            // current playback position.
            self.official_playback_position = self.current_playback_position;
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-load>
    #[allow(dead_code)]
    pub(crate) fn load(&mut self, ec: &mut dyn ExecutionContext<crate::js::Types>) {

        // Step 1: Let resumptionSteps be the media element's lazy load resumption steps.
        // Step 2: If resumptionSteps is not null, set to null and invoke resumptionSteps.
        // Note: Lazy load resumption steps are not yet implemented — always null.
        // Step 3: Run the media element load algorithm.
        self.media_element_load_algorithm(ec);
    }
}
