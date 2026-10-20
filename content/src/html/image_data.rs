//! The HTML image processing model.
//!
//! This module owns the per-element image request state
//! (<https://html.spec.whatwg.org/#images-processing-model>) and the "update
//! the image data" algorithm that drives the element's fetches.  The state
//! is shared between the content process's DOM hooks (which observe relevant
//! mutations), the image element's IDL getters, and the event loop's fetch
//! completion handling.

use std::collections::HashMap;

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
        match self.crossorigin.as_deref() {
            Some(value) if value.eq_ignore_ascii_case("use-credentials") => "use-credentials",
            Some(value) if value.eq_ignore_ascii_case("anonymous") => "anonymous",
            _ => "no-cors",
        }
    }
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

    /// Forget a destroyed document's element state.  The available-images
    /// entries are kept: a later document in the same partition can reuse
    /// them without a fetch.
    pub(crate) fn forget_document(&mut self, document_id: DocumentId) {
        self.elements
            .retain(|(element_document_id, _), _| *element_document_id != document_id);
        self.available_images
            .retain(|(image_document_id, _), _| *image_document_id != document_id);
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
    sink: &dyn ImageFetchSink,
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
    let element = registry.element_mut(document_id, node_id);
    let previous_url = element.current_request.current_url.clone();

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
    if let Some(selected_source) = selected_source.clone() {
        // Step 7.1: Let `urlString` be the result of encoding-parsing-and-serializing a URL given `selected source`, relative to `element`'s node document.
        let Some(url_string) = encoding_parsing_and_serializing_url(base_url, &selected_source)
        else {
            // Step 7.2: If `urlString` is failure, then abort this inner set of steps.
            run_after_selected_source(
                registry,
                sink,
                document_id,
                node_id,
                attributes,
                base_url,
                partition,
                origin,
                selected_source,
                previous_url,
            );
            return;
        };

        // Step 7.3: Let `key` be a tuple consisting of `urlString`, `element`'s `crossorigin` attribute's mode, and, if that mode is not No CORS, `element`'s node document's origin.
        let key = available_images_key(&url_string, attributes, origin);

        // Step 7.4: If the list of available images contains an entry for `key`:
        if let Some(available) = registry
            .available_images
            .get(&(document_id, key.clone()))
            .cloned()
        {
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
            // Note: The `maybe omit events` flag is never set.
            if previous_url != url_string {
                registry.pending_tasks.push(ImageTask::FireLoad {
                    document_id,
                    node_id,
                });
            }
            // Step 7.4.8: Abort the update the image data algorithm.
            return;
        }

        // Step 8: Queue a microtask to perform the rest of this algorithm, allowing the task that invoked this algorithm to continue.
        // Note: The microtask boundary is not modeled: the remaining steps
        // run synchronously in the mutation hook, and the fetch itself is
        // asynchronous.
        // Step 9: If another instance of this algorithm for `element` was started after this instance (even if it aborted and is no longer running), then return.
        // Note: The algorithm runs synchronously from the mutation hook, so
        // there is no interleaving instance to discard.
        // Step 10: Let `selected source` and `selected pixel density` be the URL and pixel density that results from selecting an image source, respectively.
        // Note: `selected source` was selected in step 5; selecting from a
        // source set is not modeled here.
        // Step 11: If `selected source` is null:
        // Note: `selected source` is non-null in this branch.
        run_after_selected_source(
            registry,
            sink,
            document_id,
            node_id,
            attributes,
            base_url,
            partition,
            origin,
            selected_source,
            previous_url,
        );
    } else {
        // Step 9: If another instance of this algorithm for `element` was started after this instance (even if it aborted and is no longer running), then return.
        // Note: The algorithm runs synchronously from the mutation hook, so
        // there is no interleaving instance to discard.
        // Step 10: Let `selected source` and `selected pixel density` be the URL and pixel density that results from selecting an image source, respectively.
        // Note: The source set is selected by `select_an_image_source`
        // below: the first `srcset` candidate, or `src` when the source set
        // yields nothing.  A `sizes`-driven choice is not modeled.
        let selected_source = select_an_image_source(attributes);
        if let Some(selected_source) = selected_source {
            run_after_selected_source(
                registry,
                sink,
                document_id,
                node_id,
                attributes,
                base_url,
                partition,
                origin,
                selected_source,
                previous_url,
            );
            return;
        }

        // Step 11: If `selected source` is null:
        let element = registry.element_mut(document_id, node_id);
        // Step 11.1: Set the current request's state to `broken`, abort the image request for the current request and the pending request, and set the pending request to null.
        element.current_request.state = ImageRequestState::Broken;
        element.current_request.image_data = None;
        element.pending_request = None;
        // Step 11.2: Queue an element task on the DOM manipulation task source given `element` and the following steps:
        // Step 11.2.1: Change the current request's current URL to the empty string.
        element.current_request.current_url = String::new();
        // Step 11.2.2: If all of the following are true: `element` has a `src` attribute or it uses `srcset` or `picture`; and `maybe omit events` is not set or `previousURL` is not the empty string, then fire an event named `error` at `element`.
        let has_src = attributes.src.is_some();
        if (has_src || attributes.uses_srcset_or_picture()) && !previous_url.is_empty() {
            registry.pending_tasks.push(ImageTask::FireError {
                document_id,
                node_id,
            });
        }
        // Step 11.3: Return.
    }
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

// Note: This helper holds steps 9 through 26 of the same algorithm; it is
// split out only so the step 7 fast path can return early.
#[allow(clippy::too_many_arguments)]
fn run_after_selected_source(
    registry: &mut ImageRegistry,
    sink: &dyn ImageFetchSink,
    document_id: DocumentId,
    node_id: usize,
    attributes: &ImageAttributes,
    base_url: Option<&Url>,
    partition: &ResourcePartitionKey,
    origin: &str,
    selected_source: String,
    previous_url: String,
) {
    // Step 12: Let `urlString` be the result of encoding-parsing-and-serializing a URL given `selected source`, relative to `element`'s node document.
    let Some(url_string) = encoding_parsing_and_serializing_url(base_url, &selected_source) else {
        // Step 13: If `urlString` is failure:
        // Step 13.1: Abort the image request for the current request and the pending request.
        // Step 13.2: Set the current request's state to `broken`.
        // Step 13.3: Set the pending request to null.
        // Step 13.4: Queue an element task on the DOM manipulation task source given `element` and the following steps:
        // Step 13.4.1: Change the current request's current URL to `selected source`.
        // Step 13.4.2: If `maybe omit events` is not set or `previousURL` is not equal to `selected source`, then fire an event named `error` at `element`.
        // Step 13.5: Return.
        abort_and_fire_error(
            registry,
            document_id,
            node_id,
            attributes,
            previous_url,
            selected_source,
        );
        return;
    };

    let blob_id = registry.next_blob_id;
    registry.next_blob_id += 1;
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
    sink.start_image_fetch(document_id, node_id, handler_id, &url_string);
}

/// Step 27 of "update the image data" for a response in a supported image
/// format, once the image's dimensions are known.
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
            // Step 27 (last task) 1: If `image request` is the pending request, abort the image request for the current request, upgrade the pending request to the current request, and prepare `image request` for presentation given `element`.
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
    // Step 27 (the next networking task): If the user agent is able to determine `image request`'s image's width and height, set `image request`'s state to `partially available`.
    // Step 27 (the last networking task) 2: Set `image request` to the `completely available` state.
    request.state = ImageRequestState::CompletelyAvailable;
    request.image_data = Some(loaded.clone());
    request.preferred_density_corrected_dimensions = Some(dimensions);
    let key = request.available_images_key.clone();
    element.current_request = request;

    // Step 27 (the last networking task) 3: Add the image to the list of available images using the key `key`, with the ignore higher-layer caching flag set.
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
    // Step 27 (the last networking task) 4: If `maybe omit events` is not set or `previousURL` is not equal to `urlString`, then fire an event named `load` at `element`.
    // Note: `maybe omit events` is never set.
    registry.pending_tasks.push(ImageTask::FireLoad {
        document_id,
        node_id,
    });
}

