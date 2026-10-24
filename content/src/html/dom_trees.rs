//! The content process's implementation of `blitz-dom`'s DOM mutation hooks
//! and resource hooks.
//!
//! The DOM Standard's hooks enter the HTML Standard's own hooks in
//! `#dom-trees`, which dispatch to the per-element hooks an element's local
//! name defines.  The hooks report subresource requests and DOM tree
//! mutations while blitz's DOM is mutably borrowed.  Loading a subresource is
//! asynchronous and starts here; DOM algorithms that need the document run on
//! the event loop after the hook returns.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use blitz_dom::resource::{Resource, ResourceLoadResponse};
use blitz_dom::{
    BaseDocument, DocumentEvent, ElementData, FontFaceOverrides, MutationHooks, ResourceHooks,
    ResourceRequest,
};
use blitz_traits::net::Bytes;
use html5ever::ns;
use ipc_messages::content::{
    Command, DocumentFetchId, DocumentId, Event as ContentEvent, EventLoopId, ImageElementId,
    NavigableId, ResourcePartitionKey,
};
use ipc_messages::network::Request as NetworkRequest;
use log::error;
use style::stylesheets::Origin;
use url::Url;
use uuid::Uuid;

use crate::fetch::{FetchContext, InternalFetchRequest, fetch};
use crate::html::image_data::{
    IMAGE_ELEMENT_HOOKS, ImageAttributes, ImageFetchSink, ImageRegistry,
    reacting_to_environment_changes as run_reacting_to_environment_changes, update_the_image_data,
};
use crate::js::RealmJobQueue;
use crate::js::platform_objects::with_global_scope;
use crate::{LocalContentStateRef, PendingNetworkHandler};

/// Which document of the content process an asset fetch belongs to: a
/// document in the process's document table, or a same-origin iframe
/// subdocument owned by one of those documents.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DocumentHost {
    Main(DocumentId),
    Subdocument {
        parent_document_id: DocumentId,
        id: Uuid,
    },
}

impl DocumentHost {
    pub(crate) fn parent_document_id(self) -> DocumentId {
        match self {
            Self::Main(document_id) => document_id,
            Self::Subdocument {
                parent_document_id, ..
            } => parent_document_id,
        }
    }
}

/// A subresource fetch the content process started in response to a
/// `blitz-dom` hook or an image mutation.
#[derive(Clone, Debug)]
pub(crate) enum AssetRequest {
    /// A `<link rel=stylesheet>` stylesheet.
    Stylesheet {
        node_id: usize,
        url: String,
        request_id: Option<usize>,
    },
    /// An `@import` stylesheet.
    StylesheetImport { url: String },
    /// An `@font-face` font.
    Font {
        overrides: FontFaceOverrides,
        url: String,
    },
    /// A decoded image installed into the document's layout-time image
    /// loader: a CSS `background-image`/`mask-image`, or an `<img>` in a
    /// same-origin iframe subdocument (which blitz paints into its parent's
    /// scene).
    DecodedImage {
        node_id: usize,
        url: String,
        image_type: blitz_dom::util::ImageType,
    },
}

/// The service providers of one content document, handed to `blitz-dom` as
/// its [`MutationHooks`] and [`ResourceHooks`].
pub(crate) struct DocumentProviders {
    host: DocumentHost,
    navigable_id: NavigableId,
    event_loop_id: EventLoopId,
    /// The document's URL, which subresource URLs are resolved against.  A
    /// `<base>` element's href is not reflected here.
    base_url: Option<Url>,
    partition: ResourcePartitionKey,
    local_state: LocalContentStateRef,
    network_extension_sender: ipc::IpcSender<NetworkRequest>,
    content_command_sender: ipc::IpcSender<Command>,
    event_sender: ipc::IpcSender<ContentEvent>,
    embedder_schemes: Rc<HashSet<String>>,
    image_registry: Rc<RefCell<ImageRegistry>>,
    /// The realm's microtask queue, so the `<img>` hooks can queue the
    /// continuation of step 8 of "update the image data" where the mutation
    /// happens, without an execution context.  Set once the document's realm
    /// exists.
    image_job_queue: RefCell<Option<RealmJobQueue>>,
    needs_paint: Rc<Cell<bool>>,
}

