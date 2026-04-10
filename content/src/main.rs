#[allow(dead_code)]
#[path = "../../embedder/src/ui_event.rs"]
mod ui_event;

mod boa;
mod dom;
mod html;
mod webidl;

use crate::boa::{JsHtmlParserProvider, JsState, parse_html_into_document};
use crate::dom::{dispatch_ui_event, fire_event};
use crate::html::run_animation_frame_callbacks;
use crate::ui_event::deserialize_ui_event;
use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, DocumentConfig, NodeData};
use blitz_paint::paint_scene;
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use data_url::DataUrl;
use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::Command::{
    CallbackReady, CompleteDocumentFetch, CreateEmptyDocument, CreateLoadedDocument, DispatchEvent,
    EvaluateScript, SetViewport, Shutdown, UpdateTheRendering,
};
use ipc_messages::content::{
    Bootstrap, CallbackData, ColorScheme as MessageColorScheme, Command, Event as ContentEvent,
    FetchRequest as ContentFetchRequest, FontTransportSender, PaintFrame, RecordedScene,
    ScrollOffset, ViewportSnapshot,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    env,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
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

struct PendingDocumentHandler {
    document_id: u64,
    handler: Box<dyn NetHandler>,
}

struct LocalContentState {
    pending_handlers: HashMap<u64, PendingDocumentHandler>,
    next_handler_id: u64,
}

type LocalContentStateRef = Arc<Mutex<LocalContentState>>;

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
                    PendingDocumentHandler {
                        document_id: self.content_document_id,
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
    js_state: JsState,
    pending_update_the_rendering: bool,
}

struct ContentRuntime {
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
    viewport: Option<ViewportSnapshot>,
    documents: HashMap<u64, ContentDocument>,
    animation_start: Instant,
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
            animation_start: Instant::now(),
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

    /// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>
    /// Note: This resumes the Rust-owned suffix of browsing-context creation after `FormalWeb.UserAgent.queueCreateEmptyDocument` reaches `FormalWeb.EventLoop.runEventLoopMessage` and the FFI emits `CreateEmptyDocument`.
    fn create_empty_document(&mut self, document_id: u64) -> Result<(), String> {
        let document = Rc::new(RefCell::new(BaseDocument::new(self.document_config(document_id, None))));
        let mut js_state = JsState::new(
            Rc::clone(&document),
            Url::parse("about:blank").map_err(|error| error.to_string())?,
        )?;

        // Note: This block continues <https://html.spec.whatwg.org/#creating-a-new-browsing-context>.
        // Step 7: "Mark document as ready for post-load tasks."
        // TODO: Persist the document's post-load readiness state in the DOM/runtime model.

        {
            let mut document_guard = document.borrow_mut();

            // Step 8: "Populate with html/head/body given document."
            // Note: The content runtime drives the shared HTML parser with a fixed `about:blank` skeleton instead of constructing the three elements manually.
            parse_html_into_document(
                &mut document_guard,
                EMPTY_HTML_DOCUMENT,
                &mut js_state.settings.execution_context,
            );
        }

        // Step 10: "Completely finish loading document."
        // Note: Parser task drainage runs the queued work associated with the initial `about:blank` document.
        // TODO: Model the rest of the `completely finish loading` bookkeeping explicitly instead of relying on parser task drainage alone.
        js_state.settings.execution_context.drain_tasks()?;

        // Step 9: "Make active document."
        // Note: The runtime records the document as addressable for future commands by storing it under `document_id` after initialization completes.
        self.documents.insert(
            document_id,
            ContentDocument {
                document,
                js_state,
                pending_update_the_rendering: false,
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
        // Note: `BaseDocument::new` and `JsState::new` split document creation between the DOM carrier and the JavaScript environment settings object.
        let document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(document_id, Some(url.clone())),
        )));
        let mut js_state = JsState::new(
            Rc::clone(&document),
            Url::parse(&url).map_err(|error| error.to_string())?,
        )?;

        {
            let mut document_guard = document.borrow_mut();

            // Step 3: "Otherwise, create an HTML parser and associate it with the document."
            // Note: The embedder has already buffered the response body, so the content runtime feeds it into the parser immediately instead of waiting on separate networking tasks.
            parse_html_into_document(
                &mut document_guard,
                &body,
                &mut js_state.settings.execution_context,
            );
        }

        // Note: This block continues <https://html.spec.whatwg.org/#the-end>.
        // Step 1: "Update the current document readiness to `complete`."
        // Note: Parser task drainage runs the queued end-of-document work before the load event fires.
        js_state.settings.execution_context.drain_tasks()?;

        // Step 5: "Fire an event named `load` at `window`, with legacy target override flag set."
        let window = js_state.settings.execution_context.context.global_object();
        fire_event(
            &mut js_state.settings.execution_context,
            &window,
            "load",
            true,
        )
        .map_err(|error| error.to_string())?;

        // Step 12: "Completely finish loading the `Document`."
        // Step 12.1: "Set document's completely loaded time to the current time."
        // TODO: Persist the document's completely loaded time in the DOM/runtime state.

        // Step 12.2: "Let container be document's node navigable's container."
        // Note: This runtime entry point creates a top-level document, so `container` is null and the container `load` event branch in `completely finish loading` does not run here.

        self.documents.insert(
            document_id,
            ContentDocument {
                document,
                js_state,
                pending_update_the_rendering: false,
            },
        );
        Ok(())
    }

    fn evaluate_script(&mut self, document_id: u64, source: String) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        document
            .js_state
            .settings
            .execution_context
            .evaluate_script(&source)
    }

    fn dispatch_event(&mut self, document_id: u64, event: String) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;

        // Note: This continues <https://dom.spec.whatwg.org/#concept-event-fire> after `FormalWeb.UserAgent.queueDispatchedEvent` hands the serialized UI event to the content runtime.
        let event = deserialize_ui_event(&event)?;
        dispatch_ui_event(
            Rc::clone(&document.document),
            &mut document.js_state.settings.execution_context,
            event,
        )
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
        let frame_timestamp_ms = self.animation_start.elapsed().as_secs_f64() * 1000.0;
        let event_sender = self.event_sender.clone();
        let paint_frame = {
            let document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;

            if document.document.borrow().has_pending_critical_resources() {
                return Ok(());
            }

            // Step 1: "Let `frameTimestamp` be `eventLoop`'s last render opportunity time."
            // Note: The content runtime currently derives a monotonic frame timestamp from process start instead of the HTML event loop's shared render-opportunity clock.

            // Step 14: "For each `doc` of `docs`, run the animation frame callbacks for `doc`, passing in the relative high resolution time given `frameTimestamp` and `doc`'s relevant global object as the timestamp."
            // Note: The content runtime collapses `docs` to the single active document for this content process and uses the same monotonic clock value as both the HTML frame timestamp and the callback timestamp.
            run_animation_frame_callbacks(
                &mut document.js_state.settings.execution_context,
                frame_timestamp_ms,
            )?;

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
        let pending_handler = self
            .local_state
            .lock()
            .expect("local content state mutex poisoned")
            .pending_handlers
            .remove(&handler_id)
            .ok_or_else(|| format!("unknown fetch handler id: {handler_id}"))?;
        pending_handler
            .handler
            .bytes(resolved_url, Bytes::copy_from_slice(&body));
        self.continue_updating_the_rendering(pending_handler.document_id)
    }

    fn callback_ready(
        &mut self,
        document_id: u64,
        callback_id: u64,
        data: CallbackData,
    ) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        document
            .js_state
            .settings
            .execution_context
            .resolve_callback(callback_id, data);
        document.js_state.settings.execution_context.drain_tasks()
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
            EvaluateScript {
                document_id,
                source,
            } => {
                self.evaluate_script(document_id, source)?;
                Ok(true)
            }
            DispatchEvent { document_id, event } => {
                self.dispatch_event(document_id, event)?;
                Ok(true)
            }
            CallbackReady {
                document_id,
                callback_id,
                data,
            } => {
                self.callback_ready(document_id, callback_id, data)?;
                Ok(true)
            }
            UpdateTheRendering { document_id } => {
                self.update_the_rendering(document_id)?;
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
        match runtime.handle_command(command) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => eprintln!("content error: {error}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{JsHtmlParserProvider, JsState, log_paint_debug, parse_html_into_document};
    use anyrender::Scene as RenderScene;
    use blitz_dom::{BaseDocument, DocumentConfig};
    use blitz_paint::paint_scene;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use ipc_messages::content::FontTransportSender;
    use std::{cell::RefCell, fs, rc::Rc, sync::Arc};
    use url::Url;

    #[test]
    fn startup_example_generates_glyph_runs() {
        let artifact_path = format!(
            "{}/../artifacts/StartupExample.html",
            env!("CARGO_MANIFEST_DIR")
        );
        let html = fs::read_to_string(&artifact_path)
        .expect("startup artifact should be readable");
        let artifact_url = Url::from_file_path(&artifact_path)
            .expect("startup artifact path should convert to a file URL");
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(960, 720, 1.0, ColorScheme::Light)),
            base_url: Some(artifact_url.to_string()),
            html_parser_provider: Some(Arc::new(JsHtmlParserProvider)),
            ..DocumentConfig::default()
        })));
        let mut js_state = JsState::new(
            Rc::clone(&document),
            artifact_url,
        )
        .expect("js state should initialize");

        {
            let mut document_guard = document.borrow_mut();
            parse_html_into_document(&mut document_guard, &html, &mut js_state.settings.execution_context);
        }

        js_state
            .settings
            .execution_context
            .drain_tasks()
            .expect("startup scripts should run");

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
}
