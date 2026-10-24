//! The HTML image processing model.
//!
//! This module owns the per-element image request state
//! (<https://html.spec.whatwg.org/#images-processing-model>) and the "update
//! the image data" algorithm that drives the element's fetches.  It also
//! holds the `<img>` element's HTML element steps, which the general DOM tree
//! steps in `dom_trees.rs` dispatch to.  The steps count their DOM mutations
//! as <https://html.spec.whatwg.org/#reacting-to-dom-mutations:relevant-mutations>;
//! "when to obtain images" then answers a relevant mutation (or the element's
//! creation) by running "update the image data" synchronously
//! (<https://html.spec.whatwg.org/#when-to-obtain-images>).  The state is
//! shared between the steps, the image element's IDL getters, and the event
//! loop's fetch completion handling.

use std::collections::{HashMap, HashSet};

use blitz_dom::ElementData;
use html5ever::local_name;
use ipc_messages::content::{
    DocumentFetchId, DocumentId, ImageElementId, ImageIdentifier, ResourcePartitionKey,
};
use log::error;
use url::Url;

use crate::html::document_providers::{DocumentHost, DocumentProviders};
use crate::js::RealmJob;
use crate::js::platform_objects::with_global_scope;

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
    /// Which algorithm created the request, so its fetch's processResponse
    /// runs the matching continuation.
    pub(crate) source: ImageRequestSource,
    /// The state "reacting to environment changes" carries from its
    /// synchronous section to `finishUpdate`.
    pub(crate) environment_update: Option<EnvironmentUpdate>,
}

/// Which algorithm created an image request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ImageRequestSource {
    /// <https://html.spec.whatwg.org/#update-the-image-data>
    #[default]
    UpdateTheImageData,
    /// <https://html.spec.whatwg.org/#reacting-to-environment-changes>
    ReactingToEnvironmentChanges,
}

/// The state "reacting to environment changes" carries from its synchronous
/// section to its `finishUpdate` steps.
#[derive(Clone, Debug)]
pub(crate) struct EnvironmentUpdate {
    /// The image-data algorithm instance that was current when the
    /// environment algorithm started (step 15.1).
    pub(crate) instance_id: u64,
    /// The URL and pixel density selected at step 3.
    pub(crate) selected_source: String,
    pub(crate) selected_pixel_density: f64,
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
            source: ImageRequestSource::UpdateTheImageData,
            environment_update: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedImage {
    pub(crate) image_id: ImageIdentifier,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The graphics process's decode outcome for `image_id`; `None` until it
    /// reports one.  A request that reuses a cached entry inherits it, so
    /// `decode()` settles without waiting for a second report.
    pub(crate) decode: Option<ImageDecode>,
}

/// The image-processing state of one `<img>` element.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImageElement {
    /// The document the element belongs to, and its DOM node id.
    pub(crate) document_id: DocumentId,
    pub(crate) node_id: usize,
    /// <https://html.spec.whatwg.org/#current-request>
    pub(crate) current_request: ImageRequest,
    /// <https://html.spec.whatwg.org/#pending-request>
    pub(crate) pending_request: Option<ImageRequest>,
    /// <https://html.spec.whatwg.org/#last-selected-source>
    pub(crate) last_selected_source: Option<String>,
    /// The image-data algorithm instance that most recently started for the
    /// element.  A later instance invalidates an earlier queued microtask or
    /// `finishUpdate` (step 9 and environment step 15.1).
    pub(crate) update_instance_id: u64,
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
        /// The current request URL the task sets before firing, when the
        /// algorithm's queued steps include it (steps 11.2.1 and 13.4.1).
        current_url: Option<String>,
    },
    /// The element task queued by "reacting to environment changes" to run
    /// `finishUpdate` (steps 16.2 and 17.5.3).
    FinishEnvironmentUpdate {
        document_id: DocumentId,
        node_id: usize,
        instance_id: u64,
        selected_source: String,
        selected_pixel_density: f64,
    },
}

