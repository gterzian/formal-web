//! The content process's implementation of the `blitz-dom` document hooks.
//!
//! The hooks report subresource requests and DOM tree mutations while
//! blitz's DOM is mutably borrowed.  Loading a subresource is asynchronous
//! and starts here; DOM algorithms that need the document run on the event
//! loop after the hook returns.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use blitz_dom::resource::{Resource, ResourceLoadResponse};
use blitz_dom::{
    BaseDocument, DocumentEvent, DocumentHooks, ElementData, FontFaceOverrides, ResourceRequest,
};
use blitz_traits::net::Bytes;
use ipc_messages::content::{
    Command, DocumentFetchId, DocumentId, Event as ContentEvent, EventLoopId, NavigableId,
    ResourcePartitionKey,
};
use ipc_messages::network::Request as NetworkRequest;
use log::error;
use style::stylesheets::Origin;
use url::Url;
use uuid::Uuid;

use crate::fetch::{FetchContext, InternalFetchRequest, fetch};
use crate::html::image_data::{
    ImageAttributes, ImageFetchSink, ImageRegistry, cors_settings_mode, referrer_policy_state,
    update_the_image_data,
};
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
/// its [`DocumentHooks`].
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
    embedder_schemes: Arc<HashSet<String>>,
    image_registry: Arc<Mutex<ImageRegistry>>,
    needs_paint: Arc<AtomicBool>,
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
        embedder_schemes: Arc<HashSet<String>>,
        image_registry: Arc<Mutex<ImageRegistry>>,
        needs_paint: Arc<AtomicBool>,
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
            needs_paint,
        }
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
        document_id: DocumentId,
        node_id: usize,
        handler_id: DocumentFetchId,
        url: &str,
    ) {
        self.start_fetch(
            handler_id,
            url.to_string(),
            PendingNetworkHandler::Image {
                document_id,
                navigable_id: self.navigable_id,
                node_id,
            },
        );
    }
}

impl DocumentHooks for DocumentProviders {
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

    /// <https://html.spec.whatwg.org/#relevant-mutations>
    fn attribute_change_steps(
        &self,
        node_id: usize,
        element: &ElementData,
        local_name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        namespace: &str,
    ) {
        if element.name.local.as_ref() != "img" || !namespace.is_empty() {
            return;
        }
        // <https://html.spec.whatwg.org/#relevant-mutations>
        let relevant = match local_name {
            "src" | "srcset" | "sizes" => true,
            "crossorigin" => cors_settings_mode(old_value) != cors_settings_mode(new_value),
            "referrerpolicy" => {
                referrer_policy_state(old_value) != referrer_policy_state(new_value)
            }
            _ => false,
        };
        if !relevant {
            return;
        }
        // <https://html.spec.whatwg.org/#reacting-to-dom-mutations>: setting
        // `src` to the same value sets the restart animation flag.
        // Note: Image animation is not implemented, so the flag has no
        // effect.
        self.run_image_update(node_id, element);
    }

    /// <https://html.spec.whatwg.org/#relevant-mutations>
    fn insertion_steps(&self, node_id: usize, element: Option<&ElementData>) {
        let Some(element) = element else {
            return;
        };
        if element.name.local.as_ref() != "img" {
            return;
        }
        self.run_image_update(node_id, element);
    }

    /// <https://html.spec.whatwg.org/#relevant-mutations>
    fn removing_steps(
        &self,
        node_id: usize,
        element: Option<&ElementData>,
        is_subtree_root: bool,
        old_ancestor: Option<&ElementData>,
    ) {
        let Some(element) = element else {
            return;
        };
        if element.name.local.as_ref() != "img" {
            return;
        }
        // Note: The relevant-mutations list counts every `img` removal, but
        // only a removal from a `<picture>` parent is selected here: it is the
        // case where the selected source could change (picture selection
        // itself is not tracked), while any other removal would re-run the
        // algorithm only to fire a redundant `load` for an available image.
        let removed_from_picture =
            old_ancestor.is_some_and(|ancestor| ancestor.name.local.as_ref() == "picture");
        if !is_subtree_root || !removed_from_picture {
            return;
        }
        self.run_image_update(node_id, element);
    }
}

impl DocumentProviders {
    fn run_image_update(&self, node_id: usize, element: &ElementData) {
        // Same-origin iframe subdocuments keep their images in the parent
        // document's scene (`embedded_images` is false for them), so their
        // images load through the document's layout-time image loader rather
        // than the `<img>` processing model.
        if !matches!(self.host, DocumentHost::Main(_)) {
            return;
        }
        let attributes = ImageAttributes::from_element(element);
        let mut registry = self.image_registry.lock().unwrap_or_else(|error| {
            error!("image registry mutex poisoned: {error}");
            error.into_inner()
        });
        update_the_image_data(
            &mut registry,
            self.host.parent_document_id(),
            node_id,
            &attributes,
            self.base_url.as_ref(),
            &self.partition,
            &self.partition.origin,
        );
        self.needs_paint.store(true, Ordering::Relaxed);
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
                    Arc::new(decoded.into_raw()),
                )),
            }));
            Ok(())
        }
    }
}
