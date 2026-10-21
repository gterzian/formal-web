//! The HTML image processing model.
//!
//! This module owns the per-element image request state
//! (<https://html.spec.whatwg.org/#images-processing-model>) and the "update
//! the image data" algorithm that drives the element's fetches.  The state
//! is shared between the content process's DOM hooks (which observe relevant
//! mutations), the image element's IDL getters, and the event loop's fetch
//! completion handling.

use std::collections::{HashMap, VecDeque};

use blitz_dom::ElementData;
use html5ever::local_name;
use ipc_messages::content::{DocumentFetchId, DocumentId, ImageIdentifier, ResourcePartitionKey};
use url::Url;

/// <https://html.spec.whatwg.org/#img-req-state>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ImageRequestState {
    /// <https://html.spec.whatwg.org/#img-none>
    #[default]
    Unavailable,
    /// <https://html.spec.whatwg.org/#img-inc>
    PartiallyAvailable,
    /// <https://html.spec.whatwg.org/#img-all>
    CompletelyAvailable,
    /// <https://html.spec.whatwg.org/#img-error>
    Broken,
}

impl ImageRequestState {
    /// <https://html.spec.whatwg.org/#img-available>
    pub(crate) fn is_available(self) -> bool {
        matches!(self, Self::PartiallyAvailable | Self::CompletelyAvailable)
    }
}

/// <https://html.spec.whatwg.org/#image-request>
#[derive(Clone, Debug)]
pub(crate) struct ImageRequest {
    /// <https://html.spec.whatwg.org/#img-req-state>
    pub(crate) state: ImageRequestState,
    /// <https://html.spec.whatwg.org/#img-req-url>
    pub(crate) current_url: String,
    /// <https://html.spec.whatwg.org/#img-req-data>
    pub(crate) image_data: Option<LoadedImage>,
    /// <https://html.spec.whatwg.org/#current-pixel-density>
    pub(crate) current_pixel_density: f64,
    /// <https://html.spec.whatwg.org/#preferred-density-corrected-dimensions>
    pub(crate) preferred_density_corrected_dimensions: Option<(u32, u32)>,
    /// Identifies the decoded resource this request will show.
    pub(crate) image_id: Option<ImageIdentifier>,
    /// The key this request will be added to the list of available images
    /// under once it is completely available.
    pub(crate) available_images_key: Option<String>,
    /// The fetch this request is waiting for, if any.
    pub(crate) fetch_id: Option<DocumentFetchId>,
    /// Identifies this request instance: a `decode()` promise waiting on the
    /// current request rejects when the element's current request is replaced
    /// by a request with a different id.
    pub(crate) request_id: u64,
}