impl ImageTask {
    /// The element this task was queued for.
    pub(crate) fn element(&self) -> (DocumentId, usize) {
        match self {
            Self::PrepareForPresentation {
                document_id,
                node_id,
                ..
            }
            | Self::FireLoad {
                document_id,
                node_id,
            }
            | Self::FireError {
                document_id,
                node_id,
                ..
            }
            | Self::FinishEnvironmentUpdate {
                document_id,
                node_id,
                ..
            } => (*document_id, *node_id),
        }
    }
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

/// The identity of the "update the image data" instance whose step 8 queued a
/// microtask: the continuation reads the element, the document and the
/// processing model state when it runs.
pub(crate) struct PendingImageDataUpdate {
    /// The element the algorithm instance is running for.
    pub(crate) element_id: ImageElementId,
    /// The image-data algorithm instance that queued the microtask (step 9).
    pub(crate) instance_id: u64,
}

/// The document facts the step-8 continuation reads when it runs: the
/// element's attributes (step 10 selects an image source from them again), and
/// the document's base URL, partition and origin.
pub(crate) struct ImageUpdateEnvironment {
    pub(crate) attributes: ImageAttributes,
    pub(crate) base_url: Option<Url>,
    pub(crate) partition: ResourcePartitionKey,
    pub(crate) origin: String,
}

/// The image-processing state of the content process, shared by every
/// document it hosts.
#[derive(Default)]
pub(crate) struct ImageRegistry {
    /// The processing model state of every `<img>` element this process has
    /// seen, keyed by the element's identity: the value the DOM hooks, the
    /// step-8 continuation and the image fetch's response all carry.
    pub(crate) elements: HashMap<ImageElementId, ImageElement>,
    /// The element identity for a DOM node, allocated the first time the
    /// processing model sees the element.
    pub(crate) element_ids: HashMap<(DocumentId, usize), ImageElementId>,
    /// <https://html.spec.whatwg.org/#list-of-available-images>
    pub(crate) available_images: HashMap<(DocumentId, String), LoadedImage>,
    /// Identifies the next fetched image resource within this process.
    pub(crate) next_blob_id: u64,
    /// Identifies the next image request instance within this process.
    pub(crate) next_request_id: u64,
    /// Identifies the next image-data algorithm instance within this process.
    pub(crate) next_update_instance_id: u64,
    /// Element tasks the algorithms queued, drained by `ContentProcess`.
    pub(crate) pending_tasks: Vec<ImageTask>,
}

pub(crate) trait ImageFetchSink {
    fn start_image_fetch(
        &self,
        element_id: ImageElementId,
        document_id: DocumentId,
        handler_id: DocumentFetchId,
        url: &str,
    );
}

/// The `<img>` attributes relevant to the processing model.
#[derive(Clone)]
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

/// <https://html.spec.whatwg.org/#when-to-obtain-images>
pub(crate) fn run_image_update(
    document: &DocumentProviders,
    node_id: usize,
    element: &ElementData,
) {
    // "When obtaining images immediately, the user agent must synchronously
    // update the image data of the img element, with the restart animation
    // flag set if so stated, whenever that element is created or has
    // experienced relevant mutations."
    // <https://html.spec.whatwg.org/#when-to-obtain-images:relevant-mutations>
    // The restart animation flag is never set (image animation is not
    // implemented).
    // Note: Same-origin iframe subdocuments keep their images in the parent
    // document's scene (`embedded_images` is false for them), so their images
    // load through the document's layout-time image loader rather than the
    // `<img>` processing model.
    if !matches!(document.host(), DocumentHost::Main(_)) {
        return;
    }
    let document_id = document.host().parent_document_id();
    let attributes = ImageAttributes::from_element(element);
    let update = {
        let mut registry = document.image_registry().borrow_mut();
        update_the_image_data(
            &mut registry,
            document_id,
            node_id,
            &attributes,
            document.base_url(),
            &document.partition().origin,
        )
    };
    document.request_redraw();
    // Step 8 of "update the image data" queues a microtask; this runs without
    // an execution context, so it hands the continuation to the realm's queue,
    // which keeps it in the same order relative to the microtasks the current
    // task queues next.
    if let Some(update) = update {
        let job: RealmJob = Box::new(move |ec| {
            if let Err(error) = with_global_scope(ec, |global_scope, ec| {
                global_scope.run_image_update(ec, &update)
            }) {
                let message = ec
                    .to_rust_string(error)
                    .unwrap_or_else(|_| String::from("unknown error"));
                error!("failed to continue \"update the image data\": {message}");
            }
        });
        document.queue_image_job(job);
    }
}

/// <https://html.spec.whatwg.org/#the-img-element:html-element-insertion-steps>
pub(crate) fn insertion_steps(
    document: &DocumentProviders,
    node_id: usize,
    inserted_node: &ElementData,
) {
    // Step 1: "If insertedNode's parent is a picture element, then, count this as a relevant mutation for insertedNode."
    // This counts the mutation as a relevant mutation.  When obtaining
    // images immediately, that synchronously updates the image data
    // (<https://html.spec.whatwg.org/#when-to-obtain-images>).
    // Note: The DOM insertion hook is passed the element without the node tree, so the parent is not available.  The image data is updated for every insertion instead: the algorithm also runs when the element is created, and insertion is the first point at which this implementation observes the element.
    run_image_update(document, node_id, inserted_node);
}

/// <https://html.spec.whatwg.org/#the-img-element:html-element-removing-steps>
pub(crate) fn removing_steps(
    document: &DocumentProviders,
    node_id: usize,
    removed_node: &ElementData,
    is_subtree_root: bool,
    old_ancestor: Option<&ElementData>,
) {
    // Step 1: "If isSubtreeRoot is true and oldAncestor is a picture element, then count this as a relevant mutation for removedNode."
    // Note: Only a removal from a `<picture>` parent re-runs the algorithm: it is the case where the selected source could change (picture selection itself is not tracked), while any other removal would re-run the algorithm only to fire a redundant `load` for an available image.
    let removed_from_picture =
        old_ancestor.is_some_and(|ancestor| ancestor.name.local.as_ref() == "picture");
    if !is_subtree_root || !removed_from_picture {
        return;
    }
    // Step 2: "Set removedNode's last auto-sizes width to null."
    // Note: The last auto-sizes width is not tracked; auto-sizes is not implemented.
    // The mutation is a relevant mutation; obtaining images immediately
    // updates the image data
    // (<https://html.spec.whatwg.org/#when-to-obtain-images>).
    run_image_update(document, node_id, removed_node);
}

/// <https://html.spec.whatwg.org/#reacting-to-dom-mutations:relevant-mutations>
pub(crate) fn attribute_change_steps(
    document: &DocumentProviders,
    node_id: usize,
    element: &ElementData,
    local_name: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
    namespace: &str,
) {
    // The relevant mutations for an img element are as follows:
    // "The element's src, srcset, or sizes attributes are set, changed, or removed."
    // "The element's src attribute is set to the same value as the previous value. This must set the restart animation flag for the update the image data algorithm."
    // "The element's crossorigin attribute's state is changed."
    // "The element's referrerpolicy attribute's state is changed."
    // Note: The remaining relevant mutations (`<source>` sibling changes under a `<picture>` parent, the adopting steps, and auto-sizes) are not observed.
    if !namespace.is_empty() {
        return;
    }
    let relevant = match local_name {
        "src" | "srcset" | "sizes" => true,
        "crossorigin" => cors_settings_mode(old_value) != cors_settings_mode(new_value),
        "referrerpolicy" => referrer_policy_state(old_value) != referrer_policy_state(new_value),
        _ => false,
    };
    if !relevant {
        return;
    }
    // Note: The restart animation flag is not implemented because image animation is not.
    // The attribute change is one of the relevant mutations; obtaining
    // images immediately updates the image data
    // (<https://html.spec.whatwg.org/#when-to-obtain-images>).
    run_image_update(document, node_id, element);
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
    /// The identity of the element at `node_id`, allocated the first time the
    /// processing model sees it.  Every later input for this element (a DOM
    /// hook, the step-8 continuation, the image fetch's response) carries this
    /// value, so the state machine never has to search for the element.
    pub(crate) fn element_id(&mut self, document_id: DocumentId, node_id: usize) -> ImageElementId {
        *self.element_ids.entry((document_id, node_id)).or_default()
    }

    /// The element's identity, if the processing model has seen it.
    pub(crate) fn existing_element_id(
        &self,
        document_id: DocumentId,
        node_id: usize,
    ) -> Option<ImageElementId> {
        self.element_ids.get(&(document_id, node_id)).copied()
    }

    /// The document and DOM node an element identity is registered for.
    pub(crate) fn element_location(
        &self,
        element_id: ImageElementId,
    ) -> Option<(DocumentId, usize)> {
        self.elements
            .get(&element_id)
            .map(|element| (element.document_id, element.node_id))
    }

    /// The processing model state for one element, created on first use.
    pub(crate) fn element(&self, document_id: DocumentId, node_id: usize) -> ImageElement {
        self.existing_element_id(document_id, node_id)
            .and_then(|element_id| self.elements.get(&element_id).cloned())
            .unwrap_or_default()
    }

    pub(crate) fn element_mut(
        &mut self,
        document_id: DocumentId,
        node_id: usize,
    ) -> &mut ImageElement {
        let element_id = self.element_id(document_id, node_id);
        self.elements
            .entry(element_id)
            .or_insert_with(|| ImageElement {
                document_id,
                node_id,
                ..ImageElement::default()
            })
    }

    pub(crate) fn take_pending_tasks(&mut self) -> Vec<ImageTask> {
        std::mem::take(&mut self.pending_tasks)
    }

    /// Forget a destroyed document's element state, including its list of
    /// available images: the list belongs to the Document that owned it.
    /// Returns the image identifiers the document registered, so the content
    /// process can release their decoded state and shared surfaces in the
    /// graphics process.
    pub(crate) fn forget_document(&mut self, document_id: DocumentId) -> Vec<ImageIdentifier> {
        let mut image_ids = HashSet::new();
        for element in self.elements.values() {
            if element.document_id != document_id {
                continue;
            }
            if let Some(image_id) = element.current_request.image_id.clone() {
                image_ids.insert(image_id);
            }
            if let Some(image_id) = element
                .pending_request
                .as_ref()
                .and_then(|pending| pending.image_id.clone())
            {
                image_ids.insert(image_id);
            }
        }
        for ((image_document_id, _), loaded) in &self.available_images {
            if *image_document_id == document_id {
                image_ids.insert(loaded.image_id.clone());
            }
        }
        self.elements
            .retain(|_, element| element.document_id != document_id);
        self.element_ids
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
            }
            | ImageTask::FinishEnvironmentUpdate {
                document_id: task_document_id,
                ..
            } => *task_document_id != document_id,
        });
        image_ids.into_iter().collect()
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
    origin: &str,
) -> Option<PendingImageDataUpdate> {
    // Step 1: If `element`'s node document is not fully active:
    // Note: The content process only runs this algorithm for an active
    // document, so the step and its substeps do not apply.
    // Step 2: If the user agent cannot support images, or its support for images has been disabled, then
    // Note: This user agent supports images.

    // Step 3: Let `previousURL` be the current request's current URL.
    // Note: `previousURL` is compared against only when `maybe omit events`
    // is set, and it never is (the flag is set only by the auto-sizes
    // mutation, which is not implemented), so it is not tracked.
    let instance_id = next_update_instance_id(registry);

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

    {
        let element = registry.element_mut(document_id, node_id);
        // Step 6: Set `element`'s last selected source to `selected source`.
        element.last_selected_source = selected_source.clone();
        // Records that this instance is now the latest one (step 9).
        element.update_instance_id = instance_id;
    }

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
                abort_the_image_request(Some(&mut element.current_request));
                abort_the_image_request(element.pending_request.as_mut());
                // Step 7.4.3: Set the pending request to null.
                element.pending_request = None;
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
                    source: ImageRequestSource::UpdateTheImageData,
                    environment_update: None,
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
                return None;
            }
        }
    }

    // Step 8: Queue a microtask to perform the rest of this algorithm, allowing the task that invoked this algorithm to continue.
    // Note: The DOM hooks run without an execution context, so the caller
    // queues the returned update as a microtask in the element's realm.
    Some(PendingImageDataUpdate {
        element_id: registry.element_id(document_id, node_id),
        instance_id,
    })
}

