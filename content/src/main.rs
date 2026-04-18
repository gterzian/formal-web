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
use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::Command::{
    CompleteDocumentFetch, CreateEmptyDocument, CreateLoadedDocument, DestroyDocument,
    DispatchEvent, EvaluateScript, FailDocumentFetch, RunWindowTimer, SetViewport, Shutdown,
    UpdateTheRendering,
};
use ipc_messages::content::{
    Bootstrap, ColorScheme as MessageColorScheme, Command, DispatchEventEntry,
    Event as ContentEvent, FetchRequest as ContentFetchRequest, FontTransportSender, PaintFrame,
    RecordedScene, ScriptEvaluationResult, ScrollOffset, ViewportSnapshot,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    env,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";

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
    document: Rc<RefCell<BaseDocument>>,
    settings: EnvironmentSettingsObject,
    pending_update_the_rendering: bool,
    pending_document_load: Option<PendingDocumentLoad>,
}

struct ContentRuntime {
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
    viewport: Option<ViewportSnapshot>,
    documents: HashMap<u64, ContentDocument>,
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
    fn create_empty_document(&mut self, document_id: u64) -> Result<(), String> {
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(document_id, None),
        )));
        let mut settings = EnvironmentSettingsObject::new(
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
        execute_parser_scripts(&mut settings, parser_scripts)?;

        // Step 9: "Make active document."
        // Note: The runtime records the document as addressable for future commands by storing it under `document_id` after initialization completes.
        self.documents.insert(
            document_id,
            ContentDocument {
                document,
                settings,
                pending_update_the_rendering: false,
                pending_document_load: None,
            },
        );
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#navigate-html>
    /// Note: This continues the HTML document loading algorithm through the end-of-document load steps and into `completely finish loading`.
    fn create_loaded_document(
        &mut self,
        document_id: u64,
        url: String,
        body: String,
    ) -> Result<(), String> {
        // Note: This block continues <https://html.spec.whatwg.org/#navigate-html>.
        // Step 1: "Let document be the result of creating and initializing a `Document` object given `html`, `text/html`, and navigationParams."
        // Note: `BaseDocument::new` and `EnvironmentSettingsObject::new` split document creation between the DOM carrier and the JavaScript environment settings object.
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(document_id, Some(url.clone())),
        )));
        let settings = EnvironmentSettingsObject::new(
            Rc::clone(&document),
            Url::parse(&url).map_err(|error| error.to_string())?,
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
                document: Rc::clone(&document),
                settings,
                pending_update_the_rendering: false,
                pending_document_load: Some(PendingDocumentLoad {
                    finalize_url: url.clone(),
                    scripts: deferred_scripts,
                }),
            },
        );

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
        document_id: u64,
        source: String,
    ) -> Result<serde_json::Value, String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        document.settings.evaluate_script_to_json(&source)
    }

    fn destroy_document(&mut self, document_id: u64) -> Result<(), String> {
        if let Some(content_document) = self.documents.remove(&document_id) {
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
            let document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;

            // Note: This continues <https://dom.spec.whatwg.org/#concept-event-fire> after `FormalWeb.UserAgent.queueDispatchedEvent` hands the serialized UI event batch to the content runtime.
            let event = deserialize_ui_event(&event)?;
            dispatch_ui_event(
                document_id,
                Rc::clone(&document.document),
                &mut document.settings,
                &self.event_sender,
                event,
            )?;
        }

        Ok(())
    }

    fn run_before_unload(&mut self, document_id: u64, check_id: u64) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        let canceled = !dispatch_window_event(&mut document.settings, "beforeunload", true)
            .map_err(|error| error.to_string())?;
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

    fn update_the_rendering(&mut self, document_id: u64) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        document.pending_update_the_rendering = true;
        self.continue_updating_the_rendering(document_id)
    }

    /// <https://html.spec.whatwg.org/#update-the-rendering>
    /// Note: Lean queues this rendering task via `FormalWeb.UserAgent.queueUpdateTheRendering` and `FormalWeb.EventLoop.runEventLoopMessage`, and the content runtime continues the noted rendering opportunity once critical fetches finish.
    fn continue_updating_the_rendering(&mut self, document_id: u64) -> Result<(), String> {
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
                let viewport_scroll = document_guard.viewport_scroll();
                PaintFrame::new(
                    document_id,
                    ScrollOffset {
                        x: viewport_scroll.x as f32,
                        y: viewport_scroll.y as f32,
                    },
                    scene,
                )?
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
        resolved_url: String,
        body: Vec<u8>,
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
                handler.bytes(resolved_url, Bytes::copy_from_slice(&body));
                if self.documents.contains_key(&document_id) {
                    self.continue_document_load(document_id)?;
                    self.continue_updating_the_rendering(document_id)?;
                }
                Ok(())
            }
            PendingNetworkHandler::DeferredScript {
                document_id,
                script_index,
            } => {
                let _ = resolved_url;
                self.complete_deferred_script_fetch(document_id, script_index, body);
                if self.documents.contains_key(&document_id) {
                    self.continue_document_load(document_id)?;
                    self.continue_updating_the_rendering(document_id)?;
                }
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
                if self.documents.contains_key(&document_id) {
                    self.continue_document_load(document_id)?;
                    self.continue_updating_the_rendering(document_id)?;
                }
                Ok(())
            }
            PendingNetworkHandler::DeferredScript {
                document_id,
                script_index,
            } => {
                self.mark_deferred_script_failed(document_id, script_index);
                if self.documents.contains_key(&document_id) {
                    self.continue_document_load(document_id)?;
                    self.continue_updating_the_rendering(document_id)?;
                }
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
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        document
            .settings
            .run_window_timer(timer_id, timer_key, nesting_level)
    }

    fn note_command_completed(&self) -> Result<(), String> {
        self.event_sender
            .send(ContentEvent::CommandCompleted)
            .map_err(|error| format!("failed to send content command completion: {error}"))
    }

    /// <https://html.spec.whatwg.org/#event-loop-processing-model>
    /// Note: Lean emits these runtime effects from `FormalWeb.EventLoop.runEventLoopMessage`, and each branch below resumes the corresponding Rust-owned continuation.
    fn handle_command(&mut self, command: Command) -> Result<bool, String> {
        match command {
            SetViewport(viewport) => {
                self.set_viewport(viewport);
                Ok(true)
            }
            CreateEmptyDocument { document_id } => {
                self.create_empty_document(document_id)?;
                Ok(true)
            }
            CreateLoadedDocument {
                document_id,
                url,
                body,
            } => {
                self.create_loaded_document(document_id, url, body)?;
                Ok(true)
            }
            DestroyDocument { document_id } => {
                self.destroy_document(document_id)?;
                Ok(true)
            }
            EvaluateScript {
                document_id,
                request_id,
                source,
            } => {
                let value = self.evaluate_script(document_id, source)?;
                let value_json = serde_json::to_string(&value).map_err(|error| {
                    format!("failed to encode script evaluation result: {error}")
                })?;
                self.event_sender
                    .send(ContentEvent::ScriptEvaluated(ScriptEvaluationResult {
                        request_id,
                        value_json,
                        error: None,
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
            UpdateTheRendering { document_id } => {
                self.update_the_rendering(document_id)?;
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
                resolved_url,
                body,
            } => {
                self.complete_document_fetch(handler_id, resolved_url, body)?;
                Ok(true)
            }
            FailDocumentFetch { handler_id } => {
                self.fail_document_fetch(handler_id)?;
                Ok(true)
            }
            Shutdown => Ok(false),
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
        EnvironmentSettingsObject, JsHtmlParserProvider, execute_parser_scripts, log_paint_debug,
        parse_html_into_document,
    };
    use anyrender::Scene as RenderScene;
    use blitz_dom::{BaseDocument, DocumentConfig};
    use blitz_paint::paint_scene;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use ipc_messages::content::FontTransportSender;
    use std::{cell::RefCell, fs, rc::Rc, sync::Arc};
    use url::Url;

    fn blank_environment_settings() -> EnvironmentSettingsObject {
        let base_url = Url::parse("https://example.test/").expect("URL should parse");
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(960, 720, 1.0, ColorScheme::Light)),
            base_url: Some(base_url.to_string()),
            html_parser_provider: Some(Arc::new(JsHtmlParserProvider)),
            ..DocumentConfig::default()
        })));

        EnvironmentSettingsObject::new(document, base_url)
            .expect("environment settings should initialize")
    }

    #[test]
    fn startup_example_generates_glyph_runs() {
        let artifact_path = format!(
            "{}/../artifacts/StartupExample.html",
            env!("CARGO_MANIFEST_DIR")
        );
        let html = fs::read_to_string(&artifact_path).expect("startup artifact should be readable");
        let artifact_url = Url::from_file_path(&artifact_path)
            .expect("startup artifact path should convert to a file URL");
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(960, 720, 1.0, ColorScheme::Light)),
            base_url: Some(artifact_url.to_string()),
            html_parser_provider: Some(Arc::new(JsHtmlParserProvider)),
            ..DocumentConfig::default()
        })));
        let mut settings = EnvironmentSettingsObject::new(Rc::clone(&document), artifact_url)
            .expect("environment settings should initialize");

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();
            parse_html_into_document(&mut document_guard, &html)
        };

        execute_parser_scripts(&mut settings, parser_scripts).expect("startup scripts should run");

        let mut document_guard = document.borrow_mut();
        document_guard.resolve(0.0);
        let viewport = document_guard.viewport().clone();
        let (width, height) = viewport.window_size;
        let mut scene = RenderScene::new();
        paint_scene(
            &mut scene,
            &document_guard,
            viewport.scale_f64(),
            width,
            height,
            0,
            0,
        );
        let mut font_sender = FontTransportSender::default();
        let prepared_scene = font_sender.prepare_scene(1, scene);
        log_paint_debug(1, &document_guard, &prepared_scene.scene);

        let summary = prepared_scene.scene.summary();
        assert!(
            summary.glyph_runs > 0,
            "expected startup scene to include glyph runs, got {}",
            summary.describe()
        );
        assert!(
            summary.glyphs > 0,
            "expected startup scene to include glyphs, got {}",
            summary.describe()
        );
        assert!(
            summary.font_refs > 0,
            "expected startup scene to reference fonts, got {}",
            summary.describe()
        );
        assert!(
            !prepared_scene.registered_fonts.is_empty(),
            "expected startup scene to register fonts for transport"
        );
    }

    #[test]
    fn abort_interfaces_are_exposed_on_window() {
        let mut settings = blank_environment_settings();
        let value = settings
            .evaluate_script_to_json(
                r#"({
                    abortController: typeof AbortController,
                    abortSignal: typeof AbortSignal,
                    domException: typeof DOMException,
                    windowAddEventListener: typeof window.addEventListener,
                    selfAddEventListener: typeof self.addEventListener,
                    abortStatic: typeof AbortSignal.abort,
                    timeoutStatic: typeof AbortSignal.timeout,
                    anyStatic: typeof AbortSignal.any
                })"#,
            )
            .expect("script evaluation should succeed");

        assert_eq!(value["abortController"], "function");
        assert_eq!(value["abortSignal"], "function");
        assert_eq!(value["domException"], "function");
        assert_eq!(value["windowAddEventListener"], "function");
        assert_eq!(value["selfAddEventListener"], "function");
        assert_eq!(value["abortStatic"], "function");
        assert_eq!(value["timeoutStatic"], "function");
        assert_eq!(value["anyStatic"], "function");
    }

    #[test]
    fn abort_signal_abort_returns_aborted_signal() {
        let mut settings = blank_environment_settings();
        let value = settings
            .evaluate_script_to_json(
                r#"(() => {
                    const signal = AbortSignal.abort();
                    return {
                        isSignal: signal instanceof AbortSignal,
                        aborted: signal.aborted,
                        reasonIsDomException: signal.reason instanceof DOMException,
                        reasonName: signal.reason.name
                    };
                })()"#,
            )
            .expect("script evaluation should succeed");

        assert_eq!(value["isSignal"], true);
        assert_eq!(value["aborted"], true);
        assert_eq!(value["reasonIsDomException"], true);
        assert_eq!(value["reasonName"], "AbortError");
    }

    #[test]
    fn abort_controller_construction_returns_signal() {
        let mut settings = blank_environment_settings();
        let value = settings
            .evaluate_script_to_json(
                r#"(() => {
                    const controller = new AbortController();
                    return {
                        controllerIsObject: typeof controller === "object",
                        signalIsSignal: controller.signal instanceof AbortSignal,
                        signalStable: controller.signal === controller.signal,
                        aborted: controller.signal.aborted,
                        reasonIsUndefined: controller.signal.reason === undefined
                    };
                })()"#,
            )
            .expect("script evaluation should succeed");

        assert_eq!(value["controllerIsObject"], true);
        assert_eq!(value["signalIsSignal"], true);
        assert_eq!(value["signalStable"], true);
        assert_eq!(value["aborted"], false);
        assert_eq!(value["reasonIsUndefined"], true);
    }

    #[test]
    fn abort_controller_works_under_testharness() {
        let mut settings = blank_environment_settings();
        settings
            .evaluate_script(
                r#"self.GLOBAL = {
                    isWindow() { return true; },
                    isWorker() { return false; },
                    isShadowRealm() { return false; }
                };"#,
            )
            .expect("GLOBAL should install");

        let harness_path = format!(
            "{}/../vendor/wpt/resources/testharness.js",
            env!("CARGO_MANIFEST_DIR")
        );
        let harness = fs::read_to_string(harness_path).expect("testharness.js should be readable");
        settings
            .evaluate_script(&harness)
            .expect("testharness should evaluate");
        settings
            .evaluate_script(
                r#"test(() => {
                    const controller = new AbortController();
                    assert_true(controller.signal instanceof AbortSignal);
                    assert_false(controller.signal.aborted);
                }, "AbortController testharness integration");
                done();"#,
            )
            .expect("testharness test should run");
    }
}