impl Default for ImageRequest {
    fn default() -> Self {
        Self {
            state: ImageRequestState::Unavailable,
            current_url: String::new(),
            image_data: None,
            current_pixel_density: 1.0,
            preferred_density_corrected_dimensions: None,
            image_id: None,
            available_images_key: None,
            fetch_id: None,
            request_id: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedImage {
    pub(crate) image_id: ImageIdentifier,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The image-processing state of one `<img>` element.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImageElement {
    /// <https://html.spec.whatwg.org/#current-request>
    pub(crate) current_request: ImageRequest,
    /// <https://html.spec.whatwg.org/#pending-request>
    pub(crate) pending_request: Option<ImageRequest>,
    /// <https://html.spec.whatwg.org/#last-selected-source>
    pub(crate) last_selected_source: Option<String>,
}

/// Work produced by the processing model that needs the document (and so runs
/// on the content process's event loop, not inside a DOM mutation hook).
#[derive(Clone, Debug)]
pub(crate) enum ImageTask {
    /// The work of step 7.4.5 / step 27's "prepare an image for presentation",
    /// run once the event loop can touch the document.
    PrepareForPresentation {
        document_id: DocumentId,
        node_id: usize,
        width: u32,
        height: u32,
    },
    /// The element task queued to fire `load` at the element.
    FireLoad {
        document_id: DocumentId,
        node_id: usize,
    },
    /// The element task queued to fire `error` at the element.
    FireError {
        document_id: DocumentId,
        node_id: usize,
    },
}

/// The outcome of decoding one image resource, as reported by the graphics
/// process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageDecode {
    Decoded,
    Failed,
}

/// What `decode()` does with a promise waiting on a request (step 2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecodeSettlement {
    Wait,
    Resolve,
    Reject,
}

/// The fetch the microtask queued at step 8 of "update the image data"
/// starts, once the algorithm reached a stable state.
pub(crate) struct PendingImageFetch {
    pub(crate) document_id: DocumentId,
    pub(crate) node_id: usize,
    pub(crate) handler_id: DocumentFetchId,
    pub(crate) url: String,
}

/// The image-processing state of the content process, shared by every
/// document it hosts.
#[derive(Default)]
pub(crate) struct ImageRegistry {
    /// <https://html.spec.whatwg.org/#images-processing-model>
    pub(crate) elements: HashMap<(DocumentId, usize), ImageElement>,
    /// <https://html.spec.whatwg.org/#list-of-available-images>
    pub(crate) available_images: HashMap<(DocumentId, String), LoadedImage>,
    /// Identifies the next fetched image resource within this process.
    pub(crate) next_blob_id: u64,
    /// Identifies the next image request instance within this process.
    pub(crate) next_request_id: u64,
    /// The decode outcome of each image resource the graphics process has
    /// reported.  A request whose image id is absent is still being decoded.
    pub(crate) image_decodes: HashMap<ImageIdentifier, ImageDecode>,
    /// The fetches queued by the microtask at step 8, in the order their
    /// instances queued them.  Drained at a microtask checkpoint.
    pub(crate) pending_microtasks: VecDeque<PendingImageFetch>,
    /// Element tasks the algorithms queued, drained by `ContentProcess`.
    pub(crate) pending_tasks: Vec<ImageTask>,
}

pub(crate) trait ImageFetchSink {
    fn start_image_fetch(
        &self,
        document_id: DocumentId,
        node_id: usize,
        handler_id: DocumentFetchId,
        url: &str,
    );
}

/// The `<img>` attributes relevant to the processing model.
pub(crate) struct ImageAttributes {
    pub(crate) src: Option<String>,
    pub(crate) srcset: Option<String>,
    pub(crate) crossorigin: Option<String>,
}

impl ImageAttributes {
    pub(crate) fn from_element(element: &ElementData) -> Self {
        Self {
            src: element
                .attr(local_name!("src"))
                .map(|value| value.to_string()),
            srcset: element
                .attr(local_name!("srcset"))
                .map(|value| value.to_string()),
            crossorigin: element
                .attr(local_name!("crossorigin"))
                .map(|value| value.to_string()),
        }
    }

    /// <https://html.spec.whatwg.org/#use-srcset-or-picture>
    fn uses_srcset_or_picture(&self) -> bool {
        // Note: a parent `<picture>` element is not tracked; only the
        // srcset attribute is observed.
        self.srcset.is_some()
    }

    /// <https://html.spec.whatwg.org/#attr-img-crossorigin>
    fn cors_settings_mode(&self) -> &'static str {
        cors_settings_mode(self.crossorigin.as_deref())
    }
}

/// The CORS settings attribute mode of a `crossorigin` attribute value.
/// <https://html.spec.whatwg.org/#cors-settings-attribute>
pub(crate) fn cors_settings_mode(value: Option<&str>) -> &'static str {
    match value {
        None => "no-cors",
        Some(value) if value.eq_ignore_ascii_case("use-credentials") => "use-credentials",
        // Note: the attribute's empty value default and invalid value default
        // are both the Anonymous state.
        Some(_) => "anonymous",
    }
}

/// <https://html.spec.whatwg.org/#referrer-policy-attribute>
pub(crate) fn referrer_policy_state(value: Option<&str>) -> &'static str {
    const STATES: [&str; 8] = [
        "no-referrer",
        "no-referrer-when-downgrade",
        "same-origin",
        "origin",
        "strict-origin",
        "origin-when-cross-origin",
        "strict-origin-when-cross-origin",
        "unsafe-url",
    ];
    let Some(value) = value else {
        // The missing value default is the empty string state.
        return "";
    };
    // An enumerated attribute's keywords match ASCII case-insensitively; the
    // invalid value default is also the empty string state.
    STATES
        .into_iter()
        .find(|state| value.eq_ignore_ascii_case(state))
        .unwrap_or("")
}

