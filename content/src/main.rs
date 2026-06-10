#[allow(dead_code)]
#[path = "../../embedder/src/ui_event.rs"]
pub(crate) mod ui_event;

pub mod js;
pub mod css;
pub mod dom;
pub mod html;
pub mod infra;
pub mod streams;
pub mod wasm;
pub mod webidl;

use crate::dom::{
    dispatch_trusted_click_event, dispatch_ui_event, dispatch_window_event, fire_event,
};
use crate::html::{
    EnvironmentSettingsObject, JsHtmlParserProvider, PendingParserScript,
    attach_same_origin_child_document_for_traversable, execute_parser_scripts,
    parse_html_into_document, run_dom_post_connection_steps_for_document,
    run_dom_removing_steps_for_document, run_iframe_load_event_steps_for_traversable,
};
use crate::ui_event::deserialize_ui_event;
use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_paint::paint_scene;
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use data_url::DataUrl;
use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::Command::{
    ClickElement, CompleteDocumentFetch, CreateEmptyDocument, CreateLoadedDocument,
    DestroyDocument, DispatchEvent, EvaluateScript, FailDocumentFetch, RunWindowTimer,
    SetTraversableViewport, SetViewport, Shutdown, UpdateTheRendering,
};
use ipc_messages::content::{
    BeforeUnloadCheckId, Bootstrap, ClipboardReadRequest, ClipboardWriteRequest,
    ColorScheme as MessageColorScheme, Command, DispatchEventEntry, DocumentFetchId, DocumentId,
    ElementClickResult, EmbedBackgroundPolicy, EmbedSiteId, Event as ContentEvent, EventLoopId,
    FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse,
    FontTransportSender, FrameCompositionMetadata, FrameEmbedSite, FrameId, LoadedDocumentResponse,
    NavigableId, NavigationId, PaintFrame, ScriptEvaluationResult, TraversableViewport,
    ViewportSnapshot, WebviewId, WindowTimerKey,
};
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

struct ContentShellProvider {
    event_sender: IpcSender<ContentEvent>,
}

impl ContentShellProvider {
    fn new(event_sender: IpcSender<ContentEvent>) -> Self {
        Self { event_sender }
    }
}