/// Step 27's "Otherwise" entry: the image data is not in a supported format.
/// <https://html.spec.whatwg.org/#update-the-image-data>
pub(crate) fn image_fetch_failed(registry: &mut ImageRegistry, handler_id: DocumentFetchId) {
    // Step 27: Otherwise: The image data is not in a supported file format; the user agent must set `image request`'s state to `broken`, abort the image request for the current request and the pending request, upgrade the pending request to the current request if `image request` is the pending request, and then, if `maybe omit events` is not set or `previousURL` is not equal to `urlString`, queue an element task on the DOM manipulation task source given `element` to fire an event named `error` at `element`.
    // Note: `maybe omit events` is never set, and the event task is queued
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
    // Note: Density correction is not modeled; the decoded pixel dimensions
    // are returned.
    let image = element.current_request.image_data.as_ref()?;
    Some((image.width, image.height))
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

// Note: This helper merges the shared tail of steps 11 and 13 (the state and
// event handling both entries perform).
fn abort_and_fire_error(
    registry: &mut ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    attributes: &ImageAttributes,
    previous_url: String,
    current_url: String,
) {
    let element = registry.element_mut(document_id, node_id);
    element.current_request.state = ImageRequestState::Broken;
    element.current_request.image_data = None;
    element.current_request.current_url = current_url.clone();
    element.pending_request = None;
    let has_src = attributes.src.is_some();
    if (has_src || attributes.uses_srcset_or_picture()) && previous_url != current_url {
        registry.pending_tasks.push(ImageTask::FireError {
            document_id,
            node_id,
        });
    }
}