impl ImageRegistry {
    /// The processing model state for one element, created on first use.
    pub(crate) fn element(&self, document_id: DocumentId, node_id: usize) -> ImageElement {
        self.elements
            .get(&(document_id, node_id))
            .cloned()
            .unwrap_or_default()
    }

    fn element_mut(&mut self, document_id: DocumentId, node_id: usize) -> &mut ImageElement {
        self.elements.entry((document_id, node_id)).or_default()
    }

    pub(crate) fn take_pending_tasks(&mut self) -> Vec<ImageTask> {
        std::mem::take(&mut self.pending_tasks)
    }

    /// Forget a destroyed document's element state, including its list of
    /// available images: the list belongs to the Document that owned it.
    pub(crate) fn forget_document(&mut self, document_id: DocumentId) {
        self.elements
            .retain(|(element_document_id, _), _| *element_document_id != document_id);
        self.available_images
            .retain(|(image_document_id, _), _| *image_document_id != document_id);
        self.pending_microtasks
            .retain(|update| update.document_id != document_id);
        self.pending_tasks.retain(|task| match task {
            ImageTask::PrepareForPresentation {
                document_id: task_document_id,
                ..
            }
            | ImageTask::FireLoad {
                document_id: task_document_id,
                ..
            }
            | ImageTask::FireError {
                document_id: task_document_id,
                ..
            } => *task_document_id != document_id,
        });
    }
}

/// The result of encoding-parsing-and-serializing an image source.
/// <https://html.spec.whatwg.org/#encoding-parsing-and-serializing-a-url>
fn encoding_parsing_and_serializing_url(base_url: Option<&Url>, source: &str) -> Option<String> {
    let url = match base_url {
        Some(base_url) => base_url.join(source).ok()?,
        None => Url::parse(source).ok()?,
    };
    Some(url.to_string())
}

/// The key an image request would be added to the list of available images
/// under.
/// <https://html.spec.whatwg.org/#list-of-available-images>
fn available_images_key(url: &str, attributes: &ImageAttributes, origin: &str) -> String {
    let mode = attributes.cors_settings_mode();
    if mode == "no-cors" {
        format!("{url}\u{0}{mode}")
    } else {
        format!("{url}\u{0}{mode}\u{0}{origin}")
    }
}

