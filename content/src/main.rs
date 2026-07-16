#[path = "../../embedder/src/ui_event.rs"]
#[allow(dead_code)]
pub(crate) mod ui_event;

pub mod css;
pub(crate) mod fetch;
pub mod infra;
pub mod js;
pub mod testutils;

pub mod dom;
#[cfg(test)]
mod generic_js_test;
pub mod html;
pub mod streams;
#[cfg(all(boa_backend, feature = "wasm"))]
pub mod wasm;
pub mod webidl;

use crate::dom::{EventTargetAccess, dispatch_trusted_click_event, dispatch_ui_event, fire_event};
use crate::html::{
    EnvironmentSettingsObject, JsHtmlParserProvider, PendingParserScript,
    attach_same_origin_child_document_for_traversable, execute_parser_scripts,
    parse_html_into_document, run_dom_post_connection_steps_for_document,
    run_dom_removing_steps_for_document, run_iframe_load_event_steps_for_traversable,
};
use crate::js::platform_objects::with_global_scope;
use crate::ui_event::deserialize_ui_event;
#[cfg(all(boa_backend, feature = "wasm"))]
use crate::wasm::{WasmResult, compile_continuation, compile_rejection, instantiate_continuation};
use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_paint::paint_scene;
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use data_url::DataUrl;
use html5ever::local_name;
use js_engine::ExecutionContext;

use ipc_messages::content::Command::{
    ClickElement, CompleteDocumentFetch, ContentBootstrap, CreateEmptyDocument,
    CreateLoadedDocument, DestroyDocument, DispatchEvent, EvaluateScript, FailDocumentFetch,
    RunWindowTimer, SetTraversableViewport, SetViewport, Shutdown, UpdateTheRendering,
};
use ipc_messages::content::{
    BeforeUnloadCheckId, ClipboardWriteRequested, ColorScheme as MessageColorScheme, Command,
    DispatchEventEntry, DocumentFetchId, DocumentId, ElementClickResult, EmbedBackgroundPolicy,
    EmbedLayout, EmbedSite, EmbedSiteId, Event as ContentEvent, EventLoopId,
    FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse,
    FontTransportSender, FrameCompositionMetadata, FrameId, IframeEmbedSite,
    LoadedDocumentResponse, NavigableId, NavigationId, PaintFrame, ScriptEvaluationResult,
    TraversableViewport, ViewportSnapshot, WebviewId, WindowTimerKey,
};
use ipc_messages::media::{VideoEmbedData, VideoPaintId};
use log::{debug, error, trace, warn};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    rc::Rc,
    sync::{Arc, LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;
use verification::{TLATracer, TraceSender};

pub(crate) const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";

static LOGGED_INPUT_LAYOUT_DOCUMENTS: LazyLock<Mutex<HashSet<DocumentId>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn normalized_content_type_essence(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_javascript_mime_essence(essence: &str) -> bool {
    matches!(
        essence,
        "text/javascript"
            | "application/javascript"
            | "application/ecmascript"
            | "text/ecmascript"
            | "application/x-javascript"
            | "text/x-javascript"
    )
}

fn deferred_script_response_is_executable(response: &ContentFetchResponse) -> bool {
    if !(200..=299).contains(&response.status) {
        return false;
    }

    let essence = normalized_content_type_essence(&response.content_type);
    essence.is_empty() || is_javascript_mime_essence(&essence)
}

fn new_font_namespace() -> u64 {
    let pid = u64::from(std::process::id());
    let start_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    pid.rotate_left(32) ^ start_nanos
}

enum PendingNetworkHandler {
    Resource {
        document_id: DocumentId,
        request_url: String,
        handler: Box<dyn NetHandler>,
    },
    DeferredScript {
        document_id: DocumentId,
        script_index: usize,
    },
}

struct LocalContentState {
    pending_handlers: HashMap<DocumentFetchId, PendingNetworkHandler>,
}

pub(crate) type LocalContentStateRef = Arc<Mutex<LocalContentState>>;

pub(crate) fn new_document_fetch_id() -> DocumentFetchId {
    DocumentFetchId::new()
}

/// Shared clipboard cache for paste-without-IPC.
/// `get_clipboard_text` reads from this cache instead of doing a blocking
/// IPC round-trip. The embedder prefetches clipboard text before dispatching
/// paste events and writes it here via `set_clipboard_cache`.
type ClipboardCache = std::sync::Arc<std::sync::Mutex<Option<String>>>;

fn new_clipboard_cache() -> ClipboardCache {
    std::sync::Arc::new(std::sync::Mutex::new(None))
}

struct ContentShellProvider {
    event_sender: ipc::IpcSender<ContentEvent>,
    clipboard_cache: ClipboardCache,
}

impl ContentShellProvider {
    fn new(event_sender: ipc::IpcSender<ContentEvent>, clipboard_cache: ClipboardCache) -> Self {
        Self {
            event_sender,
            clipboard_cache,
        }
    }
}

impl ShellProvider for ContentShellProvider {
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        // First try the prefetched cache (populated by the embedder before
        // dispatching paste events via DispatchEventEntry.prefetched_clipboard_text).
        if let Ok(mut cache) = self.clipboard_cache.lock() {
            if let Some(text) = cache.take() {
                return Ok(text);
            }
        }
        // Fall back to reading the system clipboard directly.
        // This avoids a blocking IPC round-trip and works because the
        // clipboard is a shared system resource accessible from any process.
        clipboard_direct_read()
    }

    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        // Fire-and-forget: send the write request, no reply expected.
        self.event_sender
            .send(ContentEvent::ClipboardWriteRequested(
                ClipboardWriteRequested { text },
            ))
            .map_err(|_| ClipboardError)
    }
}

/// Read the system clipboard directly from this process.
/// Used as a fallback when the prefetched clipboard cache is empty.
/// This is a best-effort read; if the clipboard cannot be accessed,
/// an empty string is returned.
fn clipboard_direct_read() -> Result<String, ClipboardError> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => match clipboard.get_text() {
                Ok(text) => Ok(text),
                Err(_) => Ok(String::new()),
            },
            Err(_) => Ok(String::new()),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(String::new())
    }
}

enum DeferredScriptState {
    Inline { source: String },
    ExternalPending { src: String },
    ExternalReady { source: String },
    ExternalFailed { src: String },
}

#[derive(Clone)]
pub(crate) struct NavigableContainerState {
    pub(crate) content_navigable: Option<NavigableId>,
    pub(crate) content_frame_id: FrameId,
    pub(crate) current_key: String,
    pub(crate) cross_origin: bool,
}

struct PendingDocumentLoad {
    finalize_url: String,
    scripts: Vec<DeferredScriptState>,
}

fn request_body_string(body: &Body) -> String {
    match body {
        Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Body::Form(form) => serde_json::to_string(form).unwrap_or_default(),
        Body::Empty => String::new(),
    }
}

fn viewport_of_snapshot(snapshot: &ViewportSnapshot) -> Viewport {
    let color_scheme = match snapshot.color_scheme {
        MessageColorScheme::Light => ColorScheme::Light,
        MessageColorScheme::Dark => ColorScheme::Dark,
    };
    Viewport::new(
        snapshot.width,
        snapshot.height,
        snapshot.scale,
        color_scheme,
    )
}

fn render_state_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_RENDER_STATE").is_some()
}

fn input_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

fn log_render_state_debug(message: impl AsRef<str>) {
    if render_state_debug_enabled() {
        debug!("[render-state][content] {}", message.as_ref());
    }
}

fn maybe_log_input_layout_debug(document_id: DocumentId, document: &BaseDocument) {
    if !input_debug_enabled() {
        return;
    }

    let mut logged_documents = LOGGED_INPUT_LAYOUT_DOCUMENTS
        .lock()
        .expect("input layout debug mutex poisoned");
    if !logged_documents.insert(document_id) {
        return;
    }

    let mut interesting_nodes = Vec::new();
    document.visit(|node_id, node| {
        let Some(element) = node.element_data() else {
            return;
        };
        let Some(id) = element.id.as_ref().map(|id| id.as_ref()) else {
            return;
        };

        if matches!(id, "click-counter-button" | "click-count" | "signal-card") {
            interesting_nodes.push(node_id);
        }
    });

    if interesting_nodes.is_empty() {
        trace!(
            "[input-debug][layout] document={} interesting_nodes=none",
            document_id,
        );
        return;
    }

    trace!(
        "[input-debug][layout] document={} interesting_nodes={interesting_nodes:?}",
        document_id,
    );

    for node_id in interesting_nodes {
        document.debug_log_node(node_id);

        let mut ancestor_id = document.get_node(node_id).and_then(|node| node.parent);
        while let Some(current_id) = ancestor_id {
            document.debug_log_node(current_id);
            ancestor_id = document.get_node(current_id).and_then(|node| node.parent);
        }
    }
}

#[derive(Clone)]
struct ContentNetProvider {
    local_state: LocalContentStateRef,
    content_document_id: DocumentId,
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    content_command_sender: ipc::IpcSender<Command>,
}

