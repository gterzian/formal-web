#[allow(dead_code)]
#[path = "../../embedder/src/ui_event.rs"]
mod ui_event;

mod boa;
mod dom;
mod html;
mod streams;
mod webidl;

use crate::dom::{dispatch_ui_event, dispatch_window_event, fire_event};
use crate::html::{
    EnvironmentSettingsObject, JsHtmlParserProvider, PendingParserScript, execute_parser_scripts,
    parse_html_into_document,
};
use crate::ui_event::deserialize_ui_event;
use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, DocumentConfig, NodeData};
use blitz_paint::paint_scene;
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use data_url::DataUrl;
use html5ever::local_name;
use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::Command::{
    CompleteDocumentFetch, CreateEmptyDocument, CreateLoadedDocument, DestroyDocument,
    DispatchEvent, EvaluateScript, FailDocumentFetch, RunWindowTimer, SetViewport, Shutdown,
    UpdateTheRendering,
};
use ipc_messages::content::{
    Bootstrap, ColorScheme as MessageColorScheme, Command, DispatchEventEntry,
    Event as ContentEvent, FetchRequest as ContentFetchRequest,
    FetchResponse as ContentFetchResponse, FontTransportSender, FrameId,
    LoadedDocumentResponse, PaintFrame, RecordedScene, ScriptEvaluationResult, ScrollOffset,
    ViewportSnapshot,
    WebviewId,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";

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
        document_id: u64,
        request_url: String,
        handler: Box<dyn NetHandler>,
    },
    DeferredScript {
        document_id: u64,
        script_index: usize,
    },
}

struct LocalContentState {
    pending_handlers: HashMap<u64, PendingNetworkHandler>,
    next_handler_id: u64,
}

type LocalContentStateRef = Arc<Mutex<LocalContentState>>;

enum DeferredScriptState {
    Inline { source: String },
    ExternalPending { src: String },
    ExternalReady { source: String },
    ExternalFailed { src: String },
}

#[derive(Clone)]
struct IframeState {
    source_navigable_id: u64,
    current_key: String,
    cross_origin: bool,
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

fn iframe_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_IFRAMES").is_some()
}

fn log_iframe_debug(message: impl AsRef<str>) {
    if iframe_debug_enabled() {
        eprintln!(
            "[iframe-debug][content][pid={}] {}",
            std::process::id(),
            message.as_ref()
        );
    }
}

fn is_subframe_document_id(document_id: u64) -> bool {
    document_id >= (1_u64 << 63)
}

fn render_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_RENDER").is_some()
}

fn log_iframe_layout_debug(document_id: u64, document: &BaseDocument) {
    static LOGGED_IFRAMES: OnceLock<Mutex<HashSet<(u64, usize)>>> = OnceLock::new();

    if !iframe_debug_enabled() {
        return;
    }

    let logged_iframes = LOGGED_IFRAMES.get_or_init(|| Mutex::new(HashSet::new()));
    let mut logged_iframes = logged_iframes
        .lock()
        .expect("iframe layout debug set mutex poisoned");
    let mut iframe_nodes = Vec::new();

    document.visit(|node_id, node| {
        let is_iframe = node
            .element_data()
            .map(|element| {
                *element.name.local == local_name!("iframe")
                    || *element.name.local == local_name!("frame")
            })
            .unwrap_or(false);

        if is_iframe && logged_iframes.insert((document_id, node_id)) {
            iframe_nodes.push((node_id, node.parent, node.layout_parent.get()));
        }
    });
    drop(logged_iframes);

    for (node_id, parent_id, layout_parent_id) in iframe_nodes {
        log_iframe_debug(format!(
            "dump_iframe_layout document={} node={} parent={:?} layout_parent={:?}",
            document_id, node_id, parent_id, layout_parent_id,
        ));
        document.debug_log_node(node_id);

        if let Some(parent_id) = parent_id {
            log_iframe_debug(format!(
                "dump_iframe_parent document={} node={} parent={}",
                document_id, node_id, parent_id,
            ));
            document.debug_log_node(parent_id);
        }

        if let Some(layout_parent_id) = layout_parent_id.filter(|id| Some(*id) != parent_id) {
            log_iframe_debug(format!(
                "dump_iframe_layout_parent document={} node={} layout_parent={}",
                document_id, node_id, layout_parent_id,
            ));
            document.debug_log_node(layout_parent_id);
        }
    }
}