/// <https://html.spec.whatwg.org/#update-the-image-data>
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_the_image_data(
    registry: &mut ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    attributes: &ImageAttributes,
    base_url: Option<&Url>,
    partition: &ResourcePartitionKey,
    origin: &str,
) {
    // Step 1: If `element`'s node document is not fully active:
    // Note: The content process only runs this algorithm for an active
    // document, so the step and its substeps do not apply.
    // Step 2: If the user agent cannot support images, or its support for images has been disabled, then
    // Note: This user agent supports images.

    // Step 3: Let `previousURL` be the current request's current URL.
    // Note: `previousURL` is compared against only when `maybe omit events`
    // is set, and it never is (the flag is set only by the auto-sizes
    // mutation, which is not implemented), so it is not tracked.
    let element = registry.element_mut(document_id, node_id);

    // Step 4: Let `selected source` be null and `selected pixel density` be undefined.
    let mut selected_source: Option<String> = None;
    let mut selected_pixel_density: Option<f64> = None;

    // Step 5: If `element` does not use `srcset` or `picture` and it has a `src` attribute specified whose value is not the empty string, then set `selected source` to the value of `element`'s `src` attribute and set `selected pixel density` to 1.0.
    if !attributes.uses_srcset_or_picture()
        && let Some(src) = attributes.src.as_deref()
        && !src.is_empty()
    {
        selected_source = Some(src.to_string());
        selected_pixel_density = Some(1.0);
    }

    // Step 6: Set `element`'s last selected source to `selected source`.
    element.last_selected_source = selected_source.clone();

    // Step 7: If `selected source` is not null:
    if let Some(selected_source) = selected_source.as_deref() {
        // Step 7.1: Let `urlString` be the result of encoding-parsing-and-serializing a URL given `selected source`, relative to `element`'s node document.
        let url_string = encoding_parsing_and_serializing_url(base_url, selected_source);
        // Step 7.2: If `urlString` is failure, then abort this inner set of steps.
        if let Some(url_string) = url_string {
            // Step 7.3: Let `key` be a tuple consisting of `urlString`, `element`'s `crossorigin` attribute's mode, and, if that mode is not No CORS, `element`'s node document's origin.
            let key = available_images_key(&url_string, attributes, origin);

            // Step 7.4: If the list of available images contains an entry for `key`:
            if let Some(available) = registry
                .available_images
                .get(&(document_id, key.clone()))
                .cloned()
            {
                let request_id = next_request_id(registry);
                let element = registry.element_mut(document_id, node_id);
                // Step 7.4.1: Set the ignore higher-layer caching flag for that entry.
                // Note: Higher-layer caching is not modeled.
                // Step 7.4.2: Abort the image request for the current request and the pending request.
                element.pending_request = None;
                // Step 7.4.3: Set the pending request to null.
                // Step 7.4.4: Set the current request to a new image request whose image data is that of the entry and whose state is completely available.
                element.current_request = ImageRequest {
                    state: ImageRequestState::CompletelyAvailable,
                    current_url: url_string.clone(),
                    image_data: Some(available.clone()),
                    current_pixel_density: 1.0,
                    preferred_density_corrected_dimensions: None,
                    image_id: Some(available.image_id.clone()),
                    available_images_key: Some(key),
                    fetch_id: None,
                    request_id,
                };
                // Step 7.4.5: Prepare the current request for presentation given `element`.
                registry
                    .pending_tasks
                    .push(ImageTask::PrepareForPresentation {
                        document_id,
                        node_id,
                        width: available.width,
                        height: available.height,
                    });
                // Step 7.4.6: Set the current request's current pixel density to `selected pixel density`.
                if let Some(density) = selected_pixel_density {
                    registry
                        .element_mut(document_id, node_id)
                        .current_request
                        .current_pixel_density = density;
                }
                // Step 7.4.7: Queue an element task on the DOM manipulation task source given `element` and the following steps:
                // Step 7.4.7.1: If `restart animation` is set, then restart the animation.
                // Note: Image animation is not implemented.
                // Step 7.4.7.2: Set the current request's current URL to `urlString`.
                // Note: Handled by step 7.4.4 above.
                // Step 7.4.7.3: If `maybe omit events` is not set or `previousURL` is not equal to `urlString`, then fire an event named `load` at `element`.
                // Note: `maybe omit events` is never set, so the event always fires.
                registry.pending_tasks.push(ImageTask::FireLoad {
                    document_id,
                    node_id,
                });
                // Step 7.4.8: Abort the update the image data algorithm.
                return;
            }
        }
    }

    // Step 10: Let `selected source` and `selected pixel density` be the URL and pixel density that results from selecting an image source, respectively.
    // Note: The source set is selected by `select_an_image_source`: the first
    // `srcset` candidate, or `src` when the source set yields nothing.  A
    // `sizes`-driven choice is not modeled.
    let selected_source = select_an_image_source(attributes);
    // Step 11: If `selected source` is null:
    let Some(selected_source) = selected_source else {
        // Step 11.1: Set the current request's state to `broken`, abort the image request for the current request and the pending request, and set the pending request to null.
        let element = registry.element_mut(document_id, node_id);
        element.current_request.state = ImageRequestState::Broken;
        element.current_request.image_data = None;
        element.current_request.fetch_id = None;
        element.pending_request = None;
        // Step 11.2: Queue an element task on the DOM manipulation task source given `element` and the following steps:
        // Step 11.2.1: Change the current request's current URL to the empty string.
        element.current_request.current_url = String::new();
        // Step 11.2.2: If all of the following are true: `element` has a `src` attribute or it uses `srcset` or `picture`; and `maybe omit events` is not set or `previousURL` is not the empty string, then fire an event named `error` at `element`.
        // Note: `maybe omit events` is never set, so the second condition
        // always holds.
        if attributes.src.is_some() || attributes.uses_srcset_or_picture() {
            registry.pending_tasks.push(ImageTask::FireError {
                document_id,
                node_id,
            });
        }
        // Step 11.3: Return.
        return;
    };

    // Step 12: Let `urlString` be the result of encoding-parsing-and-serializing a URL given `selected source`, relative to `element`'s node document.
    let Some(url_string) = encoding_parsing_and_serializing_url(base_url, &selected_source) else {
        // Step 13: If `urlString` is failure:
        // Step 13.1: Abort the image request for the current request and the pending request.
        // Step 13.2: Set the current request's state to `broken`.
        // Step 13.3: Set the pending request to null.
        // Step 13.4: Queue an element task on the DOM manipulation task source given `element` and the following steps:
        // Step 13.4.1: Change the current request's current URL to `selected source`.
        // Step 13.4.2: If `maybe omit events` is not set or `previousURL` is not equal to `selected source`, then fire an event named `error` at `element`.
        // Note: `maybe omit events` is never set, so the condition always
        // holds.
        abort_and_fire_error(registry, document_id, node_id, attributes, selected_source);

        // Step 13.5: Return.
        return;
    };

    let blob_id = registry.next_blob_id;
    registry.next_blob_id += 1;
    let request_id = next_request_id(registry);
    let element = registry.element_mut(document_id, node_id);
    // Step 14: If the pending request is not null and `urlString` is the same as the pending request's current URL, then return.
    if element
        .pending_request
        .as_ref()
        .is_some_and(|pending| pending.current_url == url_string)
    {
        return;
    }

    // Step 15: If `urlString` is the same as the current request's current URL and the current request's state is `partially available`:
    if element.current_request.current_url == url_string
        && element.current_request.state == ImageRequestState::PartiallyAvailable
    {
        // Step 15.1: Abort the image request for the pending request.
        element.pending_request = None;
        // Step 15.2: If `restart animation` is set, then queue an element task on the DOM manipulation task source given `element` to restart the animation.
        // Note: Image animation is not implemented.
        // Step 15.3: Return.
        return;
    }

    // Step 16: Abort the image request for the pending request.
    element.pending_request = None;

    // Step 17: Set `image request` to a new image request whose current URL is `urlString`.
    let image_id = ImageIdentifier {
        partition: partition.clone(),
        blob_id,
    };
    let key = available_images_key(&url_string, attributes, origin);
    let mut image_request = ImageRequest {
        state: ImageRequestState::Unavailable,
        current_url: url_string.clone(),
        image_data: None,
        current_pixel_density: 1.0,
        preferred_density_corrected_dimensions: None,
        image_id: Some(image_id.clone()),
        available_images_key: Some(key),
        fetch_id: None,
        request_id,
    };

    // Step 18: If the current request's state is `unavailable` or `broken`, then set the current request to `image request`. Otherwise, set the pending request to `image request`.
    let current_is_unavailable = matches!(
        element.current_request.state,
        ImageRequestState::Unavailable | ImageRequestState::Broken
    );
    let handler_id = DocumentFetchId::new();
    image_request.fetch_id = Some(handler_id);
    if current_is_unavailable {
        element.current_request = image_request;
    } else {
        element.pending_request = Some(image_request);
    }

    // Step 8: Queue a microtask to perform the rest of this algorithm, allowing the task that invoked this algorithm to continue.
    // Note: Steps 10 through 18 ran synchronously above, so `complete` and
    // `currentSrc` read the new request before the next microtask checkpoint;
    // the queued microtask performs step 9 and steps 19 through 26.
    registry.pending_microtasks.push_back(PendingImageFetch {
        document_id,
        node_id,
        handler_id,
        url: url_string,
    });
}