impl NetProvider for ContentNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        match request.url.scheme() {
            "data" => match DataUrl::process(request.url.as_str()) {
                Ok(data_url) => match data_url.decode_to_vec() {
                    Ok((bytes, _fragment)) => {
                        handler.bytes(request.url.to_string(), Bytes::from(bytes));
                    }
                    Err(_error) => {}
                },
                Err(_error) => {}
            },
            _scheme => {
                let handler_id = new_document_fetch_id();
                let mut local_state = self
                    .local_state
                    .lock()
                    .expect("local content state mutex poisoned");
                local_state.pending_handlers.insert(
                    handler_id,
                    PendingNetworkHandler::Resource {
                        document_id: self.content_document_id,
                        request_url: request.url.to_string(),
                        handler,
                    },
                );
                drop(local_state);

                let fetch_request = ContentFetchRequest {
                    handler_id,
                    url: request.url.to_string(),
                    method: request.method.to_string(),
                    body: request_body_string(&request.body),
                };
                let network_request = ipc_messages::network::Request::Fetch {
                    request_id: uuid::Uuid::new_v4(),
                    request: fetch_request,
                    reply_to: ipc_messages::network::ResponseRecipient::ContentProcess {
                        content_command_sender: self.content_command_sender.clone(),
                        handler_id,
                    },
                };
                if let Err(error) = self.network_extension_sender.send(network_request) {
                    error!("failed to send direct fetch request to net: {error}");
                }
            }
        }
    }
}

pub(crate) struct ContentDocument {
    traversable_id: NavigableId,
    parent_traversable_id: Option<NavigableId>,
    top_level_traversable_id: NavigableId,
    frame_id: FrameId,
    document: Rc<RefCell<BaseDocument>>,
    settings: EnvironmentSettingsObject,
    // Latched while an update-the-rendering attempt is queued or waiting on critical resources.
    pending_update_the_rendering: bool,
    pending_document_load: Option<PendingDocumentLoad>,
    navigable_container_states: HashMap<usize, NavigableContainerState>,
    viewport_offset_x: f32,
    viewport_offset_y: f32,
}

#[derive(Clone)]
struct DocumentViewportState {
    snapshot: ViewportSnapshot,
    offset_x: f32,
    offset_y: f32,
}

pub(crate) struct ContentProcess {
    event_sender: ipc::IpcSender<ContentEvent>,
    event_loop_id: EventLoopId,
    local_state: LocalContentStateRef,
    default_viewport: Option<ViewportSnapshot>,
    traversable_viewports: HashMap<NavigableId, DocumentViewportState>,
    documents: HashMap<DocumentId, ContentDocument>,
    active_documents_by_traversable: HashMap<NavigableId, DocumentId>,
    font_namespace: u64,
    font_sender: FontTransportSender,
    navigation_tracer: TLATracer,
    /// Shared clipboard cache. The embedder writes prefetched clipboard text
    /// here before dispatching paste events; `ShellProvider::get_clipboard_text`
    /// reads from this cache instead of doing a blocking IPC round-trip.
    clipboard_cache: ClipboardCache,
    /// Shared registry for traversable documents created during JS execution
    /// (window.open).  ContentProcess holds one Rc, and before running JS it
    /// sets a clone on the source document's GlobalScope so that
    /// `register_new_traversable_document` can insert directly into this map.
    new_document_registry:
        Rc<RefCell<HashMap<DocumentId, (EnvironmentSettingsObject, Rc<RefCell<BaseDocument>>)>>>,

    /// Consolidated wasm content-process state (worker + pending tracking).
    #[cfg(all(boa_backend, feature = "wasm"))]
    wasm: crate::wasm::ContentWasmState,

    video_paint_registry: Rc<RefCell<HashMap<(DocumentId, usize), VideoPaintId>>>,
    /// Direct sender to the net extension. Set during DirectChannelsSetup.
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    /// Direct sender to the media extension. Set during ContentBootstrap.
    media_extension_sender: Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
    /// This content process's own command sender, used by net for direct response routing.
    content_command_sender: ipc::IpcSender<Command>,
}

impl ContentProcess {
    fn new(
        event_sender: ipc::IpcSender<ContentEvent>,
        _wasm_signal_sender: crossbeam_channel::Sender<()>,
        event_loop_id: EventLoopId,
        network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
        media_extension_sender: Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
        content_command_sender: ipc::IpcSender<Command>,
        trace_sender: Option<TraceSender>,
    ) -> Self {
        let clipboard_cache = new_clipboard_cache();
        Self {
            event_sender,
            event_loop_id,
            local_state: Arc::new(Mutex::new(LocalContentState {
                pending_handlers: HashMap::new(),
            })),
            default_viewport: None,
            traversable_viewports: HashMap::new(),
            documents: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
            font_namespace: new_font_namespace(),
            font_sender: FontTransportSender::default(),
            navigation_tracer: TLATracer::new("Navigation", "formal-web:content", trace_sender),
            clipboard_cache: clipboard_cache.clone(),
            new_document_registry: Rc::new(RefCell::new(HashMap::new())),
            video_paint_registry: Rc::new(RefCell::new(HashMap::new())),
            #[cfg(all(boa_backend, feature = "wasm"))]
            wasm: crate::wasm::ContentWasmState::new(_wasm_signal_sender),
            network_extension_sender,
            media_extension_sender,
            content_command_sender,
        }
    }

    /// Set the clipboard cache from a prefetched clipboard text.
    /// Called before dispatching paste events.
    fn set_clipboard_cache(&self, text: Option<String>) {
        if let Ok(mut cache) = self.clipboard_cache.lock() {
            *cache = text;
        }
    }

    fn document_viewport_state(
        &self,
        traversable_id: NavigableId,
    ) -> Option<DocumentViewportState> {
        self.traversable_viewports
            .get(&traversable_id)
            .cloned()
            .or_else(|| {
                self.default_viewport
                    .as_ref()
                    .cloned()
                    .map(|snapshot| DocumentViewportState {
                        snapshot,
                        offset_x: 0.0,
                        offset_y: 0.0,
                    })
            })
    }

    fn document_config(
        &self,
        traversable_id: NavigableId,
        document_id: DocumentId,
        base_url: Option<String>,
    ) -> DocumentConfig {
        DocumentConfig {
            viewport: self
                .document_viewport_state(traversable_id)
                .map(|viewport| viewport_of_snapshot(&viewport.snapshot)),
            base_url,
            net_provider: Some(Arc::new(ContentNetProvider {
                local_state: Arc::clone(&self.local_state),
                content_document_id: document_id,
                network_extension_sender: self.network_extension_sender.clone(),
                content_command_sender: self.content_command_sender.clone(),
            })),
            shell_provider: Some(Arc::new(ContentShellProvider::new(
                self.event_sender.clone(),
                self.clipboard_cache.clone(),
            ))),
            html_parser_provider: Some(Arc::new(JsHtmlParserProvider)),
            ..DocumentConfig::default()
        }
    }

    fn set_viewport(&mut self, viewport: ViewportSnapshot) {
        self.default_viewport = Some(viewport);
    }

    fn set_traversable_viewport(&mut self, viewport: TraversableViewport) -> Result<(), String> {
        let traversable_id = viewport.traversable_id;
        let viewport_state = DocumentViewportState {
            snapshot: viewport.viewport,
            offset_x: viewport.offset_x,
            offset_y: viewport.offset_y,
        };
        self.traversable_viewports
            .insert(traversable_id, viewport_state.clone());

        let active_document_id = self
            .active_documents_by_traversable
            .get(&traversable_id)
            .copied();
        log_render_state_debug(format!(
            "set traversable viewport traversable={} document={:?} size=({}, {}) scale={} offset=({}, {})",
            traversable_id,
            active_document_id,
            viewport_state.snapshot.width,
            viewport_state.snapshot.height,
            viewport_state.snapshot.scale,
            viewport_state.offset_x,
            viewport_state.offset_y,
        ));

        let Some(document_id) = self
            .active_documents_by_traversable
            .get(&traversable_id)
            .copied()
        else {
            return Ok(());
        };
        let Some(document) = self.documents.get_mut(&document_id) else {
            return Ok(());
        };

        document
            .document
            .borrow_mut()
            .set_viewport(viewport_of_snapshot(&viewport_state.snapshot));
        document.viewport_offset_x = viewport_state.offset_x;
        document.viewport_offset_y = viewport_state.offset_y;

        // Repaint immediately so embed-site geometry (including iframe clip/transform)
        // reflects the new viewport instead of waiting for a later scroll/input tick.
        self.request_render_update(traversable_id, document_id, "traversable_viewport")
    }

    fn register_pending_handler(
        &self,
        pending_handler: PendingNetworkHandler,
    ) -> Result<DocumentFetchId, String> {
        let handler_id = new_document_fetch_id();
        let mut local_state = self
            .local_state
            .lock()
            .expect("local content state mutex poisoned");
        local_state
            .pending_handlers
            .insert(handler_id, pending_handler);
        Ok(handler_id)
    }