/// Allocates the next image request instance id.
fn next_request_id(registry: &mut ImageRegistry) -> u64 {
    registry.next_request_id += 1;
    registry.next_request_id
}

/// Allocates the next image-data algorithm instance id.
fn next_update_instance_id(registry: &mut ImageRegistry) -> u64 {
    registry.next_update_instance_id += 1;
    registry.next_update_instance_id
}

/// <https://html.spec.whatwg.org/#abort-the-image-request>
fn abort_the_image_request(request: Option<&mut ImageRequest>) {
    // Step 1: "If `image request` is null, then return."
    let Some(request) = request else {
        return;
    };
    // Step 2: "Forget `image request`'s image data, if any."
    request.image_data = None;
    // Step 3: "Abort any instance of the fetching algorithm for `image request`, discarding any pending tasks generated by that algorithm."
    // Note: The net fetch cannot be cancelled from content; clearing the
    // fetch id makes a late response's fetch-id match fail.
    request.fetch_id = None;
}

/// <https://html.spec.whatwg.org/#upgrade-the-pending-request-to-the-current-request>
fn upgrade_the_pending_request_to_the_current_request(element: &mut ImageElement) {
    // Step 1: "Set the `img` element's current request to the pending request."
    // Step 2: "Set the `img` element's pending request to null."
    element.current_request = element.pending_request.take().unwrap_or_default();
}