impl DocumentProviders {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        host: DocumentHost,
        navigable_id: NavigableId,
        event_loop_id: EventLoopId,
        base_url: Option<Url>,
        partition: ResourcePartitionKey,
        local_state: LocalContentStateRef,
        network_extension_sender: ipc::IpcSender<NetworkRequest>,
        content_command_sender: ipc::IpcSender<Command>,
        event_sender: ipc::IpcSender<ContentEvent>,
        embedder_schemes: Rc<HashSet<String>>,
        image_registry: Rc<RefCell<ImageRegistry>>,
        needs_paint: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            host,
            navigable_id,
            event_loop_id,
            base_url,
            partition,
            local_state,
            network_extension_sender,
            content_command_sender,
            event_sender,
            embedder_schemes,
            image_registry,
            image_job_queue: RefCell::new(None),
            needs_paint,
        }
    }

    /// Give the hooks the queue of the realm that owns this document.
    ///
    /// <https://html.spec.whatwg.org/#queue-a-microtask>
    pub(crate) fn set_image_job_queue(&self, queue: RealmJobQueue) {
        self.image_job_queue.borrow_mut().replace(queue);
    }

    /// The document's base URL, which the image processing model resolves
    /// selected sources against.
    pub(crate) fn base_url(&self) -> Option<&Url> {
        self.base_url.as_ref()
    }

    /// The partition the document's network requests are made in.
    pub(crate) fn partition(&self) -> &ResourcePartitionKey {
        &self.partition
    }

    fn start_fetch(
        &self,
        handler_id: DocumentFetchId,
        url: String,
        pending: PendingNetworkHandler,
    ) {
        let context = FetchContext {
            event_loop_id: self.event_loop_id,
            navigable_id: self.navigable_id,
            content_command_sender: &self.content_command_sender,
            event_sender: &self.event_sender,
            network_extension_sender: &self.network_extension_sender,
            embedder_schemes: self.embedder_schemes.as_ref(),
            local_state: &self.local_state,
        };
        if let Err(error) = fetch(
            &context,
            handler_id,
            pending,
            InternalFetchRequest::get(url),
        ) {
            error!("failed to start fetch: {error}");
        }
    }
}

impl ImageFetchSink for DocumentProviders {
    fn start_image_fetch(
        &self,
        element_id: ImageElementId,
        document_id: DocumentId,
        handler_id: DocumentFetchId,
        url: &str,
    ) {
        self.start_fetch(
            handler_id,
            url.to_string(),
            PendingNetworkHandler::Image {
                document_id,
                navigable_id: self.navigable_id,
                element_id,
            },
        );
    }
}

/// <https://html.spec.whatwg.org/#dom-trees>
pub(crate) trait HtmlElementHooks {
    /// <https://html.spec.whatwg.org/#html-element-insertion-steps>
    fn insertion_steps(
        &self,
        document: &DocumentProviders,
        node_id: usize,
        inserted_node: &ElementData,
    );

    /// <https://html.spec.whatwg.org/#html-element-post-connection-steps>
    fn post_connection_steps(&self, _: &DocumentProviders, _: usize, _: &ElementData) {}

    /// <https://html.spec.whatwg.org/#html-element-removing-steps>
    fn removing_steps(
        &self,
        _: &DocumentProviders,
        _: usize,
        _: &ElementData,
        _: bool,
        _: Option<&ElementData>,
    ) {
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-change-ext>
    #[allow(clippy::too_many_arguments)]
    fn attribute_change_steps(
        &self,
        _: &DocumentProviders,
        _: usize,
        _: &ElementData,
        _: &str,
        _: Option<&str>,
        _: Option<&str>,
        _: &str,
    ) {
    }
}

/// The per-element hooks the element's local name defines, or `None` when it
/// defines none.
fn html_element_hooks(element: &ElementData) -> Option<&'static dyn HtmlElementHooks> {
    if element.name.ns != ns!(html) {
        return None;
    }
    match element.name.local.as_ref() {
        "img" => Some(IMAGE_ELEMENT_HOOKS),
        _ => None,
    }
}

impl ResourceHooks for DocumentProviders {
    fn resource_request(&self, request: ResourceRequest) {
        let (url, asset) = match request {
            ResourceRequest::Stylesheet {
                node_id,
                url,
                request_id,
            } => (
                url.to_string(),
                AssetRequest::Stylesheet {
                    node_id,
                    url: url.to_string(),
                    request_id,
                },
            ),
            ResourceRequest::StylesheetImport { url } => (
                url.to_string(),
                AssetRequest::StylesheetImport {
                    url: url.to_string(),
                },
            ),
            ResourceRequest::Font { overrides, url, .. } => (
                url.to_string(),
                AssetRequest::Font {
                    overrides,
                    url: url.to_string(),
                },
            ),
            ResourceRequest::Image {
                node_id,
                url,
                image_type,
            } => (
                url.to_string(),
                AssetRequest::DecodedImage {
                    node_id,
                    url: url.to_string(),
                    image_type,
                },
            ),
        };
        let handler_id = DocumentFetchId::new();
        self.start_fetch(
            handler_id,
            url,
            PendingNetworkHandler::Asset {
                host: self.host,
                request: asset,
            },
        );
    }
}