    fn request_remote_fetch(
        &self,
        handler_id: DocumentFetchId,
        request: Request,
    ) -> Result<(), String> {
        log_render_state_debug(format!(
            "request remote fetch handler={} method={} url={}",
            handler_id, request.method, request.url,
        ));
        let fetch_request = ContentFetchRequest {
            handler_id,
            url: request.url.to_string(),
            method: request.method.to_string(),
            body: request_body_string(&request.body),
        };
        let network_request = ipc_messages::network::Request::Fetch {
            request_id: uuid::Uuid::new_v4(),
            request: fetch_request,
            reply_to: ipc_messages::network::ResponseRecipient::ContentProcess {
                content_command_sender: self.content_command_sender.clone(),
                handler_id,
            },
        };
        self.network_extension_sender
            .send(network_request)
            .map_err(|error| format!("failed to send document fetch request to net: {error}"))
    }

    fn deferred_script_state(script: PendingParserScript) -> DeferredScriptState {
        match script {
            PendingParserScript::Inline { source } => DeferredScriptState::Inline { source },
            PendingParserScript::External { src } => DeferredScriptState::ExternalPending { src },
        }
    }

    fn mark_deferred_script_failed(&mut self, document_id: DocumentId, script_index: usize) {
        let Some(content_document) = self.documents.get_mut(&document_id) else {
            return;
        };
        let Some(pending_document_load) = content_document.pending_document_load.as_mut() else {
            return;
        };
        let Some(script) = pending_document_load.scripts.get_mut(script_index) else {
            return;
        };
        let failed_src = match script {
            DeferredScriptState::ExternalPending { src }
            | DeferredScriptState::ExternalFailed { src } => src.clone(),
            DeferredScriptState::Inline { .. } | DeferredScriptState::ExternalReady { .. } => {
                return;
            }
        };
        *script = DeferredScriptState::ExternalFailed { src: failed_src };
    }

    fn complete_deferred_script_fetch(
        &mut self,
        document_id: DocumentId,
        script_index: usize,
        body: Vec<u8>,
    ) {
        let Some(content_document) = self.documents.get_mut(&document_id) else {
            return;
        };
        let Some(pending_document_load) = content_document.pending_document_load.as_mut() else {
            return;
        };
        let Some(script) = pending_document_load.scripts.get_mut(script_index) else {
            return;
        };
        if matches!(script, DeferredScriptState::ExternalPending { .. }) {
            *script = DeferredScriptState::ExternalReady {
                source: String::from_utf8_lossy(&body).into_owned(),
            };
        }
    }

    fn start_deferred_script_fetch(
        &mut self,
        document_id: DocumentId,
        script_index: usize,
        src: &str,
    ) -> Result<(), String> {
        let creation_url = self
            .documents
            .get(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?
            .settings
            .creation_url
            .clone();
        let resolved_url = creation_url
            .join(src)
            .map_err(|error| format!("failed to resolve deferred script URL `{src}`: {error}"))?;

        if resolved_url.scheme() == "data" {
            let (bytes, _fragment) = DataUrl::process(resolved_url.as_str())
                .map_err(|error| format!("failed to decode deferred data script URL: {error}"))?
                .decode_to_vec()
                .map_err(|error| format!("failed to read deferred data script body: {error}"))?;
            self.complete_deferred_script_fetch(document_id, script_index, bytes);
            return Ok(());
        }

        let handler_id = self.register_pending_handler(PendingNetworkHandler::DeferredScript {
            document_id,
            script_index,
        })?;
        self.request_remote_fetch(handler_id, Request::get(resolved_url))
    }

    fn allocate_navigable_id(&self) -> Result<NavigableId, String> {
        Ok(NavigableId::new())
    }

    fn allocate_child_frame_id(&self) -> FrameId {
        FrameId::new()
    }

    /// Set the shared new-document registry on the source document's GlobalScope
    /// so that `the_rules_for_choosing_a_navigable` can register documents created
    /// during JS execution (window.open).
    fn set_up_new_document_registry(&mut self, traversable_id: NavigableId) -> Result<(), String> {
        let document_id = *self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable {traversable_id}"))?;
        let registry = Rc::clone(&self.new_document_registry);
        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document {document_id}"))?;
        with_global_scope(content_document.settings.ec(), |global_scope| {
            global_scope.set_new_document_registry(registry);
            Ok(())
        })
        .map_err(|error| format!("failed to set new document registry: {}", error.display()))
    }

    /// Clear the shared new-document registry from the source document's
    /// GlobalScope after JS execution completes.
    fn tear_down_new_document_registry(
        &mut self,
        traversable_id: NavigableId,
    ) -> Result<(), String> {
        let document_id = *self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable {traversable_id}"))?;
        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document {document_id}"))?;
        with_global_scope(content_document.settings.ec(), |global_scope| {
            global_scope.clear_new_document_registry();
            Ok(())
        })
        .map_err(|error| format!("failed to clear new document registry: {}", error.display()))
    }

    /// Drain any newly-created traversable documents from the shared registry
    /// into `self.documents`.  Called after each JS execution that may have
    /// invoked window.open.
    ///
    /// <https://html.spec.whatwg.org/#creating-a-new-auxiliary-browsing-context>
    fn drain_new_traversable_documents(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut *self.new_document_registry.borrow_mut());
        if pending.is_empty() {
            return Ok(());
        }
        let frame_id = FrameId::new();
        let parent_traversable_id = None;
        let top_level_traversable_id = NavigableId::new();

        for (document_id, (mut settings, document)) in pending {
            if self.documents.contains_key(&document_id) {
                continue;
            }
            // Read the traversable_id from the new document's own GlobalScope.
            let new_traversable_id = with_global_scope(settings.ec(), |global_scope| {
                Ok(global_scope.source_navigable_id())
            })
            .map_err(|error| format!("failed to read new traversable id: {}", error.display()))?
            .unwrap_or_else(NavigableId::new);

            self.documents.insert(
                document_id,
                ContentDocument {
                    traversable_id: new_traversable_id,
                    parent_traversable_id,
                    top_level_traversable_id,
                    frame_id,
                    document,
                    settings,
                    pending_update_the_rendering: false,
                    pending_document_load: None,
                    navigable_container_states: HashMap::new(),
                    viewport_offset_x: 0.0,
                    viewport_offset_y: 0.0,
                },
            );
            self.active_documents_by_traversable
                .insert(new_traversable_id, document_id);
        }
        Ok(())
    }

    /// Create and register an about:blank document for a child navigable.
    /// Uses the shared `create_about_blank_document` helper.  Called by
    /// `create_a_new_child_navigable` to create the document in the content
    /// process immediately, since we are already in the correct browsing
    /// context group.

    /// <https://html.spec.whatwg.org/multipage/#navigate-html>
    fn continue_document_load(&mut self, document_id: DocumentId) -> Result<(), String> {
        let (ready_to_finish, traversable_id, resources_ready, scripts_ready) = {
            let content_document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;
            let traversable_id = content_document.traversable_id;

            content_document.document.borrow_mut().handle_messages();
            let resources_ready = !content_document
                .document
                .borrow()
                .has_pending_critical_resources();

            let Some(pending_document_load) = content_document.pending_document_load.as_mut()
            else {
                return Ok(());
            };

            let scripts_ready = pending_document_load
                .scripts
                .iter()
                .all(|script| !matches!(script, DeferredScriptState::ExternalPending { .. }));
            (
                resources_ready && scripts_ready,
                traversable_id,
                resources_ready,
                scripts_ready,
            )
        };

        if !ready_to_finish {
            log_render_state_debug(format!(
                "defer document load completion document={} traversable={} resources_ready={} scripts_ready={}",
                document_id, traversable_id, resources_ready, scripts_ready,
            ));
            return Ok(());
        }

        let pending_document_load = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?
            .pending_document_load
            .take()
            .ok_or_else(|| format!("missing pending document load for document {document_id}"))?;

        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;

        for (script_idx, script) in pending_document_load.scripts.iter().enumerate() {
            match script {
                DeferredScriptState::Inline { source }
                | DeferredScriptState::ExternalReady { source } => {
                    if let Err(error) = content_document.settings.evaluate_script(source) {
                        error!("[deferred eval #{script_idx}] content error: {error}");
                    }
                }
                DeferredScriptState::ExternalPending { .. }
                | DeferredScriptState::ExternalFailed { .. } => {}
            }
        }

        let window = content_document
            .settings
            .realm_execution_context
            .realm_global_object();
        let time_millis = content_document.settings.current_time_millis();
        let ec = &mut content_document.settings.realm_execution_context;

        let window_target = ec
            .with_object_any(&window)
            .and_then(|data| data.downcast_ref::<crate::html::Window>())
            .map(|w| w.get_event_target())
            .ok_or_else(|| {
                let msg = "failed to extract EventTarget from Window".to_string();
                log::error!("{msg}");
                msg
            })?;

        fire_event(ec, &window_target, "load", time_millis, true)
            .map_err(|error| format!("fire_event failed: {error:?}"))?;

        let traversable_id = content_document.traversable_id;
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
        run_iframe_load_event_steps_for_traversable(self, traversable_id)?;
        log_render_state_debug(format!(
            "finalize document load document={} traversable={} url={}",
            document_id, traversable_id, pending_document_load.finalize_url,
        ));

        self.event_sender
            .send(ContentEvent::FinalizeNavigation(
                ipc_messages::content::FinalizeNavigation {
                    document_id,
                    url: pending_document_load.finalize_url,
                },
            ))
            .map_err(|error| format!("failed to send finalize-navigation event: {error}"))?;

        self.update_the_rendering(traversable_id, document_id)
    }