/// <https://html.spec.whatwg.org/#select-an-image-source>
fn select_an_image_source(attributes: &ImageAttributes) -> Option<(String, f64)> {
    // Note: The source set's `sizes` descriptor is not evaluated; the first
    // candidate of the `srcset` attribute is chosen, and the `src` attribute
    // is the fallback.  Every candidate therefore has a pixel density of 1.0.
    if let Some(srcset) = attributes.srcset.as_deref() {
        for candidate in srcset.split(',') {
            let first_token = candidate.split_ascii_whitespace().next();
            if let Some(token) = first_token {
                let token = token.trim_end_matches(',');
                if !token.is_empty() {
                    return Some((token.to_string(), 1.0));
                }
            }
        }
    }
    attributes
        .src
        .clone()
        .filter(|src| !src.is_empty())
        .map(|src| (src, 1.0))
}

/// <https://html.spec.whatwg.org/#update-the-image-data>
pub(crate) fn continue_update_the_image_data(
    registry: &mut ImageRegistry,
    sink: &dyn ImageFetchSink,
    environment: &ImageUpdateEnvironment,
    update: &PendingImageDataUpdate,
) {
    let Some((document_id, node_id)) = registry.element_location(update.element_id) else {
        return;
    };
    // Step 9: If another instance of this algorithm for `element` was started after this instance (even if it aborted and is no longer running), then return.
    if registry.element(document_id, node_id).update_instance_id != update.instance_id {
        return;
    }

    // Step 10: Let `selected source` and `selected pixel density` be the URL and pixel density that results from selecting an image source, respectively.
    // Note: The source set is selected by `select_an_image_source`: the first
    // `srcset` candidate, or `src` when the source set yields nothing.  A
    // `sizes`-driven choice is not modeled.
    let Some((selected_source, selected_pixel_density)) =
        select_an_image_source(&environment.attributes)
    else {
        // Step 11.1: Set the current request's state to `broken`, abort the image request for the current request and the pending request, and set the pending request to null.
        {
            let element = registry.element_mut(document_id, node_id);
            element.current_request.state = ImageRequestState::Broken;
            abort_the_image_request(Some(&mut element.current_request));
            abort_the_image_request(element.pending_request.as_mut());
            element.pending_request = None;
        }
        // Step 11.2: Queue an element task on the DOM manipulation task source given `element` and the following steps:
        // Step 11.2.1: Change the current request's current URL to the empty string.
        // Step 11.2.2: If all of the following are true: `element` has a `src` attribute or it uses `srcset` or `picture`; and `maybe omit events` is not set or `previousURL` is not the empty string, then fire an event named `error` at `element`.
        // Note: `maybe omit events` is never set, so the second condition
        // always holds.
        if environment.attributes.src.is_some() || environment.attributes.uses_srcset_or_picture() {
            registry.pending_tasks.push(ImageTask::FireError {
                document_id,
                node_id,
                current_url: Some(String::new()),
            });
        }
        // Step 11.3: Return.
        return;
    };

    // Step 12: Let `urlString` be the result of encoding-parsing-and-serializing a URL given `selected source`, relative to `element`'s node document.
    let Some(url_string) =
        encoding_parsing_and_serializing_url(environment.base_url.as_ref(), &selected_source)
    else {
        // Step 13.1: Abort the image request for the current request and the pending request.
        // Step 13.2: Set the current request's state to `broken`.
        // Step 13.3: Set the pending request to null.
        {
            let element = registry.element_mut(document_id, node_id);
            abort_the_image_request(Some(&mut element.current_request));
            abort_the_image_request(element.pending_request.as_mut());
            element.current_request.state = ImageRequestState::Broken;
            element.pending_request = None;
        }
        // Step 13.4: Queue an element task on the DOM manipulation task source given `element` and the following steps:
        // Step 13.4.1: Change the current request's current URL to `selected source`.
        // Step 13.4.2: If `maybe omit events` is not set or `previousURL` is not equal to `selected source`, then fire an event named `error` at `element`.
        // Note: `maybe omit events` is never set, so the condition always
        // holds.
        registry.pending_tasks.push(ImageTask::FireError {
            document_id,
            node_id,
            current_url: Some(selected_source),
        });
        // Step 13.5: Return.
        return;
    };

    // Step 14: If the pending request is not null and `urlString` is the same as the pending request's current URL, then return.
    {
        let element = registry.element(document_id, node_id);
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
            registry.element_mut(document_id, node_id).pending_request = None;
            // Step 15.2: If `restart animation` is set, then queue an element task on the DOM manipulation task source given `element` to restart the animation.
            // Note: Image animation is not implemented.
            // Step 15.3: Return.
            return;
        }
    }

    // Step 16: Abort the image request for the pending request.
    // Step 17: Set `image request` to a new image request whose current URL is `urlString`.
    let blob_id = registry.next_blob_id;
    registry.next_blob_id += 1;
    let request_id = next_request_id(registry);
    let key = available_images_key(&url_string, &environment.attributes, &environment.origin);
    let handler_id = DocumentFetchId::new();
    let image_request = ImageRequest {
        state: ImageRequestState::Unavailable,
        current_url: url_string.clone(),
        image_data: None,
        current_pixel_density: selected_pixel_density,
        preferred_density_corrected_dimensions: None,
        image_id: Some(ImageIdentifier {
            partition: environment.partition.clone(),
            blob_id,
        }),
        available_images_key: Some(key),
        fetch_id: Some(handler_id),
        request_id,
        source: ImageRequestSource::UpdateTheImageData,
        environment_update: None,
    };
    {
        let element = registry.element_mut(document_id, node_id);
        // Step 16: Abort the image request for the pending request.
        abort_the_image_request(element.pending_request.as_mut());
        element.pending_request = None;
        // Step 18: If the current request's state is `unavailable` or `broken`, then set the current request to `image request`. Otherwise, set the pending request to `image request`.
        let current_is_unavailable = matches!(
            element.current_request.state,
            ImageRequestState::Unavailable | ImageRequestState::Broken
        );
        if current_is_unavailable {
            element.current_request = image_request;
        } else {
            element.pending_request = Some(image_request);
        }
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
    sink.start_image_fetch(update.element_id, document_id, handler_id, &url_string);
}

