//! The service providers one content document hands to `blitz-dom`: its
//! `MutationHooks` (the HTML DOM tree steps in `dom_trees.rs`) and its
//! `ResourceHooks` (the subresource fetches).  The mutation hooks enter the
//! `<img>` processing model in `images_processing_model.rs`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use blitz_dom::resource::{Resource, ResourceLoadResponse};
use blitz_dom::{BaseDocument, DocumentEvent, FontFaceOverrides, ResourceHooks, ResourceRequest};
use blitz_traits::net::Bytes;
use ipc_messages::content::{
    Command, DocumentFetchId, DocumentId, Event as ContentEvent, EventLoopId, ImageElementId,
    ImageEvent, NavigableId, ResourcePartitionKey,
};
use ipc_messages::network::Request as NetworkRequest;
use log::error;
use style::stylesheets::Origin;
use url::Url;
use uuid::Uuid;

use crate::fetch::{FetchContext, InternalFetchRequest, fetch};
use crate::html::images_processing_model::{ImageFetchSink, ImageRegistry};
use crate::js::{RealmJob, RealmJobQueue};
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
    /// An image the document's layout-time loader asked for (a CSS
    /// `background-image`/`mask-image`, or an `<img>` in a same-origin iframe
    /// subdocument). Nothing decodes it: the graphics process owns every image,
    /// so the request is answered as failed and blitz paints nothing.
    DecodedImage {
        node_id: usize,
        url: String,
        image_type: blitz_dom::util::ImageType,
    },
}

/// The service providers of one content document, handed to `blitz-dom` as
/// its mutation and resource hooks.
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
    /// The realm's microtask queue, so DOM hooks that run without an
    /// execution context can queue their microtasks
    /// (<https://html.spec.whatwg.org/#queue-a-microtask>).  Set once the
    /// document's realm exists.
    job_queue: RefCell<Option<RealmJobQueue>>,
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
            job_queue: RefCell::new(None),
            needs_paint,
        }
    }

    /// Give the DOM hooks the microtask queue of the realm that owns this
    /// document.
    pub(crate) fn set_job_queue(&self, queue: RealmJobQueue) {
        self.job_queue.borrow_mut().replace(queue);
    }

    /// Queue a microtask in the realm that owns this document.
    pub(crate) fn queue_job(&self, job: RealmJob) {
        let queue = self.job_queue.borrow().clone();
        if let Some(queue) = queue {
            queue.queue(job);
        }
    }

    /// Which document of the content process these providers serve.
    pub(crate) fn host(&self) -> DocumentHost {
        self.host
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

    /// The image processing state shared by every document of the content
    /// process.
    pub(crate) fn image_registry(&self) -> &RefCell<ImageRegistry> {
        &self.image_registry
    }

    /// Mark the document as needing a frame, the same flag blitz sets when it
    /// calls `ShellProvider::request_redraw`.
    pub(crate) fn request_redraw(&self) {
        self.needs_paint.set(true);
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

    fn queue_image_element_task(
        &self,
        document_id: DocumentId,
        node_id: usize,
        event: ImageEvent,
        current_url: Option<String>,
    ) {
        let command = Command::RunImageElementTask {
            document_id,
            node_id,
            event,
            current_url,
        };
        if let Err(error) = self.content_command_sender.send(command) {
            error!("failed to queue an image element task: {error}");
        }
    }

    fn queue_finish_environment_update(
        &self,
        document_id: DocumentId,
        node_id: usize,
        instance_id: u64,
        selected_source: String,
        selected_pixel_density: f64,
    ) {
        let command = Command::RunImageEnvironmentUpdate {
            document_id,
            node_id,
            instance_id,
            selected_source,
            selected_pixel_density,
        };
        if let Err(error) = self.content_command_sender.send(command) {
            error!("failed to queue an image environment update: {error}");
        }
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
        AssetRequest::DecodedImage { node_id, url, .. } => {
            // The graphics process is the only image decoder, and it composites
            // `<img>` elements as their own layers, so this process never hands
            // blitz image pixels. The failed load leaves blitz painting nothing
            // where it asked for one (a CSS `background-image`/`mask-image`, or
            // an `<img>` in a same-origin iframe subdocument).
            document.handle_message(DocumentEvent::ResourceLoad(ResourceLoadResponse {
                request_id: 0,
                node_id: Some(*node_id),
                resolved_url: Some(response_url.to_string()),
                result: Err(format!(
                    "image {url} is composited by the graphics process, not painted by the document"
                )),
            }));
            Ok(())
        }
    }
}