    /// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>
    fn create_empty_document(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
        frame_id: Option<FrameId>,
        parent_traversable_id: Option<NavigableId>,
        top_level_traversable_id: NavigableId,
    ) -> Result<(), String> {
        let viewport_state = self.document_viewport_state(traversable_id);
        let frame_id = frame_id.unwrap_or_else(FrameId::new);
        let document = Rc::new(RefCell::new(BaseDocument::new(self.document_config(
            traversable_id,
            document_id,
            None,
        ))));
        let mut settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse("about:blank").map_err(|error| error.to_string())?,
            Some(self.event_sender.clone()),
            Some(traversable_id),
            Some(document_id),
        )?;

        // Set the video-paint registry on GlobalScope so that
        // resource_selection_algorithm can register paint IDs.
        if let Err(error) = with_global_scope(settings.ec(), |global_scope| {
            global_scope.set_video_paint_registry(Rc::clone(&self.video_paint_registry));
            if let Some(ref sender) = self.media_extension_sender {
                global_scope.set_media_extension_sender(sender.clone());
            }
            Ok(())
        }) {
            error!(
                "[media] failed to set video paint registry on GlobalScope: {}",
                error.display()
            );
        }

        // This block continues <https://html.spec.whatwg.org/#creating-a-new-browsing-context>.
        // Step 7: "Mark document as ready for post-load tasks."
        // TODO: Persist the document's post-load readiness state in the DOM model.

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 8: "Populate with html/head/body given document."
            // The content process drives the shared HTML parser with a fixed `about:blank` skeleton.
            parse_html_into_document(&mut document_guard, EMPTY_HTML_DOCUMENT)
        };

        // Step 10: "Completely finish loading document."
        // Execute parser-discovered classic scripts after the initial tree build.
        // TODO: Model the rest of the `completely finish loading` bookkeeping explicitly instead of relying on parser-discovered script execution alone.
        // Step 9: "Make active document."
        // Records the document as addressable under `document_id` after init completes.
        self.documents.insert(
            document_id,
            ContentDocument {
                traversable_id,
                parent_traversable_id,
                top_level_traversable_id,
                frame_id,
                document,
                settings,
                pending_update_the_rendering: false,
                pending_document_load: None,
                navigable_container_states: HashMap::new(),
                viewport_offset_x: viewport_state
                    .as_ref()
                    .map(|viewport| viewport.offset_x)
                    .unwrap_or(0.0),
                viewport_offset_y: viewport_state
                    .as_ref()
                    .map(|viewport| viewport.offset_y)
                    .unwrap_or(0.0),
            },
        );
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);

        // Set the navigable hierarchy on the GlobalScope so that `window.open`
        // can resolve `_parent`/`_top` targets.
        self.set_navigable_hierarchy_on_global_scope(document_id)?;

        run_dom_post_connection_steps_for_document(self, document_id)?;
        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        execute_parser_scripts(&mut content_document.settings, parser_scripts)?;
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#navigate-html>
    fn create_loaded_document(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
        frame_id: Option<FrameId>,
        response: LoadedDocumentResponse,
        parent_traversable_id: Option<NavigableId>,
        top_level_traversable_id: NavigableId,
    ) -> Result<(), String> {
        let LoadedDocumentResponse {
            final_url,
            status: _,
            content_type: _,
            body,
        } = response;
        let viewport_state = self.document_viewport_state(traversable_id);
        let frame_id = frame_id.unwrap_or_else(FrameId::new);
        // This block continues <https://html.spec.whatwg.org/#navigate-html>.
        // Step 1: "Let document be the result of creating and initializing a `Document` object given `html`, `text/html`, and navigationParams."
        // BaseDocument::new and EnvironmentSettingsObject::new split document creation.
        let document = Rc::new(RefCell::new(BaseDocument::new(self.document_config(
            traversable_id,
            document_id,
            Some(final_url.clone()),
        ))));
        let mut settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse(&final_url).map_err(|error| error.to_string())?,
            Some(self.event_sender.clone()),
            Some(traversable_id),
            Some(document_id),
        )?;

        // Set the video-paint registry on GlobalScope so that
        // resource_selection_algorithm can register paint IDs.
        if let Err(error) = with_global_scope(settings.ec(), |global_scope| {
            global_scope.set_video_paint_registry(Rc::clone(&self.video_paint_registry));
            if let Some(ref sender) = self.media_extension_sender {
                global_scope.set_media_extension_sender(sender.clone());
            }
            Ok(())
        }) {
            error!(
                "[media] failed to set video paint registry on GlobalScope: {}",
                error.display()
            );
        }

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 3: "Otherwise, create an HTML parser and associate it with the document."
            // The embedder has buffered the response body; feed into parser immediately.
            parse_html_into_document(&mut document_guard, &body)
        };

        let deferred_scripts = parser_scripts
            .into_iter()
            .map(Self::deferred_script_state)
            .collect::<Vec<_>>();

        self.documents.insert(
            document_id,
            ContentDocument {
                traversable_id,
                parent_traversable_id,
                top_level_traversable_id,
                frame_id,
                document: Rc::clone(&document),
                settings,
                pending_update_the_rendering: false,
                pending_document_load: Some(PendingDocumentLoad {
                    finalize_url: final_url.clone(),
                    scripts: deferred_scripts,
                }),
                navigable_container_states: HashMap::new(),
                viewport_offset_x: viewport_state
                    .as_ref()
                    .map(|viewport| viewport.offset_x)
                    .unwrap_or(0.0),
                viewport_offset_y: viewport_state
                    .as_ref()
                    .map(|viewport| viewport.offset_y)
                    .unwrap_or(0.0),
            },
        );
        attach_same_origin_child_document_for_traversable(self, traversable_id)?;

        // Set the navigable hierarchy on the GlobalScope so that `window.open`
        // can resolve `_parent`/`_top` targets.
        let _ = self.set_navigable_hierarchy_on_global_scope(document_id);

        run_dom_post_connection_steps_for_document(self, document_id)?;

        let deferred_fetches = self
            .documents
            .get(&document_id)
            .and_then(|content_document| content_document.pending_document_load.as_ref())
            .map(|pending_document_load| {
                pending_document_load
                    .scripts
                    .iter()
                    .enumerate()
                    .filter_map(|(script_index, script)| match script {
                        DeferredScriptState::ExternalPending { src } => {
                            Some((script_index, src.clone()))
                        }
                        DeferredScriptState::Inline { .. }
                        | DeferredScriptState::ExternalReady { .. }
                        | DeferredScriptState::ExternalFailed { .. } => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for (script_index, src) in deferred_fetches {
            if let Err(error) = self.start_deferred_script_fetch(document_id, script_index, &src) {
                error!("[deferred fetch] content error: {error}");
                self.mark_deferred_script_failed(document_id, script_index);
            }
        }

        self.continue_document_load(document_id)
    }

    fn evaluate_script(
        &mut self,
        traversable_id: NavigableId,
        source: String,
    ) -> Result<serde_json::Value, String> {
        // Set up shared registry so window.open can register new documents.
        if let Err(error) = self.set_up_new_document_registry(traversable_id) {
            warn!("failed to set up new document registry: {error}");
        }

        let document_id = *self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        let result = document.settings.evaluate_script_to_json(&source);

        // Tear down and drain any documents created during JS execution.
        if let Err(error) = self.tear_down_new_document_registry(traversable_id) {
            warn!("failed to tear down new document registry: {error}");
        }
        if let Err(error) = self.drain_new_traversable_documents() {
            warn!("failed to drain new traversable documents: {error}");
        }

        result
    }

    fn click_element(
        &mut self,
        traversable_id: NavigableId,
        selector: String,
    ) -> Result<(), String> {
        // Set up shared registry so window.open can register new documents.
        if let Err(error) = self.set_up_new_document_registry(traversable_id) {
            warn!("failed to set up new document registry: {error}");
        }

        let document_id = *self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        let target_node_id = {
            let document_guard = document.document.borrow();
            document_guard
                .query_selector(&selector)
                .map_err(|error| format!("invalid selector `{selector}`: {error:?}"))?
        }
        .ok_or_else(|| format!("no element matched selector `{selector}`"))?;

        dispatch_trusted_click_event(
            document_id,
            document.traversable_id,
            document.parent_traversable_id,
            document.top_level_traversable_id,
            Rc::clone(&document.document),
            &mut document.settings,
            &self.event_sender,
            target_node_id,
        )
    }

    fn destroy_document(&mut self, document_id: DocumentId) -> Result<(), String> {
        run_dom_removing_steps_for_document(self, document_id)?;
        if let Some(mut content_document) = self.documents.remove(&document_id) {
            if self
                .active_documents_by_traversable
                .get(&content_document.traversable_id)
                .is_some_and(|current_document_id| *current_document_id == document_id)
            {
                self.active_documents_by_traversable
                    .remove(&content_document.traversable_id);
            }
            if let Err(error) = content_document.settings.clear_all_window_timers() {
                error!("failed to clear window timers during document teardown: {error}");
            }
        }
        #[cfg(all(boa_backend, feature = "wasm"))]
        {
            // Clean up any pending wasm requests for this document so that
            // worker results arriving after destruction are not misattributed,
            // and to avoid orphaned promise entries.
            // https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module
            self.wasm
                .pending_requests
                .retain(|_request_id, doc_id| *doc_id != document_id);
            self.wasm.pending_modules.retain(|request_id, _module| {
                !self.wasm.pending_requests.contains_key(request_id)
                    || self.wasm.pending_requests.get(request_id) != Some(&document_id)
            });
        }
        let mut local_state = self
            .local_state
            .lock()
            .expect("local content state mutex poisoned");
        local_state
            .pending_handlers
            .retain(|_, pending_handler| match pending_handler {
                PendingNetworkHandler::Resource {
                    document_id: pending_document_id,
                    ..
                }
                | PendingNetworkHandler::DeferredScript {
                    document_id: pending_document_id,
                    ..
                } => *pending_document_id != document_id,
            });
        Ok(())
    }

    fn dispatch_events(&mut self, events: Vec<DispatchEventEntry>) -> Result<(), String> {
        for DispatchEventEntry {
            document_id,
            event,
            prefetched_clipboard_text,
        } in events
        {
            // Store prefetched clipboard text before dispatching the event
            // so that `ShellProvider::get_clipboard_text` can return it
            // without a blocking IPC round-trip.
            self.set_clipboard_cache(prefetched_clipboard_text);

            // Extract traversable_id before borrowing self.documents.
            let traversable_id = self
                .documents
                .get(&document_id)
                .map(|doc| doc.traversable_id)
                .unwrap_or(NavigableId::new());

            // Set up shared registry so window.open can register new documents
            // (same as click_element does).
            if let Err(error) = self.set_up_new_document_registry(traversable_id) {
                warn!("failed to set up new document registry for UI event: {error}");
            }

            let Some(document) = self.documents.get_mut(&document_id) else {
                continue;
            };

            {
                let document_guard = document.document.borrow();
                maybe_log_input_layout_debug(document_id, &document_guard);
            }

            // Continues <https://dom.spec.whatwg.org/#concept-event-fire> after the
            // user agent writes the serialized UI event batch to the content process.
            let event = deserialize_ui_event(&event)?;
            dispatch_ui_event(
                document_id,
                traversable_id,
                document.parent_traversable_id,
                document.top_level_traversable_id,
                Rc::clone(&document.document),
                &mut document.settings,
                &self.event_sender,
                document.viewport_offset_x,
                document.viewport_offset_y,
                event,
            )?;

            if let Err(error) = self.tear_down_new_document_registry(traversable_id) {
                warn!("failed to tear down new document registry: {error}");
            }
            if let Err(error) = self.drain_new_traversable_documents() {
                warn!("failed to drain new traversable documents: {error}");
            }
        }

        Ok(())
    }

    fn run_before_unload(
        &mut self,
        document_id: DocumentId,
        check_id: BeforeUnloadCheckId,
        navigation_id: NavigationId,
    ) -> Result<(), String> {
        let (navigable_id, canceled) = if let Some(document) = self.documents.get_mut(&document_id)
        {
            let navigable_id = document.traversable_id;
            let time_millis = document.settings.current_time_millis();
            let canceled = !crate::html::dispatch::fire_global_event(
                &mut document.settings.realm_execution_context,
                "beforeunload",
                true,
                time_millis,
            )
            .map_err(|error| format!("fire_global_event failed: {error:?}"))?;
            (Some(navigable_id), canceled)
        } else {
            (None, false)
        };
        if let Some(navigable_id) = navigable_id {
            let outcome = if canceled { "Aborted" } else { "Approved" };
            verification::tla_log!(
                self.navigation_tracer,
                "RunBeforeUnload",
                navigable_id,
                navigation_id,
                outcome
            );
        }
        self.event_sender
            .send(ContentEvent::BeforeUnloadCompleted(
                ipc_messages::content::BeforeUnloadResult {
                    document_id,
                    check_id,
                    canceled,
                },
            ))
            .map_err(|error| format!("failed to send beforeunload completion: {error}"))
    }

    fn update_the_rendering(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
    ) -> Result<(), String> {
        self.request_render_update(traversable_id, document_id, "command")
    }

    fn request_render_update(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
        reason: &str,
    ) -> Result<(), String> {
        let Some(document) = self.documents.get_mut(&document_id) else {
            return Ok(());
        };
        log_render_state_debug(format!(
            "queue update-the-rendering traversable={} document={} reason={}",
            traversable_id, document_id, reason,
        ));
        document.pending_update_the_rendering = true;
        self.continue_updating_the_rendering(traversable_id, document_id)
    }

    /// <https://html.spec.whatwg.org/#update-the-rendering>
    fn continue_updating_the_rendering(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
    ) -> Result<(), String> {
        let event_sender = self.event_sender.clone();
        let video_paint_registry = Rc::clone(&self.video_paint_registry);
        let paint_frame = {
            let document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;

            if !document.pending_update_the_rendering {
                log_render_state_debug(format!(
                    "skip paint no pending render traversable={} document={}",
                    traversable_id, document_id,
                ));
                return Ok(());
            }

            document.document.borrow_mut().handle_messages();

            if document.document.borrow().has_pending_critical_resources() {
                log_render_state_debug(format!(
                    "skip paint pending critical resources traversable={} document={}",
                    traversable_id, document_id,
                ));
                return Ok(());
            }

            document.pending_update_the_rendering = false;

            let frame_timestamp_ms = document.settings.current_time_millis();

            // Step 1: "Let `frameTimestamp` be `eventLoop`'s last render opportunity time."
            // Note: The content process currently derives a monotonic frame timestamp from the document's environment settings object time origin instead of the HTML event loop's shared render-opportunity clock.

            // Step 14: "For each `doc` of `docs`, run the animation frame callbacks for `doc`, passing in the relative high resolution time given `frameTimestamp` and `doc`'s relevant global object as the timestamp."
            // Note: The content process collapses `docs` to the single active document for this content process and uses the same environment-relative time as both the HTML frame timestamp and the callback timestamp.
            document
                .settings
                .run_animation_frame_callbacks(frame_timestamp_ms)?;

            let animation_time = frame_timestamp_ms / 1000.0;
            {
                let mut document_guard = document.document.borrow_mut();

                // Step 16.2.1: "Recalculate styles and update layout for `doc`."
                // `resolve` advances style, layout, and resource-driven document updates.
                document_guard.resolve(animation_time);
            }

            let paint_frame = {
                let document_guard = document.document.borrow();
                let viewport = document_guard.viewport().clone();
                let (width, height) = viewport.window_size;
                let mut scene = RenderScene::new();
                let composition = Self::build_frame_composition_metadata(
                    document_id,
                    &document_guard,
                    &document.navigable_container_states,
                    viewport.scale_f64(),
                    &mut video_paint_registry.borrow_mut(),
                );

                // Step 22: "For each `doc` of `docs`, update the rendering or user interface of `doc` and its node navigable to reflect the current state."
                // Note: This implementation collapses the HTML rendering task to a single active document and records the painted scene for the embedder.
                paint_scene(
                    &mut scene,
                    &document_guard,
                    viewport.scale_f64(),
                    width,
                    height,
                    0,
                    0,
                );
                let mut next_shmem_key = 0usize;
                let scene =
                    self.font_sender
                        .prepare_scene(self.font_namespace, scene, &mut next_shmem_key);
                log_render_state_debug(format!(
                    "emit paint traversable={} document={} size=({}, {})",
                    traversable_id, document_id, width, height,
                ));
                let (paint_frame, shmem_data) = PaintFrame::new(
                    WebviewId(traversable_id),
                    document.frame_id,
                    width,
                    height,
                    composition,
                    scene,
                    &mut next_shmem_key,
                )?;
                (paint_frame, shmem_data)
            };
            paint_frame
        };

        let (paint_frame, shmem_map) = paint_frame;
        event_sender
            .send_with_shmem_map(ContentEvent::PaintReady(paint_frame), shmem_map)
            .map_err(|error| format!("failed to send paint frame: {error}"))
    }

    fn node_absolute_border_origin(
        document: &BaseDocument,
        node_id: usize,
        scale: f64,
    ) -> Option<(f64, f64)> {
        let mut x = -document.viewport_scroll().x * scale;
        let mut y = -document.viewport_scroll().y * scale;
        let mut current = Some(node_id);
        while let Some(id) = current {
            let node = document.get_node(id)?;
            x += (f64::from(node.final_layout.location.x) - node.scroll_offset.x) * scale;
            y += (f64::from(node.final_layout.location.y) - node.scroll_offset.y) * scale;
            current = node.parent;
        }
        Some((x, y))
    }

    fn content_box_for_node(
        document: &BaseDocument,
        node_id: usize,
        scale: f64,
    ) -> Option<(f64, f64, f64, f64)> {
        let node = document.get_node(node_id)?;
        let layout = node.final_layout;
        let edge = layout.padding + layout.border;
        debug!(
            "[layout] node {} layout size=({}, {}) padding+border=({},{},{},{}) scroll=({},{})",
            node_id,
            layout.size.width,
            layout.size.height,
            edge.left,
            edge.right,
            edge.top,
            edge.bottom,
            node.scroll_offset.x,
            node.scroll_offset.y
        );
        let (border_x, border_y) = Self::node_absolute_border_origin(document, node_id, scale)?;
        let x = border_x + f64::from(edge.left) * scale;
        let y = border_y + f64::from(edge.top) * scale;
        let width = (f64::from(layout.size.width) - f64::from(edge.left + edge.right)) * scale;
        let height = (f64::from(layout.size.height) - f64::from(edge.top + edge.bottom)) * scale;
        if width <= 0.0 || height <= 0.0 {
            debug!(
                "[layout] node {} skipped: computed size ({:.1},{:.1})",
                node_id, width, height
            );
            return None;
        }
        Some((x, y, width, height))
    }

    fn build_frame_composition_metadata(
        document_id: DocumentId,
        document: &BaseDocument,
        container_states: &HashMap<usize, NavigableContainerState>,
        scale: f64,
        video_paint_registry: &mut HashMap<(DocumentId, usize), VideoPaintId>,
    ) -> FrameCompositionMetadata {
        let mut iframe_node_ids = container_states
            .iter()
            .filter_map(|(iframe_node_id, state)| {
                state.cross_origin.then_some((*iframe_node_id, state))
            })
            .collect::<Vec<_>>();
        iframe_node_ids.sort_by_key(|(iframe_node_id, _)| *iframe_node_id);

        // Collect video node ids by scanning the document tree for <video> elements.
        let mut video_node_ids = Vec::new();
        document.visit(|node_id, node| {
            if let Some(element_data) = node.element_data() {
                if element_data.name.local == local_name!("video") {
                    video_node_ids.push(node_id);
                }
            }
        });

        // Build iframe embed sites.
        let iframe_count = iframe_node_ids.len();
        let video_count = video_node_ids.len();
        let mut embed_sites = Vec::with_capacity(iframe_count + video_count);

        for (paint_order, (iframe_node_id, state)) in iframe_node_ids.into_iter().enumerate() {
            let (x, y, width, height) =
                match Self::content_box_for_node(document, iframe_node_id, scale) {
                    Some(box_) => box_,
                    None => continue,
                };
            let clip_svg_path = format!("M0,0 L{width},0 L{width},{height} L0,{height} Z");
            embed_sites.push(EmbedSite::Frame(IframeEmbedSite {
                embed_site_id: EmbedSiteId((iframe_node_id as u64).wrapping_add(1)),
                child_frame_id: state.content_frame_id,
                background_policy: EmbedBackgroundPolicy::OpaqueWhite,
                clip_svg_path,
                layout: EmbedLayout {
                    z_index: 0,
                    paint_order: paint_order as u32,
                    transform: [1.0, 0.0, 0.0, 1.0, x, y],
                    clip_bounds: [x, y, x + width, y + height],
                },
            }));
        }

        // Build video embed sites.
        for (paint_offset, video_node_id) in video_node_ids.into_iter().enumerate() {
            let (x, y, width, height) = match Self::content_box_for_node(
                document,
                video_node_id,
                scale,
            ) {
                Some(box_) => box_,
                None => {
                    // Fallback: video element has 0x0 layout size (blitz doesn't natively
                    // size video elements). Compute position only and use a default size.
                    let fallback_w = 300.0 * scale;
                    let fallback_h = 150.0 * scale;
                    if let Some((bx, by)) =
                        Self::node_absolute_border_origin(document, video_node_id, scale)
                    {
                        debug!(
                            "[layout] video node {} fallback position=({:.0},{:.0}) size=({:.0},{:.0})",
                            video_node_id, bx, by, fallback_w, fallback_h
                        );
                        (bx, by, fallback_w, fallback_h)
                    } else {
                        debug!("[layout] video node {} skipped: no position", video_node_id);
                        continue;
                    }
                }
            };
            debug!(
                "[layout] video node {} embed site: pos=({:.0},{:.0}) size=({:.0},{:.0})",
                video_node_id, x, y, width, height
            );
            // Read border-radius from the element's computed style. This defaults to a
            // small rounded radius if available, otherwise 0 (rect clip). For simplicity,
            // we read from the style attribute — a full computed style lookup would be
            // more accurate but the border radius is typically small.
            let clip_radius = document
                .get_node(video_node_id)
                .and_then(|n| n.element_data())
                .and_then(|el| el.attr(local_name!("style")))
                .and_then(|style_str| {
                    // Look for border-radius in inline style: "border-radius: Npx" or "border-radius: Nrem"
                    let s = style_str.to_lowercase();
                    s.split(';')
                        .find(|part| part.trim().starts_with("border-radius"))
                        .and_then(|part| {
                            let val = part.split(':').nth(1)?.trim();
                            if val.ends_with("px") {
                                val.trim_end_matches("px")
                                    .parse::<f64>()
                                    .ok()
                                    .map(|v| v * scale)
                            } else if val.ends_with("rem") {
                                // rem is relative to root font-size (typically 16px)
                                val.trim_end_matches("rem")
                                    .parse::<f64>()
                                    .ok()
                                    .map(|v| v * 16.0 * scale)
                            } else {
                                None
                            }
                        })
                })
                .unwrap_or(0.0);

            let paint_id = video_paint_registry
                .entry((document_id, video_node_id))
                .or_insert_with(VideoPaintId::new);

            embed_sites.push(EmbedSite::Video(VideoEmbedData {
                paint_id: *paint_id,
                layout: EmbedLayout {
                    z_index: 0,
                    paint_order: (iframe_count + paint_offset) as u32,
                    transform: [1.0, 0.0, 0.0, 1.0, x, y],
                    clip_bounds: [x, y, x + width, y + height],
                },
                clip_radius,
            }));
        }

        FrameCompositionMetadata { embed_sites }
    }

    fn complete_document_fetch(
        &mut self,
        handler_id: DocumentFetchId,
        response: ContentFetchResponse,
    ) -> Result<(), String> {
        let response_url = response.final_url.clone();
        let response_status = response.status;
        let response_type = response.content_type.clone();
        let pending_handler = {
            let mut local_state = self
                .local_state
                .lock()
                .expect("local content state mutex poisoned");
            local_state.pending_handlers.remove(&handler_id)
        };

        let Some(pending_handler) = pending_handler else {
            return Ok(());
        };

        match pending_handler {
            PendingNetworkHandler::Resource {
                document_id,
                request_url: _,
                handler,
            } => {
                handler.bytes(
                    response.final_url.clone(),
                    Bytes::copy_from_slice(&response.body),
                );
                let Some(content_document) = self.documents.get(&document_id) else {
                    error!("[content] complete_document_fetch: document {document_id} not found");
                    return Ok(());
                };
                let traversable_id = content_document.traversable_id;
                log_render_state_debug(format!(
                    "complete resource fetch handler={} traversable={} document={} status={} type={} url={}",
                    handler_id,
                    traversable_id,
                    document_id,
                    response_status,
                    response_type,
                    response_url,
                ));
                self.continue_document_load(document_id)?;
                self.request_render_update(traversable_id, document_id, "resource_fetch_complete")?;
                Ok(())
            }
            PendingNetworkHandler::DeferredScript {
                document_id,
                script_index,
            } => {
                if deferred_script_response_is_executable(&response) {
                    self.complete_deferred_script_fetch(document_id, script_index, response.body);
                } else {
                    warn!(
                        "content deferred script rejected: url={} status={} content-type={}",
                        response.final_url, response.status, response.content_type,
                    );
                    self.mark_deferred_script_failed(document_id, script_index);
                }
                let Some(content_document) = self.documents.get(&document_id) else {
                    error!(
                        "[content] complete_document_fetch (deferred script): document {document_id} not found"
                    );
                    return Ok(());
                };
                let traversable_id = content_document.traversable_id;
                log_render_state_debug(format!(
                    "complete deferred-script fetch handler={} traversable={} document={} script_index={} status={} type={} url={}",
                    handler_id,
                    traversable_id,
                    document_id,
                    script_index,
                    response_status,
                    response_type,
                    response_url,
                ));
                self.continue_document_load(document_id)?;
                self.request_render_update(
                    traversable_id,
                    document_id,
                    "deferred_script_fetch_complete",
                )?;
                Ok(())
            }
        }
    }

    fn fail_document_fetch(&mut self, handler_id: DocumentFetchId) -> Result<(), String> {
        let pending_handler = {
            let mut local_state = self
                .local_state
                .lock()
                .expect("local content state mutex poisoned");
            local_state.pending_handlers.remove(&handler_id)
        };

        let Some(pending_handler) = pending_handler else {
            return Ok(());
        };

        match pending_handler {
            PendingNetworkHandler::Resource {
                document_id,
                request_url,
                handler,
            } => {
                handler.bytes(request_url, Bytes::new());
                let Some(content_document) = self.documents.get(&document_id) else {
                    error!("[content] fail_document_fetch: document {document_id} not found");
                    return Ok(());
                };
                let traversable_id = content_document.traversable_id;
                log_render_state_debug(format!(
                    "fail resource fetch handler={} traversable={} document={}",
                    handler_id, traversable_id, document_id,
                ));
                self.continue_document_load(document_id)?;
                self.request_render_update(traversable_id, document_id, "resource_fetch_failed")?;
                Ok(())
            }
            PendingNetworkHandler::DeferredScript {
                document_id,
                script_index,
            } => {
                self.mark_deferred_script_failed(document_id, script_index);
                let Some(content_document) = self.documents.get(&document_id) else {
                    error!(
                        "[content] fail_document_fetch (deferred script): document {document_id} not found"
                    );
                    return Ok(());
                };
                let traversable_id = content_document.traversable_id;
                log_render_state_debug(format!(
                    "fail deferred-script fetch handler={} traversable={} document={} script_index={}",
                    handler_id, traversable_id, document_id, script_index,
                ));
                self.continue_document_load(document_id)?;
                self.request_render_update(
                    traversable_id,
                    document_id,
                    "deferred_script_fetch_failed",
                )?;
                Ok(())
            }
        }
    }

    fn run_window_timer(
        &mut self,
        document_id: DocumentId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    ) -> Result<(), String> {
        let traversable_id = {
            let Some(document) = self.documents.get(&document_id) else {
                return Ok(());
            };
            document.traversable_id
        };
        if let Err(error) = self.set_up_new_document_registry(traversable_id) {
            warn!("failed to set up new document registry: {error}");
        }
        {
            let Some(document) = self.documents.get_mut(&document_id) else {
                return Ok(());
            };
            document
                .settings
                .run_window_timer(timer_id, timer_key, nesting_level)?;
        }
        if let Err(error) = self.tear_down_new_document_registry(traversable_id) {
            warn!("failed to tear down new document registry: {error}");
        }
        if let Err(error) = self.drain_new_traversable_documents() {
            warn!("failed to drain new traversable documents: {error}");
        }
        Ok(())
    }

    fn note_command_completed(&self) -> Result<(), String> {
        self.event_sender
            .send(ContentEvent::CommandCompleted)
            .map_err(|error| format!("failed to send content command completion: {error}"))
    }

    fn note_shutdown_completed(&self) -> Result<(), String> {
        self.event_sender
            .send(ContentEvent::ShutdownCompleted)
            .map_err(|error| format!("failed to send content shutdown completion: {error}"))
    }

    /// Set the navigable hierarchy on the GlobalScope so that `window.open`
    /// can resolve `_parent`/`_top` targets in
    /// `the_rules_for_choosing_a_navigable`.
    fn set_navigable_hierarchy_on_global_scope(
        &mut self,
        document_id: DocumentId,
    ) -> Result<(), String> {
        let Some(content_document) = self.documents.get_mut(&document_id) else {
            return Err(format!("unknown document id: {document_id}"));
        };
        let parent_traversable_id = content_document.parent_traversable_id;
        let top_level_traversable_id = content_document.top_level_traversable_id;
        with_global_scope(content_document.settings.ec(), |global_scope| {
            global_scope.set_navigable_hierarchy(parent_traversable_id, top_level_traversable_id);
            Ok(())
        })
        .map_err(|error| error.display().to_string())
    }

    /// Drain pending WebAssembly requests from all documents and submit
    /// them to the background worker.
    fn drain_all_pending_wasm_requests(&mut self) {
        #[cfg(all(boa_backend, feature = "wasm"))]
        {
            let document_ids: Vec<DocumentId> = self.documents.keys().copied().collect();

            for document_id in document_ids {
                let Some(content_document) = self.documents.get_mut(&document_id) else {
                    continue;
                };

                // Submit compile batches.
                let batches = content_document.settings.take_pending_wasm_batches();
                for (request_id, bytes) in batches {
                    self.wasm.pending_requests.insert(request_id, document_id);
                    self.wasm.worker.submit_compile(bytes, request_id);
                }

                // Submit instantiate requests.
                let instantiates = content_document.settings.take_pending_wasm_instantiates();
                for (request_id, module) in instantiates {
                    self.wasm.pending_requests.insert(request_id, document_id);
                    self.wasm.pending_modules.insert(request_id, module.clone());
                    self.wasm.worker.submit_instantiate(module, request_id);
                }
            }
        }
    }

    /// Drain completed wasm results from the shared queue.
    /// Called both at the end of `handle_command` and when the dedicated
    /// IPC signal fires.
    fn drain_wasm_results(&mut self) {
        #[cfg(all(boa_backend, feature = "wasm"))]
        {
            let completed: Vec<(u64, WasmResult)> = {
                let results = self.wasm.worker.drain_results();
                results
                    .into_iter()
                    .map(|result| {
                        let request_id = match &result {
                            WasmResult::Compiled { request_id, .. }
                            | WasmResult::CompileError { request_id, .. }
                            | WasmResult::Instantiated { request_id, .. }
                            | WasmResult::InstantiateError { request_id, .. } => *request_id,
                        };
                        (request_id, result)
                    })
                    .collect()
            };

            for (request_id, result) in completed {
                let Some(&document_id) = self.wasm.pending_requests.get(&request_id) else {
                    // This is expected when a document is destroyed before the
                    // worker finishes — the destroy_document cleanup removes the
                    // entry, and the worker's result arrives safely discarded.
                    continue;
                };

                let Some(content_document) = self.documents.get_mut(&document_id) else {
                    error!("WebAssembly: document {} not found", document_id);
                    self.wasm.pending_requests.remove(&request_id);
                    continue;
                };

                let Some((_promise, resolvers)) =
                    content_document.settings.consume_wasm_request(request_id)
                else {
                    error!(
                        "WebAssembly: request {} not found on document {}",
                        request_id, document_id
                    );
                    self.wasm.pending_requests.remove(&request_id);
                    continue;
                };

                match result {
                    WasmResult::Compiled {
                        request_id: _,
                        module,
                    } => {
                        if let Err(error) = compile_continuation(
                            &resolvers,
                            module,
                            Vec::new(),
                            content_document.settings.ec(),
                        ) {
                            error!(
                                "WebAssembly: failed to resolve compile promise: {}",
                                error.display()
                            );
                        }
                    }
                    WasmResult::CompileError {
                        request_id: _,
                        message,
                    } => {
                        if let Err(error) =
                            compile_rejection(&resolvers, message, content_document.settings.ec())
                        {
                            error!(
                                "WebAssembly: failed to reject compile promise: {}",
                                error.display()
                            );
                        }
                    }
                    WasmResult::Instantiated {
                        request_id: _,
                        store,
                        instance,
                    } => {
                        let module = self.wasm.pending_modules.remove(&request_id);
                        let Some(module) = module else {
                            error!(
                                "WebAssembly: no module found for instantiate request {}",
                                request_id
                            );
                            self.wasm.pending_requests.remove(&request_id);
                            continue;
                        };
                        if let Err(error) = instantiate_continuation(
                            &module,
                            &instance,
                            &store,
                            &resolvers,
                            content_document.settings.ec(),
                        ) {
                            error!(
                                "WebAssembly: failed to resolve instantiate promise: {}",
                                error.display()
                            );
                        }
                    }
                    WasmResult::InstantiateError {
                        request_id: _,
                        message,
                    } => {
                        if let Err(error) =
                            compile_rejection(&resolvers, message, content_document.settings.ec())
                        {
                            error!(
                                "WebAssembly: failed to reject instantiate promise: {}",
                                error.display()
                            );
                        }
                    }
                }

                self.wasm.pending_requests.remove(&request_id);
            }

            // Flush microtasks (promise .then() handlers) after resolving/rejecting.
            for document in self.documents.values_mut() {
                if let Err(error) = document.settings.perform_a_microtask_checkpoint() {
                    error!("WebAssembly: microtask checkpoint failed: {error}");
                }
            }
        }
    }

    /// <https://html.spec.whatwg.org/#event-loop-processing-model>
    fn handle_command(&mut self, command: Command) -> Result<bool, String> {
        let result = self.handle_command_inner(command);

        #[cfg(all(boa_backend, feature = "wasm"))]
        {
            // After every command, drain any pending WebAssembly requests and
            // process completed results from the shared queue.
            self.drain_all_pending_wasm_requests();
            self.drain_wasm_results();
        }

        result
    }

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    fn perform_microtask_checkpoint(&mut self) -> Result<(), String> {
        for document in self.documents.values_mut() {
            document
                .settings
                .perform_a_microtask_checkpoint()
                .map_err(|error| format!("microtask checkpoint failed: {error}"))?;
        }
        Ok(())
    }

    fn handle_command_inner(&mut self, command: Command) -> Result<bool, String> {
        match command {
            Command::SetEventLoopId(event_loop_id) => {
                self.event_loop_id = event_loop_id;
                Ok(true)
            }

            SetViewport(viewport) => {
                self.set_viewport(viewport);
                Ok(true)
            }
            SetTraversableViewport(viewport) => {
                self.set_traversable_viewport(viewport)?;
                Ok(true)
            }
            CreateEmptyDocument {
                traversable_id,
                document_id,
                frame_id,
                parent_traversable_id,
                top_level_traversable_id,
            } => {
                self.create_empty_document(
                    traversable_id,
                    document_id,
                    frame_id,
                    parent_traversable_id,
                    top_level_traversable_id,
                )?;
                Ok(true)
            }
            CreateLoadedDocument {
                traversable_id,
                document_id,
                frame_id,
                response,
                parent_traversable_id,
                top_level_traversable_id,
            } => {
                self.create_loaded_document(
                    traversable_id,
                    document_id,
                    frame_id,
                    response,
                    parent_traversable_id,
                    top_level_traversable_id,
                )?;
                Ok(true)
            }
            DestroyDocument { document_id } => {
                self.destroy_document(document_id)?;
                Ok(true)
            }
            EvaluateScript {
                traversable_id,
                request_id,
                source,
            } => {
                let (value_json, error) = match self.evaluate_script(traversable_id, source) {
                    Ok(value) => {
                        let value_json = match serde_json::to_string(&value) {
                            Ok(json) => json,
                            Err(error) => {
                                error!("failed to encode script evaluation result: {error}");
                                return Ok(true);
                            }
                        };
                        (value_json, None)
                    }
                    Err(error) => (String::from("null"), Some(error)),
                };
                if let Err(error) =
                    self.event_sender
                        .send(ContentEvent::ScriptEvaluated(ScriptEvaluationResult {
                            request_id,
                            value_json,
                            error,
                        }))
                {
                    error!("failed to send script evaluation result: {error}");
                }
                Ok(true)
            }
            ClickElement {
                traversable_id,
                request_id,
                selector,
            } => {
                let error = self.click_element(traversable_id, selector).err();
                self.event_sender
                    .send(ContentEvent::ElementClicked(ElementClickResult {
                        request_id,
                        error,
                    }))
                    .map_err(|error| format!("failed to send element click result: {error}"))?;
                Ok(true)
            }
            DispatchEvent { events } => {
                self.dispatch_events(events)?;
                // Flush any traversable documents created during event dispatch.
                // (The last involved traversable is unknown at this level, so
                // we skip flushing here — EventTarget dispatch happens through
                // the DOM and doesn't directly create traversable documents.)
                Ok(true)
            }
            Command::RunBeforeUnload {
                document_id,
                check_id,
                navigation_id,
            } => {
                self.run_before_unload(document_id, check_id, navigation_id)?;
                Ok(true)
            }
            UpdateTheRendering {
                traversable_id,
                document_id,
            } => {
                self.update_the_rendering(traversable_id, document_id)?;
                Ok(true)
            }
            RunWindowTimer {
                document_id,
                timer_id,
                timer_key,
                nesting_level,
            } => {
                self.run_window_timer(document_id, timer_id, timer_key, nesting_level)?;
                Ok(true)
            }
            CompleteDocumentFetch {
                handler_id,
                response,
            } => {
                self.complete_document_fetch(handler_id, response)?;
                Ok(true)
            }
            FailDocumentFetch { handler_id } => {
                self.fail_document_fetch(handler_id)?;
                Ok(true)
            }
            ContentBootstrap { .. } => {
                // Handled before the event loop in run_content_process.
                debug_assert!(false, "ContentBootstrap should not reach handle_command");
                Ok(true)
            }
            Shutdown => {
                self.note_shutdown_completed()?;
                Ok(false)
            }
        }
    }
}

fn content_token_from_args() -> Result<Option<String>, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--content-token" {
            return args
                .next()
                .map(Some)
                .ok_or_else(|| String::from("missing content token value"));
        }
    }
    Ok(None)
}