/// What the fetch's processResponse did with the image, so the content
/// process can hand an available resource to the graphics process.
pub(crate) enum ImageFetchOutcome {
    ImageAvailable(ImageIdentifier),
    Failed,
}

/// The fetch's processResponse continuation for an image request; dispatches
/// to the algorithm that created the request.
pub(crate) fn process_image_fetch_response(
    registry: &mut ImageRegistry,
    element_id: ImageElementId,
    handler_id: DocumentFetchId,
    dimensions: Option<(u32, u32)>,
) -> ImageFetchOutcome {
    let Some(element) = registry.elements.get(&element_id) else {
        return ImageFetchOutcome::Failed;
    };
    // The response belongs to the request that started the fetch: the current
    // request, or the pending one while an update is in flight.
    let source = if element.current_request.fetch_id == Some(handler_id) {
        element.current_request.source
    } else if element
        .pending_request
        .as_ref()
        .is_some_and(|pending| pending.fetch_id == Some(handler_id))
    {
        element
            .pending_request
            .as_ref()
            .map(|pending| pending.source)
            .unwrap_or_default()
    } else {
        // A late response for a request the processing model already aborted.
        return ImageFetchOutcome::Failed;
    };
    let (document_id, node_id) = (element.document_id, element.node_id);
    match source {
        ImageRequestSource::UpdateTheImageData => process_update_the_image_data_response(
            registry,
            document_id,
            node_id,
            handler_id,
            dimensions,
        ),
        ImageRequestSource::ReactingToEnvironmentChanges => {
            process_reacting_to_environment_changes_response(
                registry,
                document_id,
                node_id,
                handler_id,
                dimensions,
            )
        }
    }
}