impl MutationHooks for DocumentProviders {
    /// <https://dom.spec.whatwg.org/#concept-element-attributes-change-ext>
    fn attribute_change_steps(
        &self,
        node_id: usize,
        element: &ElementData,
        local_name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        namespace: &str,
    ) {
        if let Some(hooks) = html_element_hooks(element) {
            hooks.attribute_change_steps(
                self, node_id, element, local_name, old_value, new_value, namespace,
            );
        }
    }

    /// <https://html.spec.whatwg.org/#dom-trees:concept-node-insert-ext>
    fn insertion_steps(&self, node_id: usize, element: Option<&ElementData>) {
        // Step 1: "If insertedNode is an element whose namespace is the HTML namespace, and this
        // standard defines HTML element insertion steps for insertedNode's local name, then
        // run the corresponding HTML element insertion steps given insertedNode."
        if let Some(inserted_node) = element
            && let Some(hooks) = html_element_hooks(inserted_node)
        {
            hooks.insertion_steps(self, node_id, inserted_node);
        }
        // Step 2: "If insertedNode is a form-associated element or the ancestor of a
        // form-associated element:"
        // Step 2.1: "If the form-associated element's parser inserted flag is set, then return."
        // Step 2.2: "Reset the form owner of the form-associated element."
        // Note: Form-associated elements are not implemented.
        // Step 3: "If insertedNode is an Element that is not on the stack of open elements of an
        // HTML parser, then process internal resource links given insertedNode's node document."
        // Note: The stack of open elements is parser state that is not available here, and
        // internal resource links are not implemented.
    }

    /// <https://html.spec.whatwg.org/#dom-trees:concept-node-post-connection-ext>
    fn post_connection_steps(&self, node_id: usize, element: Option<&ElementData>) {
        // Step 1: "If insertedNode is an element whose namespace is the HTML namespace, and this
        // standard defines HTML element post-connection steps for insertedNode's local name, then
        // run the corresponding HTML element post-connection steps given insertedNode."
        if let Some(connected_node) = element
            && let Some(hooks) = html_element_hooks(connected_node)
        {
            hooks.post_connection_steps(self, node_id, connected_node);
        }
    }

    /// <https://html.spec.whatwg.org/#dom-trees:concept-node-remove-ext>
    fn removing_steps(
        &self,
        node_id: usize,
        element: Option<&ElementData>,
        is_subtree_root: bool,
        old_ancestor: Option<&ElementData>,
    ) {
        // Step 1: "Let document be removedNode's node document."
        // Step 2: "If document's focused area is removedNode, then set document's focused area to
        // document's viewport, and set document's relevant global object's navigation API's focus
        // changed during ongoing navigation to false."
        // Note: Focus tracking is not implemented, and the document is not reachable from this
        // hook while blitz holds it mutably borrowed.
        // Step 3: "If removedNode is an element whose namespace is the HTML namespace, and this
        // standard defines HTML element removing steps for removedNode's local name, then run the
        // corresponding HTML element removing steps given removedNode, isSubtreeRoot, and
        // oldAncestor."
        if let Some(removed_node) = element
            && let Some(hooks) = html_element_hooks(removed_node)
        {
            hooks.removing_steps(self, node_id, removed_node, is_subtree_root, old_ancestor);
        }
        // Step 4: "If removedNode is a form-associated element with a non-null form owner and
        // removedNode and its form owner are no longer in the same tree, then reset the form
        // owner of removedNode."
        // Step 5: "If removedNode's popover attribute is not in the No Popover state, then run
        // the hide popover algorithm given removedNode, false, false, false, and null."
        // Note: Form-associated elements and popovers are not implemented.
    }
}

