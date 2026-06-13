use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::html::HTMLElement;

/// <https://html.spec.whatwg.org/#media-elements>
#[derive(Trace, Finalize, JsData)]
pub struct HTMLMediaElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,

    // --- network state ---
    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub network_state: u16,

    // --- ready state ---
    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub ready_state: u16,

    // --- src ---
    /// <https://html.spec.whatwg.org/#the-src-attribute>
    pub current_src: String,

    // --- error ---
    /// <https://html.spec.whatwg.org/#error-status>
    error: Option<MediaError>,

    // --- playback state ---
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

    // --- flags ---
    /// <https://html.spec.whatwg.org/#can-autoplay-flag>
    can_autoplay: bool,

    /// <https://html.spec.whatwg.org/#delaying-the-load-event-flag>
    delaying_the_load_event: bool,

    /// <https://html.spec.whatwg.org/#is-currently-stalled>
    is_currently_stalled: bool,

    /// <https://html.spec.whatwg.org/#show-poster-flag>
    show_poster: bool,
}

/// <https://html.spec.whatwg.org/#mediaerror>
#[derive(Trace, Finalize, JsData, Clone, Debug)]
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
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-networkstate>
    pub(crate) fn network_state(&self) -> u16 {
        // Step 1: Return the current network state of the element.
        self.network_state
    }

    /// <https://html.spec.whatwg.org/#dom-media-readystate>
    pub(crate) fn ready_state(&self) -> u16 {
        // Step 1: Return the current ready state of the element.
        self.ready_state
    }

    /// <https://html.spec.whatwg.org/#dom-media-src>
    pub(crate) fn src(&self) -> String {
        // Step 1: Return the value of the src content attribute.
        self.html_element
            .element
            .get_attribute("src")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-media-src>
    pub(crate) fn set_src(&self, src: &str) {
        // Step 1: Set this's src content attribute to the given value.
        self.html_element.element.set_attribute("src", src);
        // Step 2: Invoke the element's media element load algorithm.
        // Note: This is triggered from the binding layer after calling set_src.
    }

    /// <https://html.spec.whatwg.org/#dom-media-currentsrc>
    pub(crate) fn current_src(&self) -> String {
        // Step 1: Return the URL of the current media resource, if any.
        // Returns the empty string when there is no media resource.
        self.current_src.clone()
    }

    /// <https://html.spec.whatwg.org/#dom-media-duration>
    pub(crate) fn duration(&self) -> f64 {
        // Step 1: Return the time of the end of the media resource, in seconds.
        // If no media data is available, return NaN.
        self.duration
    }

    /// <https://html.spec.whatwg.org/#dom-media-paused>
    pub(crate) fn paused(&self) -> bool {
        // Step 1: Return whether the media element is paused.
        self.paused
    }

    /// <https://html.spec.whatwg.org/#dom-media-seeking>
    pub(crate) fn seeking(&self) -> bool {
        // Step 1: Return whether the media element is currently seeking.
        self.seeking
    }

    /// <https://html.spec.whatwg.org/#dom-media-currenttime>
    pub(crate) fn current_time(&self) -> f64 {
        // Step 1: Return the default playback start position, unless that is zero,
        // in which case return the official playback position.
        if self.default_playback_start_position != 0.0 {
            self.default_playback_start_position
        } else {
            self.official_playback_position
        }
    }

    #[allow(dead_code)]
    /// <https://html.spec.whatwg.org/#dom-media-currenttime>
    /// Note: setter stub.
    pub(crate) fn set_current_time(&mut self, time: f64) {
        // Step 1: If readyState is HAVE_NOTHING, set default playback start position.
        // Otherwise, set official playback position and seek.
        if self.ready_state == Self::HAVE_NOTHING {
            self.default_playback_start_position = time;
        } else {
            self.official_playback_position = time;
            // TODO: Send seek to media process.
        }
    }

    /// <https://html.spec.whatwg.org/#dom-media-error>
    pub(crate) fn error(&self) -> Option<MediaError> {
        self.error.clone()
    }

    // ── Media element load algorithm ──

    /// <https://html.spec.whatwg.org/#media-element-load-algorithm>
    ///
    /// Note: Steps 3–6 (pending task management, abort event) and step 8 (playbackRate)
    /// are no-ops in the initial cut. The in-parallel fetch portion is delegated to
    /// the user agent via IPC.
    #[allow(dead_code)]
    pub(crate) fn media_element_load_algorithm(&mut self) {
        // Step 1: Set this element's is currently stalled to false.
        self.is_currently_stalled = false;
        // Step 2: Abort any already-running instance of the resource selection algorithm for this
        // element.
        // Note: No-op in the initial cut — resource selection runs only once.
        // Step 3: Let pending tasks be a list of all tasks from the media element's media element
        // event task source in one of the task queues.
        // Step 4: For each task in pending tasks that would resolve pending play promises or reject
        // pending play promises, immediately resolve or reject those promises in the order the
        // corresponding tasks were queued.
        // Step 5: Remove each task in pending tasks from its task queue.
        // Note: Steps 3–5 are no-ops until promise-based play() is implemented.
        // Step 6: If networkState is NETWORK_LOADING or NETWORK_IDLE, fire an event named abort at
        // the media element.
        // Note: Deferred to event dispatch.
        // Step 7: If networkState is not NETWORK_EMPTY, then:
        if self.network_state != Self::NETWORK_EMPTY {
            // Step 7.1-4: Reset playback rate. (No-op — defaultPlaybackRate always 1.0.)
            // Step 7.5: Set the element's readyState to HAVE_NOTHING.
            self.ready_state = Self::HAVE_NOTHING;
            // Step 7.6: If paused is false, then set paused to true.
            // Note: paused is always true in the initial cut.
            // Step 7.7: Set seeking to false.
            self.seeking = false;
            // Step 7.8: Set the current playback position to 0, set the official playback
            // position to 0.
            self.current_playback_position = 0.0;
            self.official_playback_position = 0.0;
            // Step 7.9: Set the timeline offset to NaN.
            // Step 7.10: Update the duration attribute to NaN.
            self.duration = f64::NAN;
        }
        // Step 8: Set playbackRate to defaultPlaybackRate.
        // Note: No-op — defaultPlaybackRate is always 1.0 in the initial cut.
        // Step 9: Set the error attribute to null and the can autoplay flag to true.
        self.error = None;
        self.can_autoplay = true;
        // Step 10: Invoke the resource selection algorithm for the element.
        self.resource_selection_algorithm();
        // Step 11: Playback of any previously playing media resource for this element stops.
        // Note: No-op — there is no active playback in the initial cut.
    }

    /// <https://html.spec.whatwg.org/#resource-selection-algorithm>
    ///
    /// Note: Runs synchronously in the initial cut. The "await a stable state" boundary
    /// and in-parallel continuation will be added when the user agent media handler
    /// is wired up.
    #[allow(dead_code)]
    pub(crate) fn resource_selection_algorithm(&mut self) {
        // Step 1: Set networkState to NETWORK_NO_SOURCE.
        self.network_state = Self::NETWORK_NO_SOURCE;
        // Step 2: Set show poster flag to true.
        self.show_poster = true;
        // Step 3: If lazy loading is Eager or scripting is disabled, set
        // delaying-the-load-event flag to true.
        // Note: Initial cut always sets delaying-the-load-event.
        self.delaying_the_load_event = true;

        // Step 4: Await a stable state. The synchronous section consists of all remaining
        // steps of this algorithm until the step says the synchronous section has ended.

        // Step 5: ⌛ If the element's blocked-on-parser flag is false, then populate the
        // list of pending text tracks.
        // Note: No-op — no text track support yet.

        // Step 6: ⌛ Determine the mode.
        // (mode = attribute if the src attribute is present and not empty)
        let src_attr = self.html_element.element.get_attribute("src");

        if let Some(src) = src_attr.filter(|s| !s.is_empty()) {
            // Step 7: ⌛ Set networkState to NETWORK_LOADING.
            self.network_state = Self::NETWORK_LOADING;
            // Step 8: ⌛ Queue a media element task given the media element to fire an event
            // named loadstart at the media element.
            // Note: Deferred to event dispatch.
            // Step 9: ⌛ Run the appropriate steps for the mode, which depends on how the
            // media source was determined.
            // For mode = attribute: set current_src, then fetch via user agent.
            self.current_src = src;

            // TODO: Send MediaLoad event to user agent via ContentEvent IPC.
        } else {
            // No src attribute and no source children — mode = none.
            // Step 6.1: ⌛ Set networkState to NETWORK_EMPTY.
            self.network_state = Self::NETWORK_EMPTY;
            // Step 6.2: ⌛ Set delaying-the-load-event flag to false.
            self.delaying_the_load_event = false;
            // Step 6.3: End the synchronous section and return.
        }
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
        // Step 1: Return the current volume.
        // Note: Initial cut always returns 1.0; the stored volume is not yet tracked.
        1.0
    }

    /// <https://html.spec.whatwg.org/#dom-media-volume>
    pub(crate) fn set_volume(&self, _volume: f64) {
        // Step 1: If the given value is in the range 0.0 to 1.0, set the volume.
        // Note: Stub for the initial cut.
    }

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

    /// <https://html.spec.whatwg.org/#dom-media-load>
    #[allow(dead_code)]
    pub(crate) fn load(&mut self) {
        // Step 1: Let resumptionSteps be the media element's lazy load resumption steps.
        // Step 2: If resumptionSteps is not null:
        //   2.1: Set the media element's lazy load resumption steps to null.
        //   2.2: Invoke resumptionSteps.
        // Note: lazy load resumption steps are not yet implemented — always null.
        // Step 3: Run the media element load algorithm.
        self.media_element_load_algorithm();
    }
}