/// <https://html.spec.whatwg.org/#update-the-image-data>
fn process_update_the_image_data_response(
    registry: &mut ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    handler_id: DocumentFetchId,
    dimensions: Option<(u32, u32)>,
) -> ImageFetchOutcome {
    let Some(dimensions) = dimensions else {
        // Step 27 (Otherwise): "The image data is not in a supported file format; the user agent must set `image request`'s state to `broken`, abort the image request for the current request and the pending request, upgrade the pending request to the current request if `image request` is the pending request, and then, if `maybe omit events` is not set or `previousURL` is not equal to `urlString`, queue an element task on the DOM manipulation task source given `element` to fire an event named `error` at `element`."
        // Note: `maybe omit events` is never set, so the event task is queued
        // unconditionally below.
        let element = registry.element_mut(document_id, node_id);
        if element
            .pending_request
            .as_ref()
            .is_some_and(|pending| pending.fetch_id == Some(handler_id))
        {
            upgrade_the_pending_request_to_the_current_request(element);
        }
        element.current_request.state = ImageRequestState::Broken;
        abort_the_image_request(Some(&mut element.current_request));
        abort_the_image_request(element.pending_request.as_mut());
        element.pending_request = None;
        registry.pending_tasks.push(ImageTask::FireError {
            document_id,
            node_id,
            current_url: None,
        });
        return ImageFetchOutcome::Failed;
    };

    let loaded;
    let key;
    let image_id;
    {
        let element = registry.element_mut(document_id, node_id);
        if element
            .pending_request
            .as_ref()
            .is_some_and(|pending| pending.fetch_id == Some(handler_id))
        {
            // Step 27.11: "If `image request` is the pending request, abort the image request for the current request, upgrade the pending request to the current request, and prepare `image request` for presentation given `element`."
            abort_the_image_request(Some(&mut element.current_request));
            upgrade_the_pending_request_to_the_current_request(element);
        }
        element.current_request.fetch_id = None;
        let mut request = element.current_request.clone();
        let Some(request_image_id) = request.image_id.clone() else {
            return ImageFetchOutcome::Failed;
        };
        image_id = request_image_id;
        loaded = LoadedImage {
            image_id: image_id.clone(),
            width: dimensions.0,
            height: dimensions.1,
            decode: None,
        };
        // Step 27 (the next network task), "If the resource type and data corresponds to a supported image format": "If the user agent is able to determine `image request`'s image's width and height, and `image request` is the pending request, set `image request`'s state to `partially available`."
        // Step 27 (the next network task), second entry: "Otherwise, if the user agent is able to determine `image request`'s image's width and height, and `image request` is the current request, prepare `image request` for presentation given `element` and set `image request`'s state to `partially available`."
        // Step 27.5: "Set `element`'s current request's state to `completely available`."
        // Note: The response body arrives whole, so the request is prepared for
        // presentation and set to completely available in the same task; the
        // partially available state is never presented on its own.
        request.state = ImageRequestState::CompletelyAvailable;
        request.image_data = Some(loaded.clone());
        request.preferred_density_corrected_dimensions = Some(dimensions);
        key = request.available_images_key.clone();
        element.current_request = request;
    }

    // Step 27.13: "Add the image to the list of available images using the key `key`, with the ignore higher-layer caching flag set."
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
    // Step 27.14: "If `maybe omit events` is not set or `previousURL` is not equal to `urlString`, then fire an event named `load` at `element`."
    // Note: `maybe omit events` is never set, so the event always fires.
    registry.pending_tasks.push(ImageTask::FireLoad {
        document_id,
        node_id,
    });
    ImageFetchOutcome::ImageAvailable(image_id)
}

/// <https://html.spec.whatwg.org/#reacting-to-environment-changes>
fn process_reacting_to_environment_changes_response(
    registry: &mut ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    handler_id: DocumentFetchId,
    dimensions: Option<(u32, u32)>,
) -> ImageFetchOutcome {
    let Some(dimensions) = dimensions else {
        // Step 17.5.1: "If response's unsafe response is a network error or if the image format is unsupported (as determined by applying the image sniffing rules, again as mentioned earlier), or if the user agent is able to determine that `image request`'s image is corrupted in some fatal way such that the image dimensions cannot be obtained, or if the resource type is `multipart/x-mixed-replace`, then set the pending request to null and return."
        {
            let element = registry.element_mut(document_id, node_id);
            if element
                .pending_request
                .as_ref()
                .is_some_and(|pending| pending.fetch_id == Some(handler_id))
            {
                element.pending_request = None;
            }
        }
        return ImageFetchOutcome::Failed;
    };

    let image_id;
    let environment_update;
    {
        let element = registry.element_mut(document_id, node_id);
        let Some(pending) = element.pending_request.as_mut() else {
            return ImageFetchOutcome::Failed;
        };
        if pending.fetch_id != Some(handler_id) {
            return ImageFetchOutcome::Failed;
        }
        let Some(request_image_id) = pending.image_id.clone() else {
            return ImageFetchOutcome::Failed;
        };
        image_id = request_image_id;
        pending.fetch_id = None;
        // Step 17.5.2: "Otherwise, response's unsafe response is `image request`'s image data."
        pending.image_data = Some(LoadedImage {
            image_id: image_id.clone(),
            width: dimensions.0,
            height: dimensions.1,
            decode: None,
        });
        pending.preferred_density_corrected_dimensions = Some(dimensions);
        let Some(update) = pending.environment_update.clone() else {
            return ImageFetchOutcome::Failed;
        };
        environment_update = update;
    }

    // Step 17.5.3: "Queue an element task on the DOM manipulation task source given the `img` element and `finishUpdate`."
    registry
        .pending_tasks
        .push(ImageTask::FinishEnvironmentUpdate {
            document_id,
            node_id,
            instance_id: environment_update.instance_id,
            selected_source: environment_update.selected_source,
            selected_pixel_density: environment_update.selected_pixel_density,
        });
    ImageFetchOutcome::ImageAvailable(image_id)
}