impl DocumentProviders {
    /// <https://html.spec.whatwg.org/#when-to-obtain-images>
    pub(crate) fn run_image_update(&self, node_id: usize, element: &ElementData) {
        // "When obtaining images immediately, the user agent must synchronously
        // update the image data of the img element, with the restart animation
        // flag set if so stated, whenever that element is created or has
        // experienced relevant mutations."
        // <https://html.spec.whatwg.org/#when-to-obtain-images:relevant-mutations>
        // The restart animation flag is never set (image animation is not
        // implemented).
        // Same-origin iframe subdocuments keep their images in the parent
        // document's scene (`embedded_images` is false for them), so their
        // images load through the document's layout-time image loader rather
        // than the `<img>` processing model.
        if !matches!(self.host, DocumentHost::Main(_)) {
            return;
        }
        let attributes = ImageAttributes::from_element(element);
        let update = {
            let mut registry = self.image_registry.borrow_mut();
            update_the_image_data(
                &mut registry,
                self.host.parent_document_id(),
                node_id,
                &attributes,
                self.base_url.as_ref(),
                &self.partition.origin,
            )
        };
        self.needs_paint.set(true);
        // Step 8 of "update the image data" queues a microtask; this hook runs
        // without an execution context, so it hands the continuation to the
        // realm's queue, which keeps it in the same order relative to the
        // microtasks the current task queues next.
        if let Some(update) = update
            && let Some(queue) = self.image_job_queue.borrow().clone()
        {
            let job: crate::js::RealmJob = Box::new(move |ec| {
                if let Err(error) = with_global_scope(ec, |global_scope, ec| {
                    global_scope.run_image_update(ec, &update)
                }) {
                    let message = ec
                        .to_rust_string(error)
                        .unwrap_or_else(|_| String::from("unknown error"));
                    error!("failed to continue \"update the image data\": {message}");
                }
            });
            queue.queue(job);
        }
    }

    /// <https://html.spec.whatwg.org/#reacting-to-environment-changes>
    pub(crate) fn reacting_to_environment_changes(
        &self,
        node_id: usize,
        attributes: &ImageAttributes,
    ) {
        if !matches!(self.host, DocumentHost::Main(_)) {
            return;
        }
        let mut registry = self.image_registry.borrow_mut();
        run_reacting_to_environment_changes(
            &mut registry,
            self,
            self.host.parent_document_id(),
            node_id,
            attributes,
            self.base_url.as_ref(),
            &self.partition,
            &self.partition.origin,
        );
    }
}

pub(crate) fn install_resource(
    document: &mut BaseDocument,
    request: &AssetRequest,
    response_url: &str,
    body: &[u8],
) -> Result<(), String> {
    match request {
        AssetRequest::Stylesheet {
            node_id,
            url,
            request_id,
        } => {
            let css = std::str::from_utf8(body)
                .map_err(|error| format!("stylesheet {url} is not UTF-8: {error}"))?;
            let sheet = document.make_stylesheet(css, Origin::Author);
            document.handle_message(DocumentEvent::ResourceLoad(ResourceLoadResponse {
                request_id: request_id.unwrap_or(0),
                node_id: Some(*node_id),
                resolved_url: Some(response_url.to_string()),
                result: Ok(Resource::Css(sheet)),
            }));
            Ok(())
        }
        AssetRequest::StylesheetImport { url } => {
            let css = std::str::from_utf8(body)
                .map_err(|error| format!("imported stylesheet {url} is not UTF-8: {error}"))?;
            document.complete_stylesheet_import(url, css);
            Ok(())
        }
        AssetRequest::Font { overrides, url } => {
            let bytes = blitz_dom::decode_font_bytes(body).into_owned();
            document.handle_message(DocumentEvent::ResourceLoad(ResourceLoadResponse {
                request_id: 0,
                node_id: None,
                resolved_url: Some(url.clone()),
                result: Ok(Resource::Font(Bytes::from(bytes), overrides.clone())),
            }));
            Ok(())
        }
        AssetRequest::DecodedImage {
            node_id,
            url,
            image_type,
        } => {
            let decoded = image::load_from_memory(body)
                .map_err(|error| format!("failed to decode image {url}: {error}"))?
                .into_rgba8();
            let width = decoded.width();
            let height = decoded.height();
            document.handle_message(DocumentEvent::ResourceLoad(ResourceLoadResponse {
                request_id: 0,
                node_id: Some(*node_id),
                resolved_url: Some(response_url.to_string()),
                result: Ok(Resource::Image(
                    *image_type,
                    width,
                    height,
                    std::sync::Arc::new(decoded.into_raw()),
                )),
            }));
            Ok(())
        }
    }
}