fn log_paint_debug(document_id: u64, document: &BaseDocument, scene: &RecordedScene) {
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
    content_document_id: u64,
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
                let mut local_state = self
                    .local_state
                    .lock()
                    .expect("local content state mutex poisoned");
                let handler_id = local_state.next_handler_id;
                local_state.next_handler_id += 1;
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
    traversable_id: u64,
    document: Rc<RefCell<BaseDocument>>,
    settings: EnvironmentSettingsObject,
    pending_update_the_rendering: bool,
    pending_document_load: Option<PendingDocumentLoad>,
    iframe_states: HashMap<usize, IframeState>,
}

struct ContentRuntime {
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
    viewport: Option<ViewportSnapshot>,
    documents: HashMap<u64, ContentDocument>,
    active_documents_by_traversable: HashMap<u64, u64>,
    next_iframe_navigable_id: u64,
    font_namespace: u64,
    font_sender: FontTransportSender,
}

impl ContentRuntime {
    fn new(event_sender: IpcSender<ContentEvent>) -> Self {
        Self {
            event_sender,
            local_state: Arc::new(Mutex::new(LocalContentState {
                pending_handlers: HashMap::new(),
                next_handler_id: 1,
            })),
            viewport: None,
            documents: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
            next_iframe_navigable_id: 1_u64 << 63,
            font_namespace: new_font_namespace(),
            font_sender: FontTransportSender::default(),
        }
    }

    fn document_config(&self, document_id: u64, base_url: Option<String>) -> DocumentConfig {
        DocumentConfig {
            viewport: self.viewport.as_ref().map(viewport_of_snapshot),
            base_url,
            net_provider: Some(Arc::new(ContentNetProvider {
                event_sender: self.event_sender.clone(),
                local_state: Arc::clone(&self.local_state),
                content_document_id: document_id,
            })),
            html_parser_provider: Some(Arc::new(JsHtmlParserProvider)),
            ..DocumentConfig::default()
        }
    }

    fn set_viewport(&mut self, viewport: ViewportSnapshot) {
        let runtime_viewport = viewport_of_snapshot(&viewport);
        log_iframe_debug(format!(
            "set_viewport width={} height={} scale={} documents={}",
            viewport.width,
            viewport.height,
            viewport.scale,
            self.documents.len()
        ));
        self.viewport = Some(viewport);
        for document in self.documents.values_mut() {
            document
                .document
                .borrow_mut()
                .set_viewport(runtime_viewport.clone());
        }
    }

    fn register_pending_handler(&self, pending_handler: PendingNetworkHandler) -> u64 {
        let mut local_state = self
            .local_state
            .lock()
            .expect("local content state mutex poisoned");
        let handler_id = local_state.next_handler_id;
        local_state.next_handler_id += 1;
        local_state
            .pending_handlers
            .insert(handler_id, pending_handler);
        handler_id
    }

