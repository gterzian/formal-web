use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::html::HTMLElement;

/// <https://html.spec.whatwg.org/#media-elements>
///
/// HTMLMediaElement is the shared base for HTMLVideoElement and HTMLAudioElement.
/// This implementation covers network state, ready state, and the
/// resource selection algorithm entry points.
///
/// Note: Only the subset needed for the initial HTMLVideoElement implementation is
/// provided. Playback control (play(), pause()), time ranges, and track management
/// are stubs for future work.
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
    /// Note: Only meaningful for HTMLVideoElement; kept here for convenience.
    show_poster: bool,
}

/// <https://html.spec.whatwg.org/#mediaerror>
#[derive(Trace, Finalize, JsData, Clone, Debug)]
pub struct MediaError {
    /// <https://html.spec.whatwg.org/#mediaerror-code>
    pub code: u16,
    /// <https://html.spec.whatwg.org/#mediaerror-message>
    pub message: String,
}

impl MediaError {
    /// <https://html.spec.whatwg.org/#create-a-mediaerror>
    pub fn new(code: u16, message: String) -> Self {
        // Step 1: Return a new MediaError object whose code is the given error code
        // and whose message is a string containing any details the user agent is able
        // to supply.
        Self { code, message }
    }
}

// Network state constants.
impl HTMLMediaElement {
    pub const NETWORK_EMPTY: u16 = 0;
    pub const NETWORK_IDLE: u16 = 1;
    pub const NETWORK_LOADING: u16 = 2;
    pub const NETWORK_NO_SOURCE: u16 = 3;

    // Ready state constants.
    pub const HAVE_NOTHING: u16 = 0;
    pub const HAVE_METADATA: u16 = 1;
    pub const HAVE_CURRENT_DATA: u16 = 2;
    pub const HAVE_FUTURE_DATA: u16 = 3;
    pub const HAVE_ENOUGH_DATA: u16 = 4;

    // Error code constants.
    pub const MEDIA_ERR_ABORTED: u16 = 1;
    pub const MEDIA_ERR_NETWORK: u16 = 2;
    pub const MEDIA_ERR_DECODE: u16 = 3;
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

    /// <https://html.spec.whatwg.org/#current-playback-position>
    /// Note: combined getter for currentTime.
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

    #[allow(dead_code)]
    /// <https://html.spec.whatwg.org/#media-element-load-algorithm>
    ///
    /// Note: This implements the initial subset — aborts existing resource selection,
    /// resets state, and invokes the resource selection algorithm. The in-parallel
    /// portions (actually fetching the media resource) are delegated to the user agent
    /// via IPC.
    pub(crate) fn load(&mut self) {
        // Step 1: Set this element's is currently stalled to false.
        self.is_currently_stalled = false;
        // Step 2: Abort any already-running instance of the resource selection algorithm.
        // (No-op in the initial cut — resource selection runs only once.)
        // Step 3-5: Remove pending tasks from the media element event task source.
        // (No-op in the initial cut.)
        // Step 6: If networkState is NETWORK_LOADING or NETWORK_IDLE, fire abort event.
        // (Deferred to event dispatch.)
        // Step 7: If networkState != NETWORK_EMPTY, reset all state.
        if self.network_state != Self::NETWORK_EMPTY {
            // Step 7.5: Set readyState to HAVE_NOTHING.
            self.ready_state = Self::HAVE_NOTHING;
            // Step 7.6: If paused is false, set paused to true.
            // (Already true in initial state.)
            // Step 7.7: Set seeking to false.
            self.seeking = false;
            // Step 7.8: Set current playback position to 0, official playback position to 0.
            self.current_playback_position = 0.0;
            self.official_playback_position = 0.0;
            // Step 7.9: Set timeline offset to NaN.
            // Step 7.10: Update the duration attribute to NaN.
            self.duration = f64::NAN;
        }
        // Step 8: Set playbackRate to defaultPlaybackRate.
        // (No-op in the initial cut.)
        // Step 9: Set error to null and can autoplay flag to true.
        self.error = None;
        self.can_autoplay = true;
        // Step 10: Invoke the resource selection algorithm.
        self.resource_selection_algorithm();
        // Step 11: Playback of any previously playing media resource stops.
    }

    #[allow(dead_code)]
    /// <https://html.spec.whatwg.org/#resource-selection-algorithm>
    ///
    /// Note: In the initial cut this runs synchronously (no in-parallel step yet).
    /// Full implementation will split at "Await a stable state" and continue
    /// in parallel via the user agent's media handler.
    pub(crate) fn resource_selection_algorithm(&mut self) {
        // Step 1: Set networkState to NETWORK_NO_SOURCE.
        self.network_state = Self::NETWORK_NO_SOURCE;
        // Step 2: Set show poster flag to true.
        self.show_poster = true;
        // Step 3: If lazy loading is Eager or scripting is disabled,
        // set delaying-the-load-event flag to true.
        // Note: Initial cut always sets delaying-the-load-event.
        self.delaying_the_load_event = true;

        // Step 4: Await a stable state. The synchronous section consists of
        // all remaining steps until the algorithm says the synchronous section has ended.
        // Note: In the initial cut this runs synchronously because we don't yet
        // have a microtask queue. The in-parallel continuation will be added when
        // the user agent media handler is wired up.

        // Step 5: ⌛ If the blocked-on-parser flag is false, populate pending text tracks.
        // (No-op — no text track support yet.)

        // Step 6: ⌛ Determine mode (object, attribute, children, or none).
        let src_attr = self.html_element.element.get_attribute("src");

        if let Some(src) = src_attr.filter(|s| !s.is_empty()) {
            // Step 6: mode = attribute
            // Step 7: ⌛ Set networkState to NETWORK_LOADING.
            self.network_state = Self::NETWORK_LOADING;
            // Step 8: ⌛ Queue a loadstart event.
            // (Deferred to event dispatch.)
            // Step 9: Run appropriate steps for mode = attribute.
            // For the initial cut, set current_src and fire the load event flow.

            // Save the URL — content process sends MediaLoad to user agent
            // which creates a pipeline in the media process.
            self.current_src = src;

            // TODO: Send MediaLoad event to user agent via ContentEvent IPC.
            // This will create the media pipeline and start loading.
            // The user agent responds with MediaReady (with VideoPaintId),
            // which content stamps onto layout output as VideoEmbedSite.
        } else {
            // No src attribute and no source children.
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
        // Note: Initial cut returns 1.0 as default.
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

    #[allow(dead_code)]
    /// <https://html.spec.whatwg.org/#dom-media-load>
    pub(crate) fn load_method(&mut self) {
        // Step 1: Let resumptionSteps be lazy load resumption steps.
        // Step 2: If resumptionSteps is not null, invoke them.
        // Step 3: Run the media element load algorithm.
        self.load();
    }
}