impl ShellProvider for ContentShellProvider {
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        let (reply_sender, reply_receiver) =
            ipc::channel::<Result<String, String>>().map_err(|_| ClipboardError)?;
        self.event_sender
            .send(ContentEvent::ClipboardReadRequested(ClipboardReadRequest {
                reply_sender,
            }))
            .map_err(|_| ClipboardError)?;
        reply_receiver
            .recv()
            .map_err(|_| ClipboardError)?
            .map_err(|_| ClipboardError)
    }

    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        let (reply_sender, reply_receiver) =
            ipc::channel::<Result<(), String>>().map_err(|_| ClipboardError)?;
        self.event_sender
            .send(ContentEvent::ClipboardWriteRequested(
                ClipboardWriteRequest { text, reply_sender },
            ))
            .map_err(|_| ClipboardError)?;
        reply_receiver
            .recv()
            .map_err(|_| ClipboardError)?
            .map_err(|_| ClipboardError)
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
        eprintln!("[render-state][content] {}", message.as_ref());
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
        eprintln!(
            "[input-debug][layout] document={} interesting_nodes=none",
            document_id,
        );
        return;
    }

    eprintln!(
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
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
    content_document_id: DocumentId,
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
                if let Err(error) = self.event_sender.send(ContentEvent::DocumentFetchRequested(
                    ContentFetchRequest {
                        handler_id,
                        url: request.url.to_string(),
                        method: request.method.to_string(),
                        body: request_body_string(&request.body),
                    },
                )) {
                    eprintln!("failed to send document fetch request to the embedder: {error}");
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
    event_sender: IpcSender<ContentEvent>,
    event_loop_id: EventLoopId,
    local_state: LocalContentStateRef,
    default_viewport: Option<ViewportSnapshot>,
    traversable_viewports: HashMap<NavigableId, DocumentViewportState>,
    documents: HashMap<DocumentId, ContentDocument>,
    active_documents_by_traversable: HashMap<NavigableId, DocumentId>,
    font_namespace: u64,
    font_sender: FontTransportSender,
    navigation_tracer: TLATracer,
    /// Shared registry for traversable documents created during JS execution
    /// (window.open).  ContentProcess holds one Rc, and before running JS it
    /// sets a clone on the source document's GlobalScope so that
    /// `register_new_traversable_document` can insert directly into this map.
    new_document_registry: Rc<
        RefCell<HashMap<DocumentId, (EnvironmentSettingsObject, Rc<RefCell<BaseDocument>>)>>,
    >,

    /// Background wasm compilation thread.
    wasm_worker: crate::wasm::WasmWorker,

    /// Pending wasm compilation requests waiting for background results.
    /// Maps request_id → document_id.
    pending_wasm_requests: HashMap<u64, DocumentId>,
}

impl ContentProcess {
    fn new(event_sender: IpcSender<ContentEvent>, event_loop_id: EventLoopId) -> Self {
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
            navigation_tracer: TLATracer::new("Navigation", "formal-web:content", None),
            new_document_registry: Rc::new(RefCell::new(HashMap::new())),
            wasm_worker: crate::wasm::WasmWorker::new(
                wasmtime::Engine::default(),
            ),
            pending_wasm_requests: HashMap::new(),
        }
    }

    fn set_trace_sender(&mut self, trace_sender: Option<TraceSender>) {
        self.navigation_tracer.set_sender(trace_sender);
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
                event_sender: self.event_sender.clone(),
                local_state: Arc::clone(&self.local_state),
                content_document_id: document_id,
            })),
            shell_provider: Some(Arc::new(ContentShellProvider::new(
                self.event_sender.clone(),
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
        self.event_sender
            .send(ContentEvent::DocumentFetchRequested(ContentFetchRequest {
                handler_id,
                url: request.url.to_string(),
                method: request.method.to_string(),
                body: request_body_string(&request.body),
            }))
            .map_err(|error| {
                format!("failed to send document fetch request to the embedder: {error}")
            })
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
    fn set_up_new_document_registry(&self, traversable_id: NavigableId) -> Result<(), String> {
        let document_id = self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable {traversable_id}"))?;
        let content_document = self
            .documents
            .get(document_id)
            .ok_or_else(|| format!("unknown document {document_id}"))?;
        let registry = Rc::clone(&self.new_document_registry);
        crate::js::platform_objects::with_global_scope(
            &content_document.settings.context,
            |global_scope| {
                global_scope.set_new_document_registry(registry);
                Ok(())
            },
        )
        .map_err(|error| format!("failed to set new document registry: {error}"))
    }

    /// Clear the shared new-document registry from the source document's
    /// GlobalScope after JS execution completes.
    fn tear_down_new_document_registry(&self, traversable_id: NavigableId) -> Result<(), String> {
        let document_id = self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable {traversable_id}"))?;
        let content_document = self
            .documents
            .get(document_id)
            .ok_or_else(|| format!("unknown document {document_id}"))?;
        crate::js::platform_objects::with_global_scope(
            &content_document.settings.context,
            |global_scope| {
                global_scope.clear_new_document_registry();
                Ok(())
            },
        )
        .map_err(|error| format!("failed to clear new document registry: {error}"))
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

        for (document_id, (settings, document)) in pending {
            if self.documents.contains_key(&document_id) {
                continue;
            }
            // Read the traversable_id from the new document's own GlobalScope.
            let new_traversable_id = crate::js::platform_objects::with_global_scope(
                &settings.context,
                |global_scope| Ok(global_scope.source_navigable_id()),
            )
            .map_err(|error| format!("failed to read new traversable id: {error}"))?
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
    /// Note: This function implements the content-process portion of the `#navigate-html`
    /// algorithm: it waits until all critical subresources and deferred scripts are ready, then
    /// executes those scripts, fires the `load` event, and sends `ContentEvent::FinalizeNavigation`
    /// to the user agent to trigger `finalize_cross_document_navigation` (step 14 of the
    /// algorithm). It may be invoked multiple times per document — each call re-checks readiness
    /// and returns early until all blocking work is complete.
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

        for script in pending_document_load.scripts {
            match script {
                DeferredScriptState::Inline { source }
                | DeferredScriptState::ExternalReady { source } => {
                    if let Err(error) = content_document.settings.evaluate_script(&source) {
                        eprintln!("content error: {error}");
                    }
                }
                DeferredScriptState::ExternalPending { .. }
                | DeferredScriptState::ExternalFailed { .. } => {}
            }
        }

        let window = content_document.settings.context.global_object();
        fire_event(&mut content_document.settings, &window, "load", true)
            .map_err(|error| error.to_string())?;

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
    /// Note: This resumes the Rust-owned suffix of browsing-context creation after `FormalWeb.UserAgent.queueCreateEmptyDocument` reaches `FormalWeb.EventLoop.runEventLoopMessage` and the user-agent/content command path emits `CreateEmptyDocument`.
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
        let settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse("about:blank").map_err(|error| error.to_string())?,
            Some(self.event_sender.clone()),
            Some(traversable_id),
            Some(document_id),
        )?;

        // Note: This block continues <https://html.spec.whatwg.org/#creating-a-new-browsing-context>.
        // Step 7: "Mark document as ready for post-load tasks."
        // TODO: Persist the document's post-load readiness state in the DOM model.

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 8: "Populate with html/head/body given document."
            // Note: The content process drives the shared HTML parser with a fixed `about:blank` skeleton instead of constructing the three elements manually.
            parse_html_into_document(&mut document_guard, EMPTY_HTML_DOCUMENT)
        };

        // Step 10: "Completely finish loading document."
        // Note: The content process executes parser-discovered classic scripts immediately after the initial tree build.
        // TODO: Model the rest of the `completely finish loading` bookkeeping explicitly instead of relying on parser-discovered script execution alone.
        // Step 9: "Make active document."
        // Note: The implementation records the document as addressable for future commands by storing it under `document_id` after initialization completes.
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
    /// Note: This continues the HTML document loading algorithm through the end-of-document load steps and into `completely finish loading`.
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
        // Note: This block continues <https://html.spec.whatwg.org/#navigate-html>.
        // Step 1: "Let document be the result of creating and initializing a `Document` object given `html`, `text/html`, and navigationParams."
        // Note: `BaseDocument::new` and `EnvironmentSettingsObject::new` split document creation between the [Document](https://dom.spec.whatwg.org/#interface-document) [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) and the JavaScript environment settings object.
        let document = Rc::new(RefCell::new(BaseDocument::new(self.document_config(
            traversable_id,
            document_id,
            Some(final_url.clone()),
        ))));
        let settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse(&final_url).map_err(|error| error.to_string())?,
            Some(self.event_sender.clone()),
            Some(traversable_id),
            Some(document_id),
        )?;

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 3: "Otherwise, create an HTML parser and associate it with the document."
            // Note: The embedder has already buffered the response body, so the content process feeds it into the parser immediately instead of waiting on separate networking tasks.
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
                eprintln!("content error: {error}");
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
            eprintln!("failed to set up new document registry: {error}");
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
            eprintln!("failed to tear down new document registry: {error}");
        }
        if let Err(error) = self.drain_new_traversable_documents() {
            eprintln!("failed to drain new traversable documents: {error}");
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
            eprintln!("failed to set up new document registry: {error}");
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
        if let Some(content_document) = self.documents.remove(&document_id) {
            if self
                .active_documents_by_traversable
                .get(&content_document.traversable_id)
                .is_some_and(|current_document_id| *current_document_id == document_id)
            {
                self.active_documents_by_traversable
                    .remove(&content_document.traversable_id);
            }
            if let Err(error) = content_document.settings.clear_all_window_timers() {
                eprintln!("failed to clear window timers during document teardown: {error}");
            }
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
        for DispatchEventEntry { document_id, event } in events {
            let Some(document) = self.documents.get_mut(&document_id) else {
                continue;
            };

            {
                let document_guard = document.document.borrow();
                maybe_log_input_layout_debug(document_id, &document_guard);
            }

            // Note: This continues <https://dom.spec.whatwg.org/#concept-event-fire> after `FormalWeb.UserAgent.queueDispatchedEvent` hands the serialized UI event batch to the content process.
            let event = deserialize_ui_event(&event)?;
            dispatch_ui_event(
                document_id,
                document.traversable_id,
                document.parent_traversable_id,
                document.top_level_traversable_id,
                Rc::clone(&document.document),
                &mut document.settings,
                &self.event_sender,
                document.viewport_offset_x,
                document.viewport_offset_y,
                event,
            )?;
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
            let canceled = !dispatch_window_event(&mut document.settings, "beforeunload", true)
                .map_err(|error| error.to_string())?;
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
    /// Note: The Rust user-agent and event-loop workers queue this rendering task, and the content process continues the noted rendering opportunity once critical fetches finish.
    fn continue_updating_the_rendering(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
    ) -> Result<(), String> {
        let event_sender = self.event_sender.clone();
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
                // Note: `resolve` advances style, layout, and resource-driven document updates for the current frame.
                document_guard.resolve(animation_time);
            }

            let paint_frame = {
                let document_guard = document.document.borrow();
                let viewport = document_guard.viewport().clone();
                let (width, height) = viewport.window_size;
                let mut scene = RenderScene::new();
                let composition = Self::build_frame_composition_metadata(
                    &document_guard,
                    &document.navigable_container_states,
                    viewport.scale_f64(),
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
                let scene = self.font_sender.prepare_scene(self.font_namespace, scene);
                log_render_state_debug(format!(
                    "emit paint traversable={} document={} size=({}, {})",
                    traversable_id, document_id, width, height,
                ));
                let paint_frame = PaintFrame::new(
                    WebviewId(traversable_id),
                    document.frame_id,
                    width,
                    height,
                    composition,
                    scene,
                )?;
                paint_frame
            };
            paint_frame
        };

        event_sender
            .send(ContentEvent::PaintReady(paint_frame))
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

    fn content_box_for_iframe(
        document: &BaseDocument,
        node_id: usize,
        scale: f64,
    ) -> Option<(f64, f64, f64, f64)> {
        let node = document.get_node(node_id)?;
        let layout = node.final_layout;
        let edge = layout.padding + layout.border;
        let (border_x, border_y) = Self::node_absolute_border_origin(document, node_id, scale)?;
        let x = border_x + f64::from(edge.left) * scale;
        let y = border_y + f64::from(edge.top) * scale;
        let width = (f64::from(layout.size.width) - f64::from(edge.left + edge.right)) * scale;
        let height = (f64::from(layout.size.height) - f64::from(edge.top + edge.bottom)) * scale;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        Some((x, y, width, height))
    }

    fn build_frame_composition_metadata(
        document: &BaseDocument,
        container_states: &HashMap<usize, NavigableContainerState>,
        scale: f64,
    ) -> FrameCompositionMetadata {
        let mut iframe_node_ids = container_states
            .iter()
            .filter_map(|(iframe_node_id, state)| {
                state.cross_origin.then_some((*iframe_node_id, state))
            })
            .collect::<Vec<_>>();
        iframe_node_ids.sort_by_key(|(iframe_node_id, _)| *iframe_node_id);

        let embed_sites = iframe_node_ids
            .into_iter()
            .enumerate()
            .filter_map(|(paint_order, (iframe_node_id, state))| {
                let (x, y, width, height) =
                    Self::content_box_for_iframe(document, iframe_node_id, scale)?;
                let clip_svg_path = format!("M0,0 L{width},0 L{width},{height} L0,{height} Z");
                Some(FrameEmbedSite {
                    embed_site_id: EmbedSiteId((iframe_node_id as u64).wrapping_add(1)),
                    child_frame_id: state.content_frame_id,
                    z_index: 0,
                    paint_order: paint_order as u32,
                    background_policy: EmbedBackgroundPolicy::OpaqueWhite,
                    transform: [1.0, 0.0, 0.0, 1.0, x, y],
                    clip_bounds: [x, y, x + width, y + height],
                    clip_svg_path,
                })
            })
            .collect();

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
                    eprintln!(
                        "[content] complete_document_fetch: document {document_id} not found"
                    );
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
                    eprintln!(
                        "content deferred script rejected: url={} status={} content-type={}",
                        response.final_url, response.status, response.content_type,
                    );
                    self.mark_deferred_script_failed(document_id, script_index);
                }
                let Some(content_document) = self.documents.get(&document_id) else {
                    eprintln!(
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
                    eprintln!("[content] fail_document_fetch: document {document_id} not found");
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
                    eprintln!(
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
            eprintln!("failed to set up new document registry: {error}");
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
            eprintln!("failed to tear down new document registry: {error}");
        }
        if let Err(error) = self.drain_new_traversable_documents() {
            eprintln!("failed to drain new traversable documents: {error}");
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
        &self,
        document_id: DocumentId,
    ) -> Result<(), String> {
        let Some(content_document) = self.documents.get(&document_id) else {
            return Err(format!("unknown document id: {document_id}"));
        };
        let parent_traversable_id = content_document.parent_traversable_id;
        let top_level_traversable_id = content_document.top_level_traversable_id;
        crate::js::platform_objects::with_global_scope(
            &content_document.settings.context,
            |global_scope| {
                global_scope.set_navigable_hierarchy(
                    parent_traversable_id,
                    top_level_traversable_id,
                );
                Ok(())
            },
        )
        .map_err(|error| error.to_string())
    }

    /// Drain pending WebAssembly requests from all documents and submit them
    /// to the background compilation thread.
    fn drain_all_pending_wasm_requests(&mut self) {
        let document_ids: Vec<DocumentId> = self.documents.keys().copied().collect();

        for document_id in document_ids {
            let Some(content_document) = self.documents.get_mut(&document_id) else {
                continue;
            };

            let batches = content_document.settings.take_pending_wasm_batches();
            for (request_id, bytes) in batches {
                self.pending_wasm_requests.insert(request_id, document_id);
                self.wasm_worker.submit_compile(bytes);
            }
        }
    }

    /// Process completed wasm compilation results from the background thread.
    fn process_wasm_results(&mut self) {
        let Some(result_rx) = self.wasm_worker.result_receiver() else {
            return;
        };

        let completed: Vec<(u64, crate::wasm::WasmResult)> = {
            let mut results = Vec::new();
            while let Ok(result) = result_rx.try_recv() {
                let request_id = match &result {
                    crate::wasm::WasmResult::Compiled { request_id, .. }
                    | crate::wasm::WasmResult::CompileError { request_id, .. } => *request_id,
                };
                results.push((request_id, result));
            }
            results
        };

        for (request_id, result) in completed {
            let Some(&document_id) = self.pending_wasm_requests.get(&request_id) else {
                eprintln!("wasm: no pending request found for id {}", request_id);
                continue;
            };

            let Some(content_document) = self.documents.get_mut(&document_id) else {
                eprintln!("wasm: document {} not found", document_id);
                self.pending_wasm_requests.remove(&request_id);
                continue;
            };

            let Some((_promise, resolvers)) =
                content_document.settings.consume_wasm_request(request_id)
            else {
                eprintln!(
                    "wasm: request {} not found on document {}",
                    request_id, document_id
                );
                self.pending_wasm_requests.remove(&request_id);
                continue;
            };

            match result {
                crate::wasm::WasmResult::Compiled {
                    request_id: _,
                    module,
                } => {
                    if let Err(error) = crate::js::bindings::wasm::resolve_compile_promise(
                        &resolvers,
                        module,
                        Vec::new(),
                        &mut content_document.settings.context,
                    ) {
                        eprintln!("wasm: failed to resolve compile promise: {error}");
                    }
                }
                crate::wasm::WasmResult::CompileError {
                    request_id: _,
                    message,
                } => {
                    if let Err(error) = crate::js::bindings::wasm::reject_compile_promise(
                        &resolvers,
                        message,
                        &mut content_document.settings.context,
                    ) {
                        eprintln!("wasm: failed to reject compile promise: {error}");
                    }
                }
            }

            self.pending_wasm_requests.remove(&request_id);
        }

        // Flush microtasks (promise .then() handlers) after resolving/rejecting.
        for document in self.documents.values_mut() {
            if let Err(error) = document.settings.perform_a_microtask_checkpoint() {
                eprintln!("wasm: microtask checkpoint failed: {error}");
            }
        }
    }

    /// <https://html.spec.whatwg.org/#event-loop-processing-model>
    /// Note: The Rust event-loop worker emits these process effects, and each branch below resumes the corresponding Rust-owned continuation.
    fn handle_command(&mut self, command: Command) -> Result<bool, String> {
        // Before processing a new command, drain any pending WebAssembly
        // requests and process completed compilation results.
        self.drain_all_pending_wasm_requests();
        self.process_wasm_results();

        let result = self.handle_command_inner(command);

        // After every command, drain any pending WebAssembly requests and
        // process completed compilation results.
        self.drain_all_pending_wasm_requests();
        self.process_wasm_results();

        result
    }

    fn handle_command_inner(&mut self, command: Command) -> Result<bool, String> {
        match command {
            Command::SetEventLoopId(event_loop_id) => {
                self.event_loop_id = event_loop_id;
                Ok(true)
            }
            Command::SetTraceSender(trace_sender) => {
                self.set_trace_sender(trace_sender);
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
                        let value_json = serde_json::to_string(&value).map_err(|error| {
                            format!("failed to encode script evaluation result: {error}")
                        })?;
                        (value_json, None)
                    }
                    Err(error) => (String::from("null"), Some(error)),
                };
                self.event_sender
                    .send(ContentEvent::ScriptEvaluated(ScriptEvaluationResult {
                        request_id,
                        value_json,
                        error,
                    }))
                    .map_err(|error| format!("failed to send script evaluation result: {error}"))?;
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

pub fn run_content_process(token: String) -> Result<(), String> {
    let (command_sender, command_receiver) =
        ipc::channel::<Command>().map_err(|error| error.to_string())?;
    let (event_sender, event_receiver) =
        ipc::channel::<ContentEvent>().map_err(|error| error.to_string())?;
    let bootstrap = IpcSender::<Bootstrap>::connect(token).map_err(|error| error.to_string())?;
    // The event loop id is sent by the UA via SetEventLoopId command after bootstrap.
    // Use a placeholder until the real id arrives.
    let placeholder_id = EventLoopId::from_u128(0);
    bootstrap
        .send(Bootstrap {
            command_sender,
            event_receiver,
            event_loop_id: placeholder_id,
        })
        .map_err(|error| error.to_string())?;

    let mut process = ContentProcess::new(event_sender, placeholder_id);
    loop {
        let command = match command_receiver.recv() {
            Ok(command) => command,
            Err(_error) => break,
        };
        let notify_event_loop = matches!(
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
                if notify_event_loop {
                    if let Err(error) = process.note_command_completed() {
                        eprintln!("content error: {error}");
                    }
                }
            }
            Ok(false) => break,
            Err(error) => {
                eprintln!("content error: {error}");
                if notify_event_loop {
                    if let Err(error) = process.note_command_completed() {
                        eprintln!("content error: {error}");
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn run_content_process_from_args() -> Result<(), String> {
    let token = content_token_from_args()?
        .ok_or_else(|| String::from("missing --content-token argument"))?;
    run_content_process(token)
}
