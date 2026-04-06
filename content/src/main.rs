#[allow(dead_code)]
#[path = "../../embedder/src/ui_event.rs"]
mod ui_event;

mod boa;
mod dom;

use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_paint::paint_scene;
use blitz_traits::net::{Body, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use ipc_messages::content::{
    Bootstrap, ColorScheme as MessageColorScheme, Command as ContentCommand,
    Event as ContentEvent, FetchRequest as ContentFetchRequest,
    PaintFrame, RecordedScene, ScrollOffset, ViewportSnapshot,
};
use data_url::DataUrl;
use ipc_channel::ipc::{self, IpcSender};
use std::collections::HashMap;
use std::env;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use url::Url;

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";

struct LocalContentState {
    pending_handlers: HashMap<u64, Box<dyn NetHandler>>,
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
    Viewport::new(snapshot.width, snapshot.height, snapshot.scale, color_scheme)
}

#[derive(Clone)]
struct ContentNetProvider {
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
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
                local_state
                    .pending_handlers
                    .insert(handler_id, handler);
                drop(local_state);
                if let Err(error) = self
                    .event_sender
                    .send(ContentEvent::DocumentFetchRequested(ContentFetchRequest {
                        handler_id,
                        url: request.url.to_string(),
                        method: request.method.to_string(),
                        body: request_body_string(&request.body),
                    }))
                {
                    eprintln!("failed to send document fetch request to the embedder: {error}");
                }
            }
        }
    }
}

struct ContentDocument {
    document: std::rc::Rc<std::cell::RefCell<BaseDocument>>,
    js_state: boa::JsState,
}

struct ContentRuntime {
    event_sender: IpcSender<ContentEvent>,
    local_state: LocalContentStateRef,
    viewport: Option<ViewportSnapshot>,
    documents: HashMap<u64, ContentDocument>,
    animation_start: Instant,
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
        }
    }

    fn document_config(&self, base_url: Option<String>) -> DocumentConfig {
        DocumentConfig {
            viewport: self.viewport.as_ref().map(viewport_of_snapshot),
            base_url,
            net_provider: Some(Arc::new(ContentNetProvider {
                event_sender: self.event_sender.clone(),
                local_state: Arc::clone(&self.local_state),
            })),
            html_parser_provider: Some(Arc::new(boa::JsHtmlParserProvider)),
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

    fn create_empty_document(&mut self, document_id: u64) -> Result<(), String> {
        let document = std::rc::Rc::new(std::cell::RefCell::new(BaseDocument::new(
            self.document_config(None),
        )));
        let mut js_state = boa::JsState::new(Rc::clone(&document), Url::parse("about:blank").map_err(|error| error.to_string())?)?;
        {
            let mut document_guard = document.borrow_mut();
            boa::parse_html_into_document(
                &mut document_guard,
                EMPTY_HTML_DOCUMENT,
                &mut js_state.settings.execution_context,
            );
        }
        js_state.settings.execution_context.drain_tasks()?;
        self.documents.insert(
            document_id,
            ContentDocument { document, js_state },
        );
        Ok(())
    }

    fn create_loaded_document(&mut self, document_id: u64, url: String, body: String) -> Result<(), String> {
        let document = std::rc::Rc::new(std::cell::RefCell::new(BaseDocument::new(
            self.document_config(Some(url.clone())),
        )));
        let mut js_state = boa::JsState::new(Rc::clone(&document), Url::parse(&url).map_err(|error| error.to_string())?)?;
        {
            let mut document_guard = document.borrow_mut();
            boa::parse_html_into_document(
                &mut document_guard,
                &body,
                &mut js_state.settings.execution_context,
            );
        }
        js_state.settings.execution_context.drain_tasks()?;
        boa::fire_load_event(&mut js_state.settings.execution_context)?;
        self.documents.insert(
            document_id,
            ContentDocument { document, js_state },
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
        let event = ui_event::deserialize_ui_event(&event)?;
        boa::dispatch_ui_event(
            &document.document,
            &mut document.js_state.settings.execution_context,
            event,
        )
    }

    fn update_the_rendering(&mut self, document_id: u64) -> Result<(), String> {
        let document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;

        let animation_time = self.animation_start.elapsed().as_secs_f64();
        for _ in 0..3 {
            let mut document_guard = document.document.borrow_mut();
            document_guard.resolve(animation_time);
            if !document_guard.has_pending_critical_resources() {
                break;
            }
        }

        let document_guard = document.document.borrow();
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
        let viewport_scroll = document_guard.viewport_scroll();
        self.event_sender
            .send(ContentEvent::PaintReady(PaintFrame {
                document_id,
                viewport_scroll: ScrollOffset {
                    x: viewport_scroll.x as f32,
                    y: viewport_scroll.y as f32,
                },
                scene: RecordedScene::from(scene),
            }))
            .map_err(|error| format!("failed to send paint frame: {error}"))
    }

    fn complete_document_fetch(
        &mut self,
        handler_id: u64,
        resolved_url: String,
        body: Vec<u8>,
    ) -> Result<(), String> {
        let handler = self
            .local_state
            .lock()
            .expect("local content state mutex poisoned")
            .pending_handlers
            .remove(&handler_id)
            .ok_or_else(|| format!("unknown fetch handler id: {handler_id}"))?;
        handler.bytes(resolved_url, Bytes::copy_from_slice(&body));
        Ok(())
    }

    fn callback_ready(
        &mut self,
        document_id: u64,
        callback_id: u64,
        data: ipc_messages::content::CallbackData,
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

    fn handle_command(&mut self, command: ContentCommand) -> Result<bool, String> {
        match command {
            ContentCommand::SetViewport(viewport) => {
                self.set_viewport(viewport);
                Ok(true)
            }
            ContentCommand::CreateEmptyDocument { document_id } => {
                self.create_empty_document(document_id)?;
                Ok(true)
            }
            ContentCommand::CreateLoadedDocument {
                document_id,
                url,
                body,
            } => {
                self.create_loaded_document(document_id, url, body)?;
                Ok(true)
            }
            ContentCommand::EvaluateScript { document_id, source } => {
                self.evaluate_script(document_id, source)?;
                Ok(true)
            }
            ContentCommand::DispatchEvent { document_id, event } => {
                self.dispatch_event(document_id, event)?;
                Ok(true)
            }
            ContentCommand::CallbackReady {
                document_id,
                callback_id,
                data,
            } => {
                self.callback_ready(document_id, callback_id, data)?;
                Ok(true)
            }
            ContentCommand::UpdateTheRendering { document_id } => {
                self.update_the_rendering(document_id)?;
                Ok(true)
            }
            ContentCommand::CompleteDocumentFetch {
                handler_id,
                resolved_url,
                body,
            } => {
                self.complete_document_fetch(handler_id, resolved_url, body)?;
                Ok(true)
            }
            ContentCommand::Shutdown => Ok(false),
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
        ipc::channel::<ContentCommand>().map_err(|error| error.to_string())?;
    let (event_sender, event_receiver) =
        ipc::channel::<ContentEvent>().map_err(|error| error.to_string())?;
    let bootstrap =
        IpcSender::<Bootstrap>::connect(token).map_err(|error| error.to_string())?;
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