/// Run the content extension.
pub fn run_content_process(token: String) -> Result<(), String> {
    // When WASM is not enabled, use `never()` so the select never fires.
    // When WASM IS enabled, create a real channel that the wasm worker
    // signals when compilation completes.
    let (wasm_rx, wasm_signal_sender) = if cfg!(all(boa_backend, feature = "wasm")) {
        let (tx, rx) = crossbeam_channel::unbounded::<()>();
        (rx, tx)
    } else {
        let rx = crossbeam_channel::never::<()>();
        let (tx, _) = crossbeam_channel::bounded::<()>(1);
        (rx, tx)
    };

    ipc::run_extension::<Command, ContentEvent>(&token, move |server| {
        let event_sender = server.connection.sender.clone();

        let cmd_rx = ipc::crossbeam_proxy(server.connection.receiver);

        let (network_extension_sender, media_sender, content_command_sender, trace_sender) = {
            match cmd_rx.recv() {
                Ok(incoming) => match incoming.payload {
                    ContentBootstrap {
                        net_sender,
                        media_sender,
                        content_command_sender,
                        trace_sender,
                    } => (
                        net_sender,
                        media_sender,
                        content_command_sender,
                        trace_sender,
                    ),
                    other => {
                        error!("first message must be ContentBootstrap, got: {other:?}");
                        return Err("wrong first message, expected ContentBootstrap".into());
                    }
                },
                Err(_) => return Err("command channel closed before ContentBootstrap".into()),
            }
        };

        let _ = event_sender.send(ContentEvent::CommandCompleted);

        let mut process = {
            let event_loop_id = EventLoopId::from_u128(0);
            ContentProcess::new(
                event_sender.clone(),
                wasm_signal_sender,
                event_loop_id,
                network_extension_sender,
                media_sender,
                content_command_sender,
                trace_sender,
            )
        };

        run_content_message_loop(&cmd_rx, &wasm_rx, &mut process)
    })
}