    fn request_remote_fetch(&self, handler_id: u64, request: Request) -> Result<(), String> {
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

    fn mark_deferred_script_failed(&mut self, document_id: u64, script_index: usize) {
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
        document_id: u64,
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
        document_id: u64,
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
        });
        self.request_remote_fetch(handler_id, Request::get(resolved_url))
    }

    fn allocate_iframe_navigable_id(&mut self) -> u64 {
        let source_navigable_id = self.next_iframe_navigable_id;
        self.next_iframe_navigable_id = self.next_iframe_navigable_id.wrapping_add(1);
        source_navigable_id
    }

    fn continue_document_load(&mut self, document_id: u64) -> Result<(), String> {
        let ready_to_finish = {
            let content_document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;

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
            resources_ready && scripts_ready
        };

        if !ready_to_finish {
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
        self.run_iframe_load_event_steps_for_traversable(traversable_id)?;

        self.event_sender
            .send(ContentEvent::FinalizeNavigation(
                ipc_messages::content::FinalizeNavigation {
                    document_id,
                    url: pending_document_load.finalize_url,
                },
            ))
            .map_err(|error| format!("failed to send finalize-navigation event: {error}"))
    }

    /// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>
    /// Note: This resumes the Rust-owned suffix of browsing-context creation after `FormalWeb.UserAgent.queueCreateEmptyDocument` reaches `FormalWeb.EventLoop.runEventLoopMessage` and the FFI emits `CreateEmptyDocument`.
    fn create_empty_document(&mut self, traversable_id: u64, document_id: u64) -> Result<(), String> {
        log_iframe_debug(format!(
            "create_empty_document traversable={} document={}",
            traversable_id, document_id
        ));
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(document_id, None),
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
                document,
                settings,
                pending_update_the_rendering: false,
                pending_document_load: None,
                iframe_states: HashMap::new(),
            },
        );
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
        self.run_iframe_post_connection_steps_for_document(document_id)?;
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
        traversable_id: u64,
        document_id: u64,
        response: LoadedDocumentResponse,
    ) -> Result<(), String> {
        let LoadedDocumentResponse {
            final_url,
            status: _,
            content_type: _,
            body,
        } = response;
        log_iframe_debug(format!(
            "create_loaded_document traversable={} document={} url={} body_len={}",
            traversable_id,
            document_id,
            final_url,
            body.len()
        ));
        // Note: This block continues <https://html.spec.whatwg.org/#navigate-html>.
        // Step 1: "Let document be the result of creating and initializing a `Document` object given `html`, `text/html`, and navigationParams."
        // Note: `BaseDocument::new` and `EnvironmentSettingsObject::new` split document creation between the DOM carrier and the JavaScript environment settings object.
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(document_id, Some(final_url.clone())),
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
                document: Rc::clone(&document),
                settings,
                pending_update_the_rendering: false,
                pending_document_load: Some(PendingDocumentLoad {
                    finalize_url: final_url.clone(),
                    scripts: deferred_scripts,
                }),
                iframe_states: HashMap::new(),
            },
        );
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
        self.run_iframe_post_connection_steps_for_document(document_id)?;

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
        traversable_id: u64,
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

    fn destroy_document(&mut self, document_id: u64) -> Result<(), String> {
        log_iframe_debug(format!(
            "destroy_document document={}",
            document_id
        ));
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
            for iframe_state in content_document.iframe_states.values() {
                self.retire_iframe_traversable(content_document.traversable_id, iframe_state)?;
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

            // Note: This continues <https://dom.spec.whatwg.org/#concept-event-fire> after `FormalWeb.UserAgent.queueDispatchedEvent` hands the serialized UI event batch to the content runtime.
            let event = deserialize_ui_event(&event)?;
            dispatch_ui_event(
                document_id,
                document.traversable_id,
                Rc::clone(&document.document),
                &mut document.settings,
                &self.event_sender,
                event,
            )?;
        }

        Ok(())
    }

    fn run_before_unload(&mut self, document_id: u64, check_id: u64) -> Result<(), String> {
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

    fn update_the_rendering(&mut self, traversable_id: u64, document_id: u64) -> Result<(), String> {
        let Some(document) = self.documents.get_mut(&document_id) else {
            return Ok(());
        };
        document.pending_update_the_rendering = true;
        self.continue_updating_the_rendering(traversable_id, document_id)
    }

    /// <https://html.spec.whatwg.org/#update-the-rendering>
    /// Note: Lean queues this rendering task via `FormalWeb.UserAgent.queueUpdateTheRendering` and `FormalWeb.EventLoop.runEventLoopMessage`, and the content runtime continues the noted rendering opportunity once critical fetches finish.
    fn continue_updating_the_rendering(
        &mut self,
        traversable_id: u64,
        document_id: u64,
    ) -> Result<(), String> {
        let event_sender = self.event_sender.clone();
        let paint_frame = {
            let document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;

            document.document.borrow_mut().handle_messages();

            if document.document.borrow().has_pending_critical_resources() {
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
                log_iframe_layout_debug(document_id, &document_guard);
                let scene_summary = scene.scene.summary();
                let viewport_scroll = document_guard.viewport_scroll();
                let paint_frame = PaintFrame::new(
                    WebviewId(traversable_id),
                    FrameId(document_id),
                    width,
                    height,
                    ScrollOffset {
                        x: viewport_scroll.x as f32,
                        y: viewport_scroll.y as f32,
                    },
                    scene,
                )?;
                if is_subframe_document_id(document_id)
                    || scene_summary.iframe_placeholder_commands > 0
                {
                    let transport = paint_frame.transport_summary();
                    log_iframe_debug(format!(
                        "paint_ready traversable={} document={} viewport={}x{} scroll=({:.1}, {:.1}) scene={} transport(scene_bytes={} fonts={} font_bytes={})",
                        traversable_id,
                        document_id,
                        width,
                        height,
                        viewport_scroll.x,
                        viewport_scroll.y,
                        scene_summary.describe(),
                        transport.scene_bytes,
                        transport.registered_fonts,
                        transport.registered_font_bytes,
                    ));
                }
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
        handler_id: u64,
        response: ContentFetchResponse,
    ) -> Result<(), String> {
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
                self.continue_document_load(document_id)?;
                self.continue_updating_the_rendering(traversable_id, document_id)?;
                Ok(())
            }
        }
    }

    fn fail_document_fetch(&mut self, handler_id: u64) -> Result<(), String> {
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
                self.continue_document_load(document_id)?;
                self.continue_updating_the_rendering(traversable_id, document_id)?;
                Ok(())
            }
        }
    }

    fn run_window_timer(
        &mut self,
        document_id: u64,
        timer_id: u32,
        timer_key: u64,
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
    /// Note: Lean emits these runtime effects from `FormalWeb.EventLoop.runEventLoopMessage`, and each branch below resumes the corresponding Rust-owned continuation.
    fn handle_command(&mut self, command: Command) -> Result<bool, String> {
        match command {
            SetViewport(viewport) => {
                self.set_viewport(viewport);
                Ok(true)
            }
            CreateEmptyDocument {
                traversable_id,
                document_id,
            } => {
                self.create_empty_document(traversable_id, document_id)?;
                Ok(true)
            }
            CreateLoadedDocument {
                traversable_id,
                document_id,
                response,
            } => {
                self.create_loaded_document(traversable_id, document_id, response)?;
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

fn content_token() -> Result<String, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--content-token" {
            return args
                .next()
                .ok_or_else(|| String::from("missing content token value"));
        }
    }
    Err(String::from("missing --content-token argument"))
}

fn main() -> Result<(), String> {
    let token = content_token()?;
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

#[cfg(test)]
mod tests {
    use super::{
        ContentFetchResponse, deferred_script_response_is_executable,
        is_javascript_mime_essence, normalized_content_type_essence,
    };

    fn response(status: u16, content_type: &str) -> ContentFetchResponse {
        ContentFetchResponse {
            final_url: String::from("https://example.test/script.js"),
            status,
            content_type: content_type.to_string(),
            body: b"console.log('ok');".to_vec(),
        }
    }

    #[test]
    fn deferred_scripts_accept_successful_javascript_mime() {
        assert!(deferred_script_response_is_executable(&response(
            200,
            "text/javascript; charset=utf-8"
        )));
    }

    #[test]
    fn deferred_scripts_reject_non_success_status() {
        assert!(!deferred_script_response_is_executable(&response(
            404,
            "text/javascript"
        )));
    }

    #[test]
    fn deferred_scripts_reject_clearly_wrong_mime() {
        assert!(!deferred_script_response_is_executable(&response(
            200,
            "text/html"
        )));
    }

    #[test]
    fn deferred_scripts_allow_missing_content_type() {
        assert!(deferred_script_response_is_executable(&response(200, "")));
    }

    #[test]
    fn content_type_essence_is_case_and_parameter_insensitive() {
        let essence = normalized_content_type_essence("Application/JavaScript; Charset=UTF-8");
        assert_eq!(essence, "application/javascript");
        assert!(is_javascript_mime_essence(&essence));
    }
}