/// <https://html.spec.whatwg.org/#reacting-to-environment-changes>
pub(crate) fn reacting_to_environment_changes(
    document: &DocumentProviders,
    node_id: usize,
    attributes: &ImageAttributes,
) {
    // Step 1: Await a stable state. The synchronous section consists of all the remaining steps of this algorithm until the algorithm says the synchronous section has ended. (Steps in synchronous sections are marked with ⌛.)
    // Note: The viewport command handler runs this when no task is queued (a
    // stable state).
    // Note: A same-origin iframe subdocument's images load through the
    // document's layout-time image loader, not the processing model.
    if !matches!(document.host(), DocumentHost::Main(_)) {
        return;
    }
    let document_id = document.host().parent_document_id();
    let base_url = document.base_url();
    let partition = document.partition();
    let origin = &partition.origin;
    let mut registry = document.image_registry().borrow_mut();
    let element = registry.element(document_id, node_id);
    // Step 2: ⌛ If the `img` element does not use `srcset` or `picture`, its node document is not fully active, it has image data whose resource type is `multipart/x-mixed-replace`, or its pending request is not null, then return.
    // Note: The content process only runs this for an active document;
    // multipart image data is not modeled; and `uses_srcset_or_picture`
    // checks only the `srcset` attribute because a `<picture>` parent is not
    // tracked.
    if !attributes.uses_srcset_or_picture() || element.pending_request.is_some() {
        return;
    }
    // Step 3: ⌛ Let `selected source` and `selected pixel density` be the URL and pixel density that results from selecting an image source, respectively.
    let Some((selected_source, selected_pixel_density)) = select_an_image_source(attributes) else {
        // Step 4: ⌛ If `selected source` is null, then return.
        return;
    };
    // Step 5: ⌛ If `selected source` and `selected pixel density` are the same as the element's last selected source and current pixel density, then return.
    if element.last_selected_source.as_deref() == Some(selected_source.as_str())
        && element.current_request.current_pixel_density == selected_pixel_density
    {
        return;
    }
    // Step 6: ⌛ Let `urlString` be the result of encoding-parsing-and-serializing a URL given `selected source`, relative to the element's node document.
    let Some(url_string) = encoding_parsing_and_serializing_url(base_url, &selected_source) else {
        // Step 7: ⌛ If `urlString` is failure, then return.
        return;
    };
    // Step 8: ⌛ Let `corsAttributeState` be the state of the element's `crossorigin` content attribute.
    // Step 9: ⌛ Let `origin` be the `img` element's node document's origin.
    // Step 10: ⌛ Let `client` be the `img` element's node document's relevant settings object.
    // Note: `origin` is the caller's document origin; `client` is not used by
    // the fetch request yet.
    // Step 11: ⌛ Let `key` be a tuple consisting of `urlString`, `corsAttributeState`, and, if `corsAttributeState` is not No CORS, `origin`.
    let key = available_images_key(&url_string, attributes, origin);
    // Step 12: ⌛ Let `image request` be a new image request whose current URL is `urlString`.
    let blob_id = registry.next_blob_id;
    registry.next_blob_id += 1;
    let request_id = next_request_id(&mut registry);
    let environment_update = EnvironmentUpdate {
        instance_id: element.update_instance_id,
        selected_source: selected_source.clone(),
        selected_pixel_density,
    };
    let image_request = ImageRequest {
        state: ImageRequestState::Unavailable,
        current_url: url_string.clone(),
        image_data: None,
        current_pixel_density: 1.0,
        preferred_density_corrected_dimensions: None,
        image_id: Some(ImageIdentifier {
            partition: partition.clone(),
            blob_id,
        }),
        available_images_key: Some(key.clone()),
        fetch_id: None,
        request_id,
        source: ImageRequestSource::ReactingToEnvironmentChanges,
        environment_update: Some(environment_update.clone()),
    };
    // Step 13: ⌛ Set the element's pending request to `image request`.
    registry.element_mut(document_id, node_id).pending_request = Some(image_request);
    // Step 14: End the synchronous section, continuing the remaining steps in parallel.
    // Step 15: Let `finishUpdate` be the following steps: ...
    // Note: `finishUpdate` runs from the element task queued at step 16.2 or
    // step 17.5.3 (`finish_environment_update`).
    // Step 16: If the list of available images contains an entry for `key`:
    if let Some(available) = registry
        .available_images
        .get(&(document_id, key.clone()))
        .cloned()
    {
        // Step 16.1: ⌛ Set `image request`'s image data to that of the entry.
        {
            let element = registry.element_mut(document_id, node_id);
            if let Some(pending) = element.pending_request.as_mut() {
                pending.image_data = Some(available.clone());
                pending.preferred_density_corrected_dimensions =
                    Some((available.width, available.height));
            }
        }
        // Step 16.2: ⌛ Queue an element task on the DOM manipulation task source given the `img` element and `finishUpdate`.
        registry
            .pending_tasks
            .push(ImageTask::FinishEnvironmentUpdate {
                document_id,
                node_id,
                instance_id: environment_update.instance_id,
                selected_source: environment_update.selected_source,
                selected_pixel_density: environment_update.selected_pixel_density,
            });
        return;
    }
    // Step 17: Otherwise:
    // Step 17.1: ⌛ Let `request` be the result of creating a potential-CORS request given `urlString`, "image", and `corsAttributeState`.
    // Step 17.2: ⌛ Set `request`'s client to `client` and set `request`'s initiator to "imageset".
    // Step 17.3: ⌛ Set `request`'s referrer policy to the current state of the element's `referrerpolicy` attribute.
    // Step 17.4: ⌛ Set `request`'s priority to the current state of the element's `fetchpriority` attribute.
    // Note: The fetch request carries the URL; CORS mode, referrer policy,
    // and priority are not distinguished by the net process yet.
    // Step 17.5: ⌛ Fetch `request`, with `processResponse` set to the following steps given response `response`:
    // Note: The response half is `process_reacting_to_environment_changes_response`.
    let handler_id = DocumentFetchId::new();
    let element_id = registry.element_id(document_id, node_id);
    {
        let element = registry.element_mut(document_id, node_id);
        if let Some(pending) = element.pending_request.as_mut() {
            pending.fetch_id = Some(handler_id);
        }
    }
    document.start_image_fetch(element_id, document_id, handler_id, &url_string);
}