fn run_content_message_loop(
    cmd_rx: &crossbeam_channel::Receiver<ipc::IpcIncoming<Command>>,
    wasm_rx: &crossbeam_channel::Receiver<()>,
    process: &mut ContentProcess,
) -> Result<(), String> {
    loop {
        crossbeam_channel::select! {
            recv(cmd_rx) -> cmd => {
                match cmd {
                    Ok(incoming) => {
                        let command = incoming.payload;
                        let notify = matches!(
                            &command,
                            CreateEmptyDocument { .. }
                                | CreateLoadedDocument { .. }
                                | DestroyDocument { .. }
                                | DispatchEvent { .. }
                                | Command::RunBeforeUnload { .. }
                                | UpdateTheRendering { .. }
                                | RunWindowTimer { .. }
                                | CompleteDocumentFetch { .. }
                                | FailDocumentFetch { .. }
                        );
                        match process.handle_command(command) {
                            Ok(true) => {
                                if notify {
                                    let _ = process.note_command_completed();
                                    // <https://html.spec.whatwg.org/#event-loop-processing-model>
                                    // Step 2.8: Perform a microtask checkpoint.
                                    if let Err(error) = process.perform_microtask_checkpoint() {
                                        error!("microtask checkpoint after task failed: {error}");
                                    }
                                }
                            }
                            Ok(false) => return Ok(()),
                            Err(error) => {
                                error!("content error: {error}");
                                if notify {
                                    let _ = process.note_command_completed();
                                }
                            }
                        }
                    }
                    Err(_) => return Ok(()),
                }
            }
            recv(wasm_rx) -> _ => {
                process.drain_all_pending_wasm_requests();
                process.drain_wasm_results();

                // <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
                // Wasm compilation results resolve promises, so run a microtask
                // checkpoint after they are processed.
                if let Err(error) = process.perform_microtask_checkpoint() {
                    error!("microtask checkpoint after wasm failed: {error}");
                }
            }
        }
    }
}

pub fn run_content_process_from_args() -> Result<(), String> {
    let token = content_token_from_args()?;
    // If a token was provided (ipc-channel mode), use it.
    // Otherwise, use the native XPC backend (process launched by launchd).
    run_content_process(token.unwrap_or_default())
}