/// Allocates the next image request instance id.
fn next_request_id(registry: &mut ImageRegistry) -> u64 {
    registry.next_request_id += 1;
    registry.next_request_id
}

/// <https://html.spec.whatwg.org/#select-an-image-source>
fn select_an_image_source(attributes: &ImageAttributes) -> Option<String> {
    // Note: The source set's `sizes` descriptor is not evaluated; the first
    // candidate of the `srcset` attribute is chosen, and the `src` attribute
    // is the fallback.
    if let Some(srcset) = attributes.srcset.as_deref() {
        for candidate in srcset.split(',') {
            let first_token = candidate.split_ascii_whitespace().next();
            if let Some(token) = first_token {
                let token = token.trim_end_matches(',');
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }
    attributes.src.clone().filter(|src| !src.is_empty())
}

/// <https://html.spec.whatwg.org/#update-the-image-data>
pub(crate) fn continue_update_the_image_data(
    registry: &mut ImageRegistry,
    sink: &dyn ImageFetchSink,
    fetch: &PendingImageFetch,
) {
    let document_id = fetch.document_id;
    let node_id = fetch.node_id;
    let handler_id = fetch.handler_id;
    // Step 9: If another instance of this algorithm for `element` was started after this instance (even if it aborted and is no longer running), then return.
    // Note: The instance takes effect only while the request it installed is
    // still the element's; a later instance that replaced the request (or
    // aborted it) discards this one.
    let request_is_installed = {
        let element = registry.element(document_id, node_id);
        element.current_request.fetch_id == Some(handler_id)
            || element
                .pending_request
                .as_ref()
                .is_some_and(|pending| pending.fetch_id == Some(handler_id))
    };
    if !request_is_installed {
        return;
    }
    // Step 19: Let `request` be the result of creating a potential-CORS request given `urlString`, "image", and the current state of `element`'s `crossorigin` content attribute.
    // Step 20: Set `request`'s client to `element`'s node document's relevant settings object.
    // Step 21: If `element` uses `srcset` or `picture`, set `request`'s initiator to "imageset".
    // Step 22: Set `request`'s referrer policy to the current state of `element`'s `referrerpolicy` attribute.
    // Step 23: Set `request`'s priority to the current state of `element`'s `fetchpriority` attribute.
    // Note: The fetch request carries the URL; CORS mode, referrer policy,
    // and priority are not distinguished by the net process yet.
    // Step 24: Let `delay load event` be true if the `img`'s lazy loading attribute is in the `Eager` state, or if scripting is disabled for the `img`, and false otherwise.
    // Note: Lazy loading is not implemented; `delay load event` is true.
    // Step 25: If the will lazy load element steps given the `img` return true:
    // Note: Not implemented; images are obtained immediately.
    // Step 26: Fetch the image: Fetch `request`. Return from this algorithm, and run the remaining steps as part of the fetch's processResponse for the response `response`.
    // Note: The request was created and installed by steps 17 and 18, before
    // this microtask ran.
    sink.start_image_fetch(document_id, node_id, handler_id, &fetch.url);
}

/// <https://html.spec.whatwg.org/#update-the-image-data>
pub(crate) fn image_fetch_succeeded(
    registry: &mut ImageRegistry,
    handler_id: DocumentFetchId,
    dimensions: (u32, u32),
) {
    let Some((document_id, node_id)) = find_fetch(registry, handler_id) else {
        return;
    };
    let element = registry.element_mut(document_id, node_id);
    let mut request = match element
        .pending_request
        .take_if(|pending| pending.fetch_id == Some(handler_id))
    {
        Some(pending) => {
            // Step 27 (the last network task) 1: "If `image request` is the pending request, abort the image request for the current request, upgrade the pending request to the current request, and prepare `image request` for presentation given `element`."
            element.current_request = pending;
            element.current_request.fetch_id = None;
            element.current_request.clone()
        }
        None => {
            let current = &mut element.current_request;
            current.fetch_id = None;
            current.clone()
        }
    };
    let Some(image_id) = request.image_id.clone() else {
        return;
    };
    let loaded = LoadedImage {
        image_id: image_id.clone(),
        width: dimensions.0,
        height: dimensions.1,
    };
    // Step 27 (the next network task), "If the resource type and data corresponds to a supported image format": "If the user agent is able to determine `image request`'s image's width and height, and `image request` is the pending request, set `image request`'s state to `partially available`."
    // Step 27 (the next network task), second entry: "Otherwise, if the user agent is able to determine `image request`'s image's width and height, and `image request` is the current request, prepare `image request` for presentation given `element` and set `image request`'s state to `partially available`."
    // Step 27 (the last network task) 2: "Set `image request` to the `completely available` state."
    // Note: The response body arrives whole, so the request is prepared for
    // presentation and set to completely available in the same task; the
    // partially available state is never presented on its own.
    request.state = ImageRequestState::CompletelyAvailable;
    request.image_data = Some(loaded.clone());
    request.preferred_density_corrected_dimensions = Some(dimensions);
    let key = request.available_images_key.clone();
    element.current_request = request;

    // Step 27 (the last network task) 3: "Add the image to the list of available images using the key `key`, with the ignore higher-layer caching flag set."
    if let Some(key) = key {
        registry
            .available_images
            .insert((document_id, key), loaded.clone());
    }
    registry
        .pending_tasks
        .push(ImageTask::PrepareForPresentation {
            document_id,
            node_id,
            width: dimensions.0,
            height: dimensions.1,
        });
    // Step 27 (the last network task) 4: "If `maybe omit events` is not set or `previousURL` is not equal to `urlString`, then fire an event named `load` at `element`."
    // Note: `maybe omit events` is never set, so the event always fires.
    registry.pending_tasks.push(ImageTask::FireLoad {
        document_id,
        node_id,
    });
}

/// <https://html.spec.whatwg.org/#update-the-image-data>
pub(crate) fn image_fetch_failed(registry: &mut ImageRegistry, handler_id: DocumentFetchId) {
    // Step 27 (Otherwise): "The image data is not in a supported file format; the user agent must set `image request`'s state to `broken`, abort the image request for the current request and the pending request, upgrade the pending request to the current request if `image request` is the pending request, and then, if `maybe omit events` is not set or `previousURL` is not equal to `urlString`, queue an element task on the DOM manipulation task source given `element` to fire an event named `error` at `element`."
    // Note: `maybe omit events` is never set, so the event task is queued
    // unconditionally below.
    let Some((document_id, node_id)) = find_fetch(registry, handler_id) else {
        return;
    };
    let element = registry.element_mut(document_id, node_id);
    if element
        .pending_request
        .as_ref()
        .is_some_and(|pending| pending.fetch_id == Some(handler_id))
    {
        element.current_request = element.pending_request.take().unwrap_or_default();
    }
    // Step 27 (Otherwise) continues: the pending request is aborted (there is
    // no fetch to cancel beyond dropping its record).
    element.pending_request = None;
    element.current_request.state = ImageRequestState::Broken;
    element.current_request.image_data = None;
    element.current_request.fetch_id = None;
    registry.pending_tasks.push(ImageTask::FireError {
        document_id,
        node_id,
    });
}

fn find_fetch(
    registry: &ImageRegistry,
    handler_id: DocumentFetchId,
) -> Option<(DocumentId, usize)> {
    registry
        .elements
        .iter()
        .find_map(|((document_id, node_id), element)| {
            if element.current_request.fetch_id == Some(handler_id)
                || element
                    .pending_request
                    .as_ref()
                    .is_some_and(|pending| pending.fetch_id == Some(handler_id))
            {
                Some((*document_id, *node_id))
            } else {
                None
            }
        })
}

/// The image-processing state of an `<img>` element, if the element has been
/// seen by the processing model.
pub(crate) fn image_element(
    registry: &ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
) -> ImageElement {
    registry.element(document_id, node_id)
}

/// The natural dimensions of the element's current request.
/// <https://html.spec.whatwg.org/#density-corrected-intrinsic-width-and-height>
pub(crate) fn natural_dimensions(element: &ImageElement) -> Option<(u32, u32)> {
    // Step 1: Let `density` be `img`'s current request's current pixel density.
    let density = element.current_request.current_pixel_density;
    // Step 2: Let `dimensions` be `img`'s current request's preferred density-corrected dimensions.
    // Step 3: If `dimensions` is not null, then set `dimensions`'s width to `dimensions`'s width divided by `density`, set `dimensions`'s height to `dimensions`'s height divided by `density`, and return `dimensions`.
    if let Some((width, height)) = element
        .current_request
        .preferred_density_corrected_dimensions
    {
        return Some((
            (f64::from(width) / density).round() as u32,
            (f64::from(height) / density).round() as u32,
        ));
    }
    // Step 4: Let `intrinsicWidth`, `intrinsicHeight`, and `intrinsicRatio` be `img`'s intrinsic width, intrinsic height, and intrinsic aspect ratio, if any, respectively.
    let image = element.current_request.image_data.as_ref()?;
    // Step 5: If `intrinsicWidth` is not absent, then set `intrinsicWidth` to `intrinsicWidth` divided by `density`.
    // Step 6: If `intrinsicHeight` is not absent, then set `intrinsicHeight` to `intrinsicHeight` divided by `density`.
    // Step 7: Return the result of applying the default sizing algorithm with `intrinsicWidth`, `intrinsicHeight`, and `intrinsicRatio`, using a contain constraint whose width and height are `img`'s concrete object size's width and height, respectively.
    // Note: The default sizing algorithm is not modeled; the density-corrected
    // intrinsic dimensions are returned directly.
    Some((
        (f64::from(image.width) / density).round() as u32,
        (f64::from(image.height) / density).round() as u32,
    ))
}

/// Whether the element provides an available paint source.
/// <https://html.spec.whatwg.org/#img-available>
pub(crate) fn is_available(element: &ImageElement) -> bool {
    element.current_request.state.is_available()
}

/// The identifier of the currently displayed image, if available.
pub(crate) fn current_image_id(element: &ImageElement) -> Option<ImageIdentifier> {
    element
        .current_request
        .image_data
        .as_ref()
        .map(|image| image.image_id.clone())
}

/// The identity of the element's current request, as `decode()` records it.
pub(crate) fn current_request_id(element: &ImageElement) -> u64 {
    element.current_request.request_id
}

/// Records the graphics process's decode outcome for one image resource.
pub(crate) fn image_decoded(
    registry: &mut ImageRegistry,
    image_id: &ImageIdentifier,
    decoded: bool,
) {
    registry.image_decodes.insert(
        image_id.clone(),
        if decoded {
            ImageDecode::Decoded
        } else {
            ImageDecode::Failed
        },
    );
}

/// The decision `decode()` step 2.3 makes for a promise waiting on the request
/// the element's current request had when the promise was registered.
pub(crate) fn decode_settlement(
    registry: &ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    request_id: u64,
) -> DecodeSettlement {
    let element = registry.element(document_id, node_id);
    let request = &element.current_request;
    // "This img element's current request changes or is mutated"
    if request.request_id != request_id {
        return DecodeSettlement::Reject;
    }
    match request.state {
        ImageRequestState::Broken => DecodeSettlement::Reject,
        ImageRequestState::CompletelyAvailable => {
            let decode = request
                .image_id
                .as_ref()
                .and_then(|image_id| registry.image_decodes.get(image_id));
            match decode {
                Some(ImageDecode::Decoded) => DecodeSettlement::Resolve,
                Some(ImageDecode::Failed) => DecodeSettlement::Reject,
                None => DecodeSettlement::Wait,
            }
        }
        ImageRequestState::Unavailable | ImageRequestState::PartiallyAvailable => {
            DecodeSettlement::Wait
        }
    }
}

/// Whether `decode()` step 2.2 rejects the promise: the current request is
/// broken, or the processing model never installed a request for the element.
/// No source leaves the request broken when "update the image data" runs, and
/// an element that never reached the model (a detached `<img>` without `src`
/// or `srcset`) is indistinguishable from it here.
pub(crate) fn decode_rejects_now(
    registry: &ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
) -> bool {
    let element = registry.element(document_id, node_id);
    element.current_request.state == ImageRequestState::Broken
        || element.current_request.request_id == 0
}

// Note: This helper merges the shared tail of steps 13.1-13.4 (the state and
// event handling the URL-parse-failure path performs).
fn abort_and_fire_error(
    registry: &mut ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    attributes: &ImageAttributes,
    current_url: String,
) {
    let element = registry.element_mut(document_id, node_id);
    element.current_request.state = ImageRequestState::Broken;
    element.current_request.image_data = None;
    element.current_request.fetch_id = None;
    element.current_request.current_url = current_url;
    element.pending_request = None;
    if attributes.src.is_some() || attributes.uses_srcset_or_picture() {
        registry.pending_tasks.push(ImageTask::FireError {
            document_id,
            node_id,
        });
    }
}