/// <https://html.spec.whatwg.org/#reacting-to-environment-changes>
pub(crate) fn finish_environment_update(
    registry: &mut ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    instance_id: u64,
    selected_source: &str,
    selected_pixel_density: f64,
) {
    // Step 15.1: If the `img` element has experienced relevant mutations since this algorithm started, then set the pending request to null and abort these steps.
    let (loaded, key) = {
        let element = registry.element_mut(document_id, node_id);
        let Some(pending) = element.pending_request.as_ref() else {
            return;
        };
        // Note: Only the request this environment update installed is
        // aborted; a newer request installed by "update the image data"
        // belongs to that algorithm.
        if pending.source != ImageRequestSource::ReactingToEnvironmentChanges
            || pending
                .environment_update
                .as_ref()
                .is_none_or(|update| update.instance_id != instance_id)
        {
            return;
        }
        if element.update_instance_id != instance_id {
            element.pending_request = None;
            return;
        }
        // Step 15.2: Set the `img` element's last selected source to `selected source` and the `img` element's current pixel density to `selected pixel density`.
        element.last_selected_source = Some(selected_source.to_string());
        let Some(request) = element.pending_request.as_mut() else {
            return;
        };
        request.current_pixel_density = selected_pixel_density;
        // Step 15.3: Set the `image request`'s state to `completely available`.
        request.state = ImageRequestState::CompletelyAvailable;
        (
            request.image_data.clone(),
            request.available_images_key.clone(),
        )
    };
    // Step 15.4: Add the image to the list of available images using the key `key`, with the ignore higher-layer caching flag set.
    if let (Some(key), Some(loaded)) = (key, loaded.clone()) {
        registry.available_images.insert((document_id, key), loaded);
    }
    // Step 15.5: Upgrade the pending request to the current request.
    upgrade_the_pending_request_to_the_current_request(registry.element_mut(document_id, node_id));
    // Step 15.6: Prepare `image request` for presentation given the `img` element.
    if let Some(loaded) = loaded {
        registry
            .pending_tasks
            .push(ImageTask::PrepareForPresentation {
                document_id,
                node_id,
                width: loaded.width,
                height: loaded.height,
            });
    }
    // Step 15.7: Fire an event named `load` at the `img` element.
    registry.pending_tasks.push(ImageTask::FireLoad {
        document_id,
        node_id,
    });
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

/// Records the graphics process's decode outcome for one image resource on
/// the requests and cached entries that reference it, and returns the elements
/// whose `decode()` promise can now settle.
pub(crate) fn image_decoded(
    registry: &mut ImageRegistry,
    image_id: &ImageIdentifier,
    decoded: bool,
) -> Vec<ImageElementId> {
    let outcome = if decoded {
        ImageDecode::Decoded
    } else {
        ImageDecode::Failed
    };
    let mut elements = Vec::new();
    for (element_id, element) in &mut registry.elements {
        let current = record_decode(&mut element.current_request, image_id, outcome);
        let pending = element
            .pending_request
            .as_mut()
            .is_some_and(|pending| record_decode(pending, image_id, outcome));
        if current || pending {
            elements.push(*element_id);
        }
    }
    // A later request for the same URL reuses the cached entry (step 7.4.4 of
    // "update the image data"), so the entry keeps the outcome too.
    for loaded in registry.available_images.values_mut() {
        if &loaded.image_id == image_id {
            loaded.decode = Some(outcome);
        }
    }
    elements
}

/// Records `outcome` on `request` if its image data is `image_id`.
fn record_decode(
    request: &mut ImageRequest,
    image_id: &ImageIdentifier,
    outcome: ImageDecode,
) -> bool {
    match request.image_data.as_mut() {
        Some(image) if &image.image_id == image_id => {
            image.decode = Some(outcome);
            true
        }
        _ => false,
    }
}

/// The decision `decode()` step 2.3 makes for a promise waiting on the request
/// the element's current request had when the promise was registered.  A
/// request id of zero means the processing model never installed a request for
/// the element (a detached `<img>` with no source), which "update the image
/// data" leaves broken: such a promise rejects.
pub(crate) fn decode_settlement(
    registry: &ImageRegistry,
    document_id: DocumentId,
    node_id: usize,
    request_id: u64,
) -> DecodeSettlement {
    let element = registry.element(document_id, node_id);
    let request = &element.current_request;
    // "This img element's current request changes or is mutated"
    if request.request_id != request_id || request.request_id == 0 {
        return DecodeSettlement::Reject;
    }
    match request.state {
        ImageRequestState::Broken => DecodeSettlement::Reject,
        ImageRequestState::CompletelyAvailable => {
            let decode = request.image_data.as_ref().and_then(|image| image.decode);
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
