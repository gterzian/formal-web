#[allow(dead_code)]
#[path = "../../embedder/src/ui_event.rs"]
pub(crate) mod ui_event;

pub mod boa;
pub mod dom;
pub mod html;
pub mod streams;
pub mod webidl;

use crate::dom::{dispatch_ui_event, dispatch_window_event, fire_event};
use crate::html::{
    EnvironmentSettingsObject, JsHtmlParserProvider, PendingParserScript, execute_parser_scripts,
    attach_same_origin_child_document_for_traversable,
    parse_html_into_document, run_dom_post_connection_steps_for_document,
    run_dom_removing_steps_for_document, run_iframe_load_event_steps_for_traversable,
};
use crate::ui_event::deserialize_ui_event;
use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, DocumentConfig, NodeData};
use blitz_paint::paint_scene;
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use data_url::DataUrl;
use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::Command::{
    CompleteDocumentFetch, CreateEmptyDocument, CreateLoadedDocument, DestroyDocument,
    DispatchEvent, EvaluateScript, FailDocumentFetch, RunWindowTimer, SetTraversableViewport,
    SetViewport, Shutdown, UpdateTheRendering,
};
use ipc_messages::content::{
    BeforeUnloadCheckId, Bootstrap, ClipboardReadRequest, ClipboardWriteRequest,
    ColorScheme as MessageColorScheme, Command, DispatchEventEntry,
    DocumentFetchId, DocumentId, Event as ContentEvent,
    FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse,
    FontTransportSender, FrameId, LoadedDocumentResponse, NavigableId, PaintFrame,
    PlaceholderFrameMapping, RecordedScene, ScriptEvaluationResult, TraversableViewport,
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
use tla_trace::{LogEntry, receive_monitor_sender};
use url::Url;

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
            .send(ContentEvent::ClipboardWriteRequested(ClipboardWriteRequest {
                text,
                reply_sender,
            }))
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
    pub(crate) content_frame_token: u64,
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

fn render_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_RENDER").is_some()
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

fn log_paint_debug(document_id: DocumentId, document: &BaseDocument, scene: &RecordedScene) {
    maybe_log_input_layout_debug(document_id, document);

    if !render_debug_enabled() {
        return;
    }

    let mut text_nodes = 0;
    let mut non_empty_text_nodes = 0;
    let mut inline_roots = 0;
    let mut inline_layouts = 0;

    document.visit(|_node_id, node| {
        if node.flags.is_inline_root() {
            inline_roots += 1;
        }
        if node
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
            .is_some()
        {
            inline_layouts += 1;
        }
        if let NodeData::Text(text) = &node.data {
            text_nodes += 1;
            if !text.content.trim().is_empty() {
                non_empty_text_nodes += 1;
            }
        }
    });

    eprintln!(
        "[render-debug][content] doc={} {} text_nodes={} non_empty_text_nodes={} inline_roots={} inline_layouts={}",
        document_id,
        scene.summary().describe(),
        text_nodes,
        non_empty_text_nodes,
        inline_roots,
        inline_layouts,
    );

    if scene.summary().glyph_runs == 0 {
        let mut inline_root_ids = Vec::new();
        document.visit(|node_id, node| {
            if node.flags.is_inline_root() {
                inline_root_ids.push(node_id);
            }
        });

        for node_id in inline_root_ids {
            document.debug_log_node(node_id);
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

struct ContentDocument {
    traversable_id: NavigableId,
    parent_traversable_id: Option<NavigableId>,
    top_level_traversable_id: NavigableId,
    frame_id: FrameId,
    document: Rc<RefCell<BaseDocument>>,
    settings: EnvironmentSettingsObject,
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

pub(crate) struct ContentRuntime {
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
    default_viewport: Option<ViewportSnapshot>,
    traversable_viewports: HashMap<NavigableId, DocumentViewportState>,
    documents: HashMap<DocumentId, ContentDocument>,
    active_documents_by_traversable: HashMap<NavigableId, DocumentId>,
    next_placeholder_frame_token: u64,
    font_namespace: u64,
    font_sender: FontTransportSender,
}

impl ContentRuntime {
    fn new(event_sender: IpcSender<ContentEvent>) -> Self {
        Self {
            event_sender,
            local_state: Arc::new(Mutex::new(LocalContentState {
                pending_handlers: HashMap::new(),
            })),
            default_viewport: None,
            traversable_viewports: HashMap::new(),
            documents: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
            next_placeholder_frame_token: 1,
            font_namespace: new_font_namespace(),
            font_sender: FontTransportSender::default(),
        }
    }

    fn document_viewport_state(&self, traversable_id: NavigableId) -> Option<DocumentViewportState> {
        self.traversable_viewports
            .get(&traversable_id)
            .cloned()
            .or_else(|| {
                self.default_viewport.as_ref().cloned().map(|snapshot| DocumentViewportState {
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

    fn set_traversable_viewport(&mut self, viewport: TraversableViewport) {
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
            return;
        };
        let Some(document) = self.documents.get_mut(&document_id) else {
            return;
        };

        document
            .document
            .borrow_mut()
            .set_viewport(viewport_of_snapshot(&viewport_state.snapshot));
        document.viewport_offset_x = viewport_state.offset_x;
        document.viewport_offset_y = viewport_state.offset_y;
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
        local_state.pending_handlers.insert(handler_id, pending_handler);
        Ok(handler_id)
    }

    fn request_remote_fetch(
        &self,
        handler_id: DocumentFetchId,
        request: Request,
    ) -> Result<(), String> {
        log_render_state_debug(format!(
            "request remote fetch handler={} method={} url={}",
            handler_id,
            request.method,
            request.url,
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

    fn allocate_placeholder_frame_token(&mut self) -> u64 {
        let token = self.next_placeholder_frame_token;
        self.next_placeholder_frame_token = self.next_placeholder_frame_token.wrapping_add(1);
        token
    }

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
            document_id,
            traversable_id,
            pending_document_load.finalize_url,
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
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(traversable_id, document_id, None),
        )));
        let settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse("about:blank").map_err(|error| error.to_string())?,
        )?;
        settings.install_timer_host(document_id, self.event_sender.clone())?;

        // Note: This block continues <https://html.spec.whatwg.org/#creating-a-new-browsing-context>.
        // Step 7: "Mark document as ready for post-load tasks."
        // TODO: Persist the document's post-load readiness state in the DOM/runtime model.

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 8: "Populate with html/head/body given document."
            // Note: The content runtime drives the shared HTML parser with a fixed `about:blank` skeleton instead of constructing the three elements manually.
            parse_html_into_document(&mut document_guard, EMPTY_HTML_DOCUMENT)
        };

        // Step 10: "Completely finish loading document."
        // Note: The content runtime executes parser-discovered classic scripts immediately after the initial tree build.
        // TODO: Model the rest of the `completely finish loading` bookkeeping explicitly instead of relying on parser-discovered script execution alone.
        // Step 9: "Make active document."
        // Note: The runtime records the document as addressable for future commands by storing it under `document_id` after initialization completes.
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
                viewport_offset_x: viewport_state.as_ref().map(|viewport| viewport.offset_x).unwrap_or(0.0),
                viewport_offset_y: viewport_state.as_ref().map(|viewport| viewport.offset_y).unwrap_or(0.0),
            },
        );
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
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
        // Note: `BaseDocument::new` and `EnvironmentSettingsObject::new` split document creation between the DOM carrier and the JavaScript environment settings object.
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(traversable_id, document_id, Some(final_url.clone())),
        )));
        let settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse(&final_url).map_err(|error| error.to_string())?,
        )?;
        settings.install_timer_host(document_id, self.event_sender.clone())?;

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 3: "Otherwise, create an HTML parser and associate it with the document."
            // Note: The embedder has already buffered the response body, so the content runtime feeds it into the parser immediately instead of waiting on separate networking tasks.
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
                viewport_offset_x: viewport_state.as_ref().map(|viewport| viewport.offset_x).unwrap_or(0.0),
                viewport_offset_y: viewport_state.as_ref().map(|viewport| viewport.offset_y).unwrap_or(0.0),
            },
        );
        attach_same_origin_child_document_for_traversable(self, traversable_id)?;
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
        let document_id = *self
            .active_documents_by_traversable
            .get(&traversable_id)
            .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        document.settings.evaluate_script_to_json(&source)
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
            let _ = content_document.settings.clear_all_window_timers();
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

            // Note: This continues <https://dom.spec.whatwg.org/#concept-event-fire> after `FormalWeb.UserAgent.queueDispatchedEvent` hands the serialized UI event batch to the content runtime.
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
    ) -> Result<(), String> {
        let canceled = if let Some(document) = self.documents.get_mut(&document_id) {
            !dispatch_window_event(&mut document.settings, "beforeunload", true)
                .map_err(|error| error.to_string())?
        } else {
            false
        };
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
        let Some(document) = self.documents.get_mut(&document_id) else {
            return Ok(());
        };
        log_render_state_debug(format!(
            "queue update-the-rendering traversable={} document={}",
            traversable_id, document_id,
        ));
        document.pending_update_the_rendering = true;
        self.continue_updating_the_rendering(traversable_id, document_id)
    }

    /// <https://html.spec.whatwg.org/#update-the-rendering>
    /// Note: The Rust user-agent and event-loop workers queue this rendering task, and the content runtime continues the noted rendering opportunity once critical fetches finish.
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

            document.document.borrow_mut().handle_messages();

            if document.document.borrow().has_pending_critical_resources() {
                log_render_state_debug(format!(
                    "skip paint pending critical resources traversable={} document={}",
                    traversable_id, document_id,
                ));
                return Ok(());
            }

            let frame_timestamp_ms = document.settings.current_time_millis();

            // Step 1: "Let `frameTimestamp` be `eventLoop`'s last render opportunity time."
            // Note: The content runtime currently derives a monotonic frame timestamp from the document's environment settings object time origin instead of the HTML event loop's shared render-opportunity clock.

            // Step 14: "For each `doc` of `docs`, run the animation frame callbacks for `doc`, passing in the relative high resolution time given `frameTimestamp` and `doc`'s relevant global object as the timestamp."
            // Note: The content runtime collapses `docs` to the single active document for this content process and uses the same environment-relative time as both the HTML frame timestamp and the callback timestamp.
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
                let placeholder_frame_mappings = document
                    .navigable_container_states
                    .values()
                    .filter(|container_state| container_state.cross_origin)
                    .map(|container_state| PlaceholderFrameMapping {
                        token: container_state.content_frame_token,
                        frame_id: container_state.content_frame_id,
                    })
                    .collect::<Vec<_>>();
                let document_guard = document.document.borrow();
                let viewport = document_guard.viewport().clone();
                let (width, height) = viewport.window_size;
                let mut scene = RenderScene::new();

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
                log_paint_debug(document_id, &document_guard, &scene.scene);
                log_render_state_debug(format!(
                    "emit paint traversable={} document={} size=({}, {})",
                    traversable_id, document_id, width, height,
                ));
                let paint_frame = PaintFrame::new(
                    WebviewId(traversable_id),
                    document.frame_id,
                    width,
                    height,
                    placeholder_frame_mappings,
                    scene,
                )?;
                paint_frame
            };

            document.pending_update_the_rendering = false;
            paint_frame
        };

        event_sender
            .send(ContentEvent::PaintReady(paint_frame))
            .map_err(|error| format!("failed to send paint frame: {error}"))
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
                handler.bytes(response.final_url.clone(), Bytes::copy_from_slice(&response.body));
                let Some(content_document) = self.documents.get(&document_id) else {
                    eprintln!("[content] complete_document_fetch: document {document_id} not found");
                    return Ok(());
                };
                let traversable_id = content_document.traversable_id;
                log_render_state_debug(format!(
                    "complete resource fetch handler={} traversable={} document={} status={} type={} url={}",
                    handler_id, traversable_id, document_id, response_status, response_type, response_url,
                ));
                self.continue_document_load(document_id)?;
                self.continue_updating_the_rendering(traversable_id, document_id)?;
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
                    eprintln!("[content] complete_document_fetch (deferred script): document {document_id} not found");
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
                self.continue_updating_the_rendering(traversable_id, document_id)?;
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
                self.continue_updating_the_rendering(traversable_id, document_id)?;
                Ok(())
            }
            PendingNetworkHandler::DeferredScript {
                document_id,
                script_index,
            } => {
                self.mark_deferred_script_failed(document_id, script_index);
                let Some(content_document) = self.documents.get(&document_id) else {
                    eprintln!("[content] fail_document_fetch (deferred script): document {document_id} not found");
                    return Ok(());
                };
                let traversable_id = content_document.traversable_id;
                log_render_state_debug(format!(
                    "fail deferred-script fetch handler={} traversable={} document={} script_index={}",
                    handler_id, traversable_id, document_id, script_index,
                ));
                self.continue_document_load(document_id)?;
                self.continue_updating_the_rendering(traversable_id, document_id)?;
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
        let Some(document) = self.documents.get_mut(&document_id) else {
            return Ok(());
        };
        document.settings.run_window_timer(timer_id, timer_key, nesting_level)
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

    /// <https://html.spec.whatwg.org/#event-loop-processing-model>
    /// Note: The Rust event-loop worker emits these runtime effects, and each branch below resumes the corresponding Rust-owned continuation.
    fn handle_command(&mut self, command: Command) -> Result<bool, String> {
        match command {
            SetViewport(viewport) => {
                self.set_viewport(viewport);
                Ok(true)
            }
            SetTraversableViewport(viewport) => {
                self.set_traversable_viewport(viewport);
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
            DispatchEvent { events } => {
                self.dispatch_events(events)?;
                Ok(true)
            }
            Command::RunBeforeUnload {
                document_id,
                check_id,
            } => {
                self.run_before_unload(document_id, check_id)?;
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

fn tla_log_server_from_args() -> Result<Option<String>, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--tla-log-server" {
            return args
                .next()
                .map(Some)
                .ok_or_else(|| String::from("missing TLA log server value"));
        }
    }
    Ok(None)
}

pub fn run_content_process(
    token: String,
    _monitor_tx: Option<IpcSender<LogEntry>>,
) -> Result<(), String> {
    let (command_sender, command_receiver) =
        ipc::channel::<Command>().map_err(|error| error.to_string())?;
    let (event_sender, event_receiver) =
        ipc::channel::<ContentEvent>().map_err(|error| error.to_string())?;
    let bootstrap = IpcSender::<Bootstrap>::connect(token).map_err(|error| error.to_string())?;
    bootstrap
        .send(Bootstrap {
            command_sender,
            event_receiver,
        })
        .map_err(|error| error.to_string())?;

    let mut runtime = ContentRuntime::new(event_sender);
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
        match runtime.handle_command(command) {
            Ok(true) => {
                if notify_event_loop {
                    if let Err(error) = runtime.note_command_completed() {
                        eprintln!("content error: {error}");
                    }
                }
            }
            Ok(false) => break,
            Err(error) => {
                eprintln!("content error: {error}");
                if notify_event_loop {
                    if let Err(error) = runtime.note_command_completed() {
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
    let monitor_tx = receive_monitor_sender(tla_log_server_from_args()?.as_deref())?;
    run_content_process(token, monitor_tx)
}