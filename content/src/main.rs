#[path = "../../webview/src/ui_event.rs"]
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
pub mod ui_events;
#[cfg(all(boa_backend, feature = "wasm"))]
pub mod wasm;
pub mod webidl;

use crate::dom::{EventTargetAccess, dispatch_with_path, fire_event, simple_path};
use crate::html::environment_settings_object::RealmWiring;
use crate::html::event_loop::{EventLoopTaskSources, command_is_event_loop_task};
use crate::html::timers::MapOfActiveTimers;
use crate::html::ui_events::{dispatch_trusted_click_event, dispatch_ui_event};
use crate::html::{
    EnvironmentSettingsObject, JsHtmlParserProvider, MessageEvent, PendingParserScript, Window,
    attach_same_origin_child_document_for_traversable, execute_parser_scripts,
    parse_html_into_document, run_dom_post_connection_steps_for_document,
    run_dom_removing_steps_for_document, run_iframe_load_event_steps_for_traversable,
    structured_data::safe_passing_of_structured_data::{
        SerializeWithTransferResult, structured_deserialize_with_transfer,
    },
    windowproxy::WindowProxyBacking,
};
use crate::infra::strip_and_collapse_ascii_whitespace;
use crate::js::Engine;
use crate::js::downcast::try_with_event_target_mut;
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
use js_engine::{EcmascriptHost, ExecutionContext, JsTypes};

use ipc_messages::content::Command::{
    ClickElement, CompleteDocumentFetch, ContentBootstrap, CreateEmptyDocument,
    CreateLoadedDocument, DestroyDocument, DispatchEvent, EvaluateScript, FailDocumentFetch,
    NotifyVideoEnded, RunWindowTimer, SetTraversableViewport, SetViewport, Shutdown,
    UpdateTheRendering,
};
use ipc_messages::content::{
    BeforeUnloadCheckId, ClipboardWriteRequested, ColorScheme as MessageColorScheme, Command,
    DispatchEventEntry, DocumentFetchId, DocumentId, ElementClickResult, EmbedBackgroundPolicy,
    EmbedLayout, EmbedSite, EmbedSiteId, Event as ContentEvent, EventLoopId,
    FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse,
    FontTransportSender, FrameCompositionMetadata, FrameId, IframeEmbedSite,
    LoadedDocumentResponse, NavigableId, NavigationId, PaintFrame, PortId, PortTaskKind,
    PreparedScene, RecordedScene, ScriptEvaluationResult, TitleChanged, TraversableViewport,
    ViewportSnapshot, WebviewId, WindowTimerKey,
};
use ipc_messages::media::{VideoEmbedData, VideoPaintId};
use ipc_messages::safe_passing_of_structured_data::PostMessageRequest;
use log::{debug, error, info, warn};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;
use verification::{TLATracer, TraceSender};

use crate::webidl::bindings::create_interface_instance;

type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

pub(crate) const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";

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

/// Milliseconds since the Unix epoch of `instant`, measured on the
/// monotonic clock shared with the user agent process.
fn epoch_millis(epoch_anchor: Instant, epoch_anchor_wall_ms: f64, instant: Instant) -> f64 {
    epoch_anchor_wall_ms
        + instant
            .saturating_duration_since(epoch_anchor)
            .as_secs_f64()
            * 1000.0
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

/// Tracks the playback state of a video element across paints.
/// Stored in the paint registry instead of on the element for now.
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
    /// Per-document flag: set when blitz asks for a repaint (a resource
    /// loaded, an event mutated the DOM, the viewport changed). Content reads
    /// it in `update_the_rendering` to decide whether to re-run blitz
    /// (resolve + paint) — a static document that keeps re-sending an
    /// identical scene during a video-driven render cycle skips the blitz
    /// work entirely.
    ///
    /// The `Arc<AtomicBool>` (rather than an `Rc<RefCell<bool>>` or a single
    /// `bool`) is a blitz shell-provider constraint, not our own cross-thread
    /// sharing: blitz's `ShellProvider` trait is `Send + Sync`, so this
    /// `ContentShellProvider` must be `Send + Sync`, which forces the flag to
    /// be a thread-safe shared value. It is shared between the
    /// `ContentShellProvider` that blitz calls `request_redraw()` on and the
    /// `ContentDocument` that `update_the_rendering` reads.
    needs_paint: Arc<AtomicBool>,
}

impl ContentShellProvider {
    fn new(
        event_sender: ipc::IpcSender<ContentEvent>,
        clipboard_cache: ClipboardCache,
        needs_paint: Arc<AtomicBool>,
    ) -> Self {
        Self {
            event_sender,
            clipboard_cache,
            needs_paint,
        }
    }
}

impl ShellProvider for ContentShellProvider {
    fn request_redraw(&self) {
        self.needs_paint.store(true, Ordering::Relaxed);
    }

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
    /// Whether the child document finished loading before the parent's
    /// document load completion ("the end", steps 5-9); when true, the
    /// iframe load event steps of "completely finish loading" step 4 are
    /// held back and run after the parent's load completion, once its
    /// deferred parser scripts have executed.
    pub(crate) child_document_loaded: bool,
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

fn log_render_state_debug(message: impl AsRef<str>) {
    if render_state_debug_enabled() {
        debug!("[render-state][content] {}", message.as_ref());
    }
}

#[derive(Clone)]
struct ContentNetProvider {
    local_state: LocalContentStateRef,
    content_document_id: DocumentId,
    event_loop_id: EventLoopId,
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
                    event_loop_id: self.event_loop_id,
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
    pending_document_load: Option<PendingDocumentLoad>,
    navigable_container_states: HashMap<usize, NavigableContainerState>,
    viewport_offset_x: f32,
    viewport_offset_y: f32,
    /// Set whenever the document may have changed and needs a blitz render
    /// (a script ran, an event was dispatched, a resource loaded, or blitz
    /// requested a repaint). Cleared after a render is produced. `update_the_rendering`
    /// skips the blitz resolve + paint when this is clear and the document is
    /// not animating — so a static document that keeps being asked to render
    /// (e.g. a video elsewhere drives the render cycle) does not re-run blitz
    /// on an unchanged scene.
    needs_paint: Arc<AtomicBool>,
    /// The last recorded scene, reused verbatim on a clean render cycle so the
    /// graphics process keeps the content layer clean (an unchanged scene is
    /// not re-rasterized) instead of re-painting an unchanged document. Fonts
    /// are not re-sent on a clean cycle (they are already registered on the
    /// graphics side).
    last_scene: Option<RecordedScene>,
    /// The last frame composition metadata, reused when the scene is clean
    /// (embed sites, viewport, and frame id are unchanged when nothing dirtied
    /// the document).
    last_composition: Option<FrameCompositionMetadata>,
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
    tla_tracer: TLATracer,
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
    /// (DocumentId, node_id) pairs of video elements that have reached
    /// end-of-stream. Checked alongside the registry to determine whether
    /// the document has active (non-ended) video.
    ended_video_nodes: HashSet<(DocumentId, usize)>,
    /// Direct sender to the net extension. Set during DirectChannelsSetup.
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    /// Direct sender to the graphics process (composition + media). Set during ContentBootstrap.
    graphics_sender: Option<ipc::IpcSender<ipc_messages::graphics::GraphicsCommand>>,
    /// This content process's own command sender, used by net for direct response routing.
    content_command_sender: ipc::IpcSender<Command>,
    /// Monotonic-clock reading captured at the same moment as
    /// `epoch_anchor_wall_ms`; together they convert monotonic readings to
    /// epoch-relative milliseconds on the clock shared with the user agent
    /// (HR Time "estimated monotonic time of the Unix epoch").
    epoch_anchor: Instant,
    /// Wall-clock milliseconds since the Unix epoch at the moment
    /// `epoch_anchor` was captured.
    epoch_anchor_wall_ms: f64,
    /// Raw TLA trace sender, forwarded to each realm's GlobalScope for the
    /// MessagePort spec trace (the Navigation tracer above is separate).
    trace_sender: Option<TraceSender>,
    realm_parent: Engine,
    /// <https://html.spec.whatwg.org/#map-of-active-timers>
    active_timers: Rc<RefCell<MapOfActiveTimers>>,
    /// <https://html.spec.whatwg.org/#task-queue>
    /// Note: Every task source shares this one queue: `queue_a_task` appends
    /// to it through `task_sender`, and the event loop processing model's step
    /// 2.3 takes the oldest task from `task_receiver`.
    task_sender: crossbeam_channel::Sender<Command>,
    /// <https://html.spec.whatwg.org/#task-queue>
    task_receiver: crossbeam_channel::Receiver<Command>,
}

impl ContentProcess {
    fn new(
        event_sender: ipc::IpcSender<ContentEvent>,
        _wasm_signal_sender: crossbeam_channel::Sender<()>,
        event_loop_id: EventLoopId,
        network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
        graphics_sender: Option<ipc::IpcSender<ipc_messages::graphics::GraphicsCommand>>,
        content_command_sender: ipc::IpcSender<Command>,
        trace_sender: Option<TraceSender>,
    ) -> Self {
        let clipboard_cache = new_clipboard_cache();
        let (task_sender, task_receiver) = crossbeam_channel::unbounded();
        // HR Time "estimated monotonic time of the Unix epoch": simultaneous
        // wall-clock and monotonic readings at process start, so epoch
        // timestamps sent by the user agent (e.g. the rendering opportunity
        // time in UpdateTheRendering) map onto this process's monotonic clock.
        let epoch_anchor = Instant::now();
        let epoch_anchor_wall_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
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
            tla_tracer: TLATracer::new("Navigation", "formal-web:content", trace_sender.clone()),
            clipboard_cache: clipboard_cache.clone(),
            new_document_registry: Rc::new(RefCell::new(HashMap::new())),
            video_paint_registry: Rc::new(RefCell::new(HashMap::new())),
            ended_video_nodes: HashSet::new(),
            #[cfg(all(boa_backend, feature = "wasm"))]
            wasm: crate::wasm::ContentWasmState::new(_wasm_signal_sender),
            network_extension_sender,
            graphics_sender,
            content_command_sender,
            epoch_anchor,
            epoch_anchor_wall_ms,
            trace_sender,
            realm_parent: Engine::new(),
            active_timers: Rc::new(RefCell::new(MapOfActiveTimers::default())),
            task_sender,
            task_receiver,
        }
    }

    fn create_environment_settings_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        creation_url: Url,
        traversable_id: NavigableId,
        document_id: DocumentId,
    ) -> Result<EnvironmentSettingsObject, String> {
        let event_sender = self.event_sender.clone();
        let task_sources = self.event_loop_task_sources();
        let mut settings = EnvironmentSettingsObject::new_in_realm(
            Some(&mut self.realm_parent),
            document,
            creation_url,
            None,
            Some(RealmWiring {
                source_navigable_id: traversable_id,
                document_id,
                event_sender,
                task_sources,
            }),
        )?;
        // The realm belongs to this content process's event loop; the global
        // scope needs the id for channel messaging (per-event-loop port
        // management) and the trace sender for the MessagePort TLA spec.
        let trace_sender = self.trace_sender.clone();
        with_global_scope(
            &mut settings.realm_execution_context,
            |global_scope, _ec| {
                global_scope.set_event_loop_id(self.event_loop_id);
                global_scope.set_trace_sender(trace_sender.clone());
                Ok(())
            },
        )
        .map_err(|error| format!("failed to set event loop id: {}", error.display()))?;
        Ok(settings)
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
        needs_paint: Arc<AtomicBool>,
    ) -> DocumentConfig {
        DocumentConfig {
            viewport: self
                .document_viewport_state(traversable_id)
                .map(|viewport| viewport_of_snapshot(&viewport.snapshot)),
            base_url,
            net_provider: Some(Arc::new(ContentNetProvider {
                local_state: Arc::clone(&self.local_state),
                content_document_id: document_id,
                event_loop_id: self.event_loop_id,
                network_extension_sender: self.network_extension_sender.clone(),
                content_command_sender: self.content_command_sender.clone(),
            })),
            shell_provider: Some(Arc::new(ContentShellProvider::new(
                self.event_sender.clone(),
                self.clipboard_cache.clone(),
                needs_paint,
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

        // A viewport change resizes the document surface; mark it dirty so the
        // next update-the-rendering re-paints instead of reusing the stale-size
        // cached scene.
        document.needs_paint.store(true, Ordering::Relaxed);
        document
            .document
            .borrow_mut()
            .set_viewport(viewport_of_snapshot(&viewport_state.snapshot));
        document.viewport_offset_x = viewport_state.offset_x;
        document.viewport_offset_y = viewport_state.offset_y;

        // The UA notes a rendering opportunity after sending this command,
        // so embed-site geometry (including iframe clip/transform) will be
        // repainted on the next UpdateTheRendering cycle.
        Ok(())
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
            event_loop_id: self.event_loop_id,
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
        with_global_scope(content_document.settings.ec(), |global_scope, _ec| {
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
        with_global_scope(content_document.settings.ec(), |global_scope, _ec| {
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
            let new_traversable_id = with_global_scope(settings.ec(), |global_scope, _ec| {
                Ok(global_scope.source_navigable_id())
            })
            .map_err(|error| format!("failed to read new traversable id: {}", error.display()))?
            .unwrap_or_else(NavigableId::new);
            // The new realm shares this process's event loop and trace
            // sender (channel messaging needs both).
            let trace_sender = self.trace_sender.clone();
            with_global_scope(settings.ec(), |global_scope, _ec| {
                global_scope.set_event_loop_id(self.event_loop_id);
                global_scope.set_trace_sender(trace_sender.clone());
                Ok(())
            })
            .map_err(|error| format!("failed to set event loop id: {}", error.display()))?;

            self.documents.insert(
                document_id,
                ContentDocument {
                    traversable_id: new_traversable_id,
                    parent_traversable_id,
                    top_level_traversable_id,
                    frame_id,
                    document,
                    settings,
                    pending_document_load: None,
                    navigable_container_states: HashMap::new(),
                    viewport_offset_x: 0.0,
                    viewport_offset_y: 0.0,
                    needs_paint: Arc::new(AtomicBool::new(false)),
                    last_scene: None,
                    last_composition: None,
                },
            );
            self.active_documents_by_traversable
                .insert(new_traversable_id, document_id);
            // Set up the shared registry so window.open calls made by this
            // new traversable's scripts can register further documents.
            if let Err(error) = self.set_up_new_document_registry(new_traversable_id) {
                warn!("failed to set up new document registry: {error}");
            }
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
        // Set up the shared registry so window.open calls made by the inline
        // and deferred scripts running during the load can register new
        // traversable documents.
        let load_traversable_id = self
            .documents
            .get(&document_id)
            .map(|document| document.traversable_id);
        if let Some(traversable_id) = load_traversable_id {
            if let Err(error) = self.set_up_new_document_registry(traversable_id) {
                warn!("failed to set up new document registry: {error}");
            }
        }

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

        {
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
        }

        // Tear down the shared registry and drain any traversable documents
        // created by the load's scripts (window.open) into this process's
        // document tables so the user agent's navigation continuations can
        // address them.
        if let Err(error) = self.tear_down_new_document_registry(traversable_id) {
            warn!("failed to tear down new document registry: {error}");
        }
        if let Err(error) = self.drain_new_traversable_documents() {
            warn!("failed to drain new traversable documents: {error}");
        }

        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        let window = content_document
            .settings
            .realm_execution_context
            .realm_global_object();
        let time_millis = content_document.settings.current_time_millis();
        let ec = &mut content_document.settings.realm_execution_context;

        let window_target = ec
            .with_object_any(&window)
            .and_then(|data| data.downcast_ref::<crate::html::Window>().cloned())
            .map(|w| w.get_event_target(ec))
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
        // Fire the iframe load events the child load completions deferred
        // until the parent's load completion (the parent's deferred parser
        // scripts have now run, so the iframe's onload handler can resolve
        // them).
        crate::html::fire_deferred_iframe_load_events(self, document_id)?;
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
        // The UA handles the rendering opportunity upon receiving
        // FinalizeNavigation, so we don't request one here.
        Ok(())
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
        let needs_paint = Arc::new(AtomicBool::new(false));
        let document = Rc::new(RefCell::new(BaseDocument::new(self.document_config(
            traversable_id,
            document_id,
            None,
            needs_paint.clone(),
        ))));
        let mut settings = self.create_environment_settings_object(
            Rc::clone(&document),
            Url::parse("about:blank").map_err(|error| error.to_string())?,
            traversable_id,
            document_id,
        )?;

        // Set the video-paint registry on GlobalScope so that
        // resource_selection_algorithm can register paint IDs.
        if let Err(error) = with_global_scope(settings.ec(), |global_scope, _ec| {
            global_scope.set_video_paint_registry(Rc::clone(&self.video_paint_registry));
            if let Some(ref sender) = self.graphics_sender {
                global_scope.set_graphics_sender(sender.clone());
            }
            Ok(())
        }) {
            error!(
                "[media] failed to set video paint registry on GlobalScope: {}",
                error.display()
            );
        }

        // This block continues <https://html.spec.whatwg.org/#creating-a-new-browsing-context>.
        // Step 21: "Mark document as ready for post-load tasks."
        // TODO: Persist the document's post-load readiness state in the DOM model.
        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 22: "Populate with html/head/body given document."
            // The content process drives the shared HTML parser with a fixed `about:blank` skeleton.
            parse_html_into_document(&mut document_guard, EMPTY_HTML_DOCUMENT)
        };

        // Step 24: "Completely finish loading document."
        // Execute parser-discovered classic scripts after the initial tree build.
        // TODO: Model the rest of the `completely finish loading` bookkeeping explicitly instead of relying on parser-discovered script execution alone.
        // Step 23: "Make active document."
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
                needs_paint,
                last_scene: None,
                last_composition: None,
            },
        );
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);

        // Set the navigable hierarchy on the GlobalScope so that `window.open`
        // can resolve `_parent`/`_top` targets.
        self.set_navigable_hierarchy_on_global_scope(document_id)?;

        // Set up the shared registry so window.open calls made by the parser
        // scripts can register new traversable documents.
        if let Err(error) = self.set_up_new_document_registry(traversable_id) {
            warn!("failed to set up new document registry: {error}");
        }

        run_dom_post_connection_steps_for_document(self, document_id)?;
        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        execute_parser_scripts(&mut content_document.settings, parser_scripts)?;
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#initialise-the-document-object>
    fn initialise_the_document_object(
        &mut self,
        traversable_id: NavigableId,
        document_id: DocumentId,
        final_url: &str,
    ) -> Result<
        (
            Rc<RefCell<BaseDocument>>,
            EnvironmentSettingsObject,
            Arc<AtomicBool>,
        ),
        String,
    > {
        // Step 1: "Let browsingContext be the result of obtaining a browsing context to use for
        // a navigation response given navigationParams."
        // Note: Ran in the user agent: `UserAgent::initialise_the_document_object` resolved
        // the browsing context and, for cross-origin loads, moved the traversable to a new
        // event loop (a fresh content process).  This function runs in the process that
        // receives the CreateLoadedDocument command.
        // Step 2: "Let permissionsPolicy be the result of creating a permissions policy from a
        // response given navigationParams's navigable's container, navigationParams's origin,
        // and navigationParams's response."
        // Note: Not implemented: permissions policy is not tracked.
        // Step 3: "Let creationURL be navigationParams's response's URL."
        // Step 4: "If navigationParams's request is non-null, then set creationURL to
        // navigationParams's request's current URL."
        // Note: Redirect chains are collapsed by the fetch layer: `final_url` is the
        // response's final URL.
        // Step 5: "Let window be null."
        // Step 6: "If browsingContext's active document's is initial about:blank is true, and
        // browsingContext's active document's origin is same origin-domain with
        // navigationParams's origin, then set window to browsingContext's active window."
        // Note: Implemented here: when the traversable's active document is the initial
        // about:blank in this process and the destination is same-origin with it (the
        // same-origin-domain condition is approximated by a same-origin comparison —
        // document.domain is not modeled), the document below is created in the existing
        // realm/Window instead of a fresh one: `take_initial_about_blank_settings` moves the
        // initial about:blank document's settings object (realm/Window) out of
        // `self.documents`, and `repoint_document` re-points it at the new document.  The
        // WindowProxy keeps its identity because the realm is unchanged; a cross-origin
        // destination, or an opaque-origin initial about:blank (a fresh top-level tab with
        // no creator), falls through to step 7 and gets a fresh realm as before.
        let reused_settings = self.take_initial_about_blank_settings(traversable_id, final_url);
        // Step 7: "Otherwise:"
        // Step 7.1: "Let oacHeader be the result of getting a structured field value given
        // `Origin-Agent-Cluster` and "item" from navigationParams's response's header list."
        // Step 7.2: "Let requestsOAC be true if oacHeader is not null and oacHeader[0] is the
        // boolean true; otherwise false."
        // Step 7.3: "If navigationParams's reserved environment is a non-secure context, then
        // set requestsOAC to false."
        // Note: Steps 7.1-7.3 are not implemented: Origin-Agent-Cluster is not tracked.
        // Step 7.4: "Let agent be the result of obtaining a similar-origin window agent given
        // navigationParams's origin, browsingContext's group, and requestsOAC."
        // Note: Ran in the user agent: for same-process loads this event loop's agent is
        // reused; for cross-origin loads the traversable was moved to a fresh agent (new
        // content process) before CreateLoadedDocument was dispatched.
        // Step 7.5: "Let realmExecutionContext be the result of creating a new realm given
        // agent and the following customizations: For the global object, create a new Window
        // object. For the global this binding, use browsingContext's WindowProxy object."
        // Step 7.6: "Set window to the global object of realmExecutionContext's Realm
        // component."
        // Step 7.10: "Set up a window environment settings object with creationURL,
        // realmExecutionContext, navigationParams's reserved environment, topLevelCreationURL,
        // and topLevelOrigin."
        // Note: Steps 7.5, 7.6 and 7.10 run together in
        // `create_environment_settings_object`: it creates the realm, the Window and the
        // environment settings object as one unit.  Steps 7.7-7.9 (top-level creation URL and
        // origin) are not implemented.
        // Step 8: "Let loadTimingInfo be a new document load timing info with its navigation
        // start time set to navigationParams's response's timing info's start time."
        // Note: Not implemented: load timing info is not tracked.
        // Step 9: "Let document be a new Document, with: type type; content type contentType;
        // origin navigationParams's origin; browsing context browsingContext; ..."
        // Note: `BaseDocument::new` builds the domain Document; the Document properties
        // (origin, policy container, permissions policy, sandboxing flags, ...) are not
        // implemented.
        // Step 10: "Set window's associated Document to document."
        // Note: The settings object ties the domain Document to the realm; the platform
        // Document object is stored on the GlobalScope.
        // Step 11: "Set document's internal ancestor origin objects list to the result of
        // running the internal ancestor origin objects list creation steps given document and
        // navigationParams's iframe element referrer policy."
        // Step 12: "Set document's ancestor origins list to the result of running the
        // ancestor origins list creation steps given document."
        // Step 13: "Run CSP initialization for a Document given document."
        // Step 14: "If navigationParams's request is non-null: ... Set document's referrer ..."
        // Step 15: "If navigationParams's fetch controller is not null: ... Create the
        // navigation timing entry ..."
        // Step 16: "Create the navigation timing entry for document ..."
        // Step 17: "If navigationParams's response has a `Refresh` header: ..."
        // Step 18: "If navigationParams's commit early hints is not null, then call
        // navigationParams's commit early hints with document."
        // Step 19: "Process link headers given document, navigationParams's response, and
        // "pre-media"."
        // Step 20: "If navigationParams's navigable is a top-level traversable, then process
        // the `Speculation-Rules` header given document and navigationParams's response."
        // Step 21: "Potentially free deferred fetch quota for document."
        // Note: Steps 11-21 are not implemented.
        // Step 22: "Return document."
        // Note: The document and settings object are returned to the caller
        // (`create_loaded_document`), which continues navigate-html with the parse.
        let creation_url = Url::parse(final_url).map_err(|error| error.to_string())?;
        let needs_paint = Arc::new(AtomicBool::new(false));
        let document = Rc::new(RefCell::new(BaseDocument::new(self.document_config(
            traversable_id,
            document_id,
            Some(final_url.to_string()),
            needs_paint.clone(),
        ))));
        // Steps 7.5, 7.6 and 7.10 run in `create_environment_settings_object` for the
        // otherwise branch; for the step-6 branch they already ran when the reused realm was
        // created, and `repoint_document` re-points that realm at the new document.
        let mut settings = match reused_settings {
            Some(mut reused) => {
                reused.repoint_document(Rc::clone(&document), creation_url, document_id)?;
                reused
            }
            None => self.create_environment_settings_object(
                Rc::clone(&document),
                creation_url,
                traversable_id,
                document_id,
            )?,
        };

        // The video-paint registry and graphics sender are engine integration, not spec
        // steps: they wire the document's GlobalScope to the media and graphics pipelines.
        if let Err(error) = with_global_scope(settings.ec(), |global_scope, _ec| {
            global_scope.set_video_paint_registry(Rc::clone(&self.video_paint_registry));
            if let Some(ref sender) = self.graphics_sender {
                global_scope.set_graphics_sender(sender.clone());
            }
            Ok(())
        }) {
            error!(
                "[media] failed to set video paint registry on GlobalScope: {}",
                error.display()
            );
        }

        Ok((document, settings, needs_paint))
    }

    /// <https://html.spec.whatwg.org/#initialise-the-document-object> — step 6
    /// Returns the active document's settings object (realm/Window) when it can be reused
    /// for the new document: the traversable's active document is the initial about:blank in
    /// this process and the destination is same-origin with it.  The settings object is moved
    /// out of `self.documents`; the caller re-points it at the new document (the old
    /// document wrapper and its DOM are dropped).  The later DestroyDocument for the old
    /// document becomes a no-op because it is no longer registered.
    fn take_initial_about_blank_settings(
        &mut self,
        traversable_id: NavigableId,
        final_url: &str,
    ) -> Option<EnvironmentSettingsObject> {
        let active_document_id = *self.active_documents_by_traversable.get(&traversable_id)?;
        let (is_initial_about_blank, active_origin) = {
            let active_document = self.documents.get(&active_document_id)?;
            (
                active_document.settings.creation_url.as_str() == "about:blank",
                active_document.settings.origin.serialized.clone(),
            )
        };
        if !is_initial_about_blank {
            return None;
        }
        let destination_url = Url::parse(final_url).ok()?;
        // Opaque origins are never same-origin: an opaque initial about:blank (no creator)
        // or an about:blank destination must not reuse the Window.
        if matches!(destination_url.origin(), url::Origin::Opaque(_)) {
            return None;
        }
        if active_origin != destination_url.origin().unicode_serialization() {
            return None;
        }
        self.documents
            .remove(&active_document_id)
            .map(|document| document.settings)
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
        // Step 1: "Let document be the result of creating and initializing a `Document` object
        // given `html`, `text/html`, and navigationParams."
        // Note: The content-side steps of `initialise-the-document-object` run in
        // `Self::initialise_the_document_object`; the user-agent-side steps (browsing context
        // and agent selection) ran in `UserAgent::initialise_the_document_object` before this
        // command was dispatched.
        let (document, settings, needs_paint) =
            self.initialise_the_document_object(traversable_id, document_id, &final_url)?;

        let parser_scripts = {
            let mut document_guard = document.borrow_mut();

            // Step 2: "If document's URL is about:blank, then populate with html/head/body
            // given document."
            // Note: Not implemented as a special case: an about:blank body is empty, and the
            // HTML parser produces the same html/head/body skeleton from it below.
            // Step 3: "Otherwise, create an HTML parser whose allow declarative shadow roots
            // is true and associate it with document."
            // Note: The embedder has buffered the response body; feed into parser immediately.
            parse_html_into_document(&mut document_guard, &body)
        };

        // Step 4: "Return document."
        // Note: navigate-html returns the document to "attempt to populate the history
        // entry's document", whose content-side continuation runs below: the document is
        // registered as the traversable's active document, parser-discovered scripts run, and
        // `continue_document_load` fires the load event and reports the commit
        // (ContentFinalizeNavigation) once resources and deferred scripts are ready.

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
                needs_paint,
                last_scene: None,
                last_composition: None,
            },
        );
        // Make the document addressable immediately so the shared
        // new-document registry (window.open) works during the load's
        // script execution.
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
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

        self.report_document_title(traversable_id, top_level_traversable_id, document_id);
        self.continue_document_load(document_id)
    }

    /// Report the parsed title of a top-level document to the user agent so
    /// the embedder can label the corresponding tab and window.
    /// <https://html.spec.whatwg.org/#the-title-element>
    fn report_document_title(
        &self,
        traversable_id: NavigableId,
        top_level_traversable_id: NavigableId,
        document_id: DocumentId,
    ) {
        // The title element: "User agents should use the document's title
        // when referring to the document in their user interface."
        // Only the top-level document's title labels the tab; iframe
        // documents carry their own titles.
        if traversable_id != top_level_traversable_id {
            return;
        }
        let Some(content_document) = self.documents.get(&document_id) else {
            return;
        };
        let title = content_document
            .document
            .borrow()
            .find_title_node()
            .map(|node| node.text_content())
            .unwrap_or_default();
        let title = strip_and_collapse_ascii_whitespace(&title);
        if title.is_empty() {
            return;
        }
        if let Err(error) = self
            .event_sender
            .send(ContentEvent::TitleChanged(TitleChanged {
                traversable_id: top_level_traversable_id,
                title,
            }))
        {
            error!("failed to report document title: {error}");
        }
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

        self.mark_traversable_dirty(traversable_id);
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

        self.mark_traversable_dirty(traversable_id);
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

        dispatch_trusted_click_event(&mut document.settings, target_node_id)
    }

    fn destroy_document(&mut self, document_id: DocumentId) -> Result<(), String> {
        run_dom_removing_steps_for_document(self, document_id)?;
        if let Some(mut content_document) = self.documents.remove(&document_id) {
            let traversable_id = content_document.traversable_id;
            // Release every event listener and event handler callback on the
            // document's EventTargets (window, document, cached node
            // wrappers): the callbacks are strong JS handles that would
            // otherwise root the realm's objects and keep the whole context
            // alive after the document is gone.
            if let Err(error) = Self::clear_document_event_listeners(&mut content_document) {
                error!("failed to clear event listeners during document teardown: {error}");
            }
            if self
                .active_documents_by_traversable
                .get(&content_document.traversable_id)
                .is_some_and(|current_document_id| *current_document_id == document_id)
            {
                self.active_documents_by_traversable
                    .remove(&content_document.traversable_id);
            }
            // WindowProxy lifecycle: navigation commit re-points the
            // cached WindowProxy backings for this navigable.  If a new
            // document for the traversable is already active in this process
            // (same-process navigation), re-point to its Window; otherwise
            // the navigable's document was created in another content process
            // and every WindowProxy for it becomes cross-content.
            // <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
            let replacement_window = self
                .active_documents_by_traversable
                .get(&traversable_id)
                .and_then(|current_document_id| self.documents.get(current_document_id))
                .and_then(|document| {
                    let global_object = document
                        .settings
                        .realm_execution_context
                        .realm_global_object();
                    document
                        .settings
                        .realm_execution_context
                        .with_object_any(&global_object)
                        .and_then(|data| data.downcast_ref::<Window>().cloned())
                        .map(|window| (window, global_object))
                });
            if let Err(error) = self.sever_window_proxy_backings(traversable_id, replacement_window)
            {
                error!("failed to sever window proxy backings: {error}");
            }
            if let Err(error) = content_document.settings.clear_all_window_timers() {
                error!("failed to clear window timers during document teardown: {error}");
            }
            drop(content_document);
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
        drop(local_state);

        // Reclaim the destroyed document's realm: release the behaviour
        // closures of callbacks whose creation realm died with it (their
        // captured JS handles would keep rooting the dead realm), drain the
        // shared microtask queue, then run a full V8 + cppgc collection so
        // the dead context is reclaimed instead of waiting for allocation
        // pressure that a fresh page may never generate.
        #[cfg(v8_backend)]
        self.realm_parent.prune_dead_realm_callbacks();
        if let Err(error) = self.realm_parent.perform_a_microtask_checkpoint() {
            log::debug!("microtask checkpoint during document teardown failed: {error:?}");
        }
        self.realm_parent.gc();
        Ok(())
    }

    /// Navigation-commit hook: every cached WindowProxy (in every realm of
    /// this process) that targets a navigable whose document was just
    /// destroyed is re-pointed.  `replacement_window` is the new document's
    /// Window when the navigation stayed in this process, and `None` when
    /// the navigable's document was created in another content process (the
    /// WindowProxy becomes cross-content).
    /// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    fn sever_window_proxy_backings(
        &mut self,
        traversable_id: NavigableId,
        replacement_window: Option<(Window, JsObject)>,
    ) -> Result<(), String> {
        let backing = match replacement_window {
            Some((window, js_object)) => {
                WindowProxyBacking::SameContentProcess { window, js_object }
            }
            None => WindowProxyBacking::CrossContentProcess,
        };
        let document_ids: Vec<DocumentId> = self.documents.keys().copied().collect();
        for document_id in document_ids {
            let Some(content_document) = self.documents.get_mut(&document_id) else {
                continue;
            };
            with_global_scope(
                &mut content_document.settings.realm_execution_context,
                |global_scope, ec| {
                    global_scope.set_window_proxy_backing(traversable_id, backing.clone(), ec);
                    Ok(())
                },
            )
            .map_err(|error| {
                format!("failed to sever window proxy backings: {}", error.display())
            })?;
        }
        Ok(())
    }

    /// Release every event listener and event handler callback held by the
    /// document's EventTargets (the Window, the Document, and every cached
    /// node wrapper). Called during document teardown: the callbacks are
    /// strong JS handles, and a listener whose bound function references its
    /// own platform wrapper would otherwise root the realm's objects and
    /// keep the whole context alive after the document is gone.
    fn clear_document_event_listeners(
        content_document: &mut ContentDocument,
    ) -> Result<(), String> {
        with_global_scope(content_document.settings.ec(), |global_scope, ec| {
            let window_value = crate::js::Types::value_from_object(ec.realm_global_object());
            let mut cleared = 0usize;
            let _ = try_with_event_target_mut(&window_value, ec, |target, ec| {
                cleared += target.clear_all_listeners_and_handlers(ec);
            });
            if let Some(document_object) = global_scope.document_object(ec) {
                let value = crate::js::Types::value_from_object(document_object);
                let _ = try_with_event_target_mut(&value, ec, |target, ec| {
                    cleared += target.clear_all_listeners_and_handlers(ec);
                });
            }
            for object in global_scope.cached_node_objects(ec) {
                let value = crate::js::Types::value_from_object(object);
                let _ = try_with_event_target_mut(&value, ec, |target, ec| {
                    cleared += target.clear_all_listeners_and_handlers(ec);
                });
            }
            let _ = cleared;
            Ok(())
        })
        .map_err(|error| format!("failed to clear document event listeners: {error:?}"))
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

            // Dispatch may mutate the document; mark it so the next
            // update-the-rendering re-runs blitz.
            self.mark_document_dirty(document_id);
            let Some(document) = self.documents.get_mut(&document_id) else {
                continue;
            };

            // Continues <https://dom.spec.whatwg.org/#concept-event-fire> after the
            // user agent writes the serialized UI event batch to the content process.
            let event = deserialize_ui_event(&event)?;
            dispatch_ui_event(
                Rc::clone(&document.document),
                &mut document.settings,
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

    /// The target-process half of the window post message steps (the
    /// substeps of step 8).  The user agent routed `request` to this event
    /// loop after the source content process ran steps 1–7.
    /// <https://html.spec.whatwg.org/#window-post-message-steps>
    fn dispatch_post_message(&mut self, request: PostMessageRequest) -> Result<(), String> {
        // Step 8.1: If the targetOrigin argument is not a single literal U+002A
        //           ASTERISK character (*) and targetWindow's associated
        //           Document's origin is not same origin with targetOrigin,
        //           then return.
        let target_document_id = *self
            .active_documents_by_traversable
            .get(&request.target_navigable_id)
            .ok_or_else(|| {
                format!(
                    "postMessage: unknown target traversable {}",
                    request.target_navigable_id
                )
            })?;
        let target_origin = self
            .documents
            .get(&target_document_id)
            .map(|document| document.settings.origin.serialized.clone())
            .ok_or_else(|| format!("postMessage: unknown target document {target_document_id}"))?;
        if request.target_origin != "*" && request.target_origin != target_origin {
            return Ok(());
        }

        // The message handler may mutate the target document.
        self.mark_traversable_dirty(request.target_navigable_id);
        // Request a rendering opportunity from the UA so the target's render
        // cycle is driven and it receives UpdateTheRendering: the handler runs
        // here (in the target's content process) and mutates the document, but
        // without this note the UA never knows to re-render the navigable, so
        // a cross-origin iframe's change waits for an unrelated input event to
        // come through before it appears.
        if let Err(error) = self.event_sender.send(ContentEvent::RenderingOpRequested(
            request.target_navigable_id,
        )) {
            error!("failed to request rendering op for postMessage: {error}");
        }

        // Set up the shared registry so window.open calls made while the
        // message handlers run can register new documents.
        let traversable_id = self
            .documents
            .get(&target_document_id)
            .map(|document| document.traversable_id);
        if let Some(traversable_id) = traversable_id
            && let Err(error) = self.set_up_new_document_registry(traversable_id)
        {
            warn!("failed to set up new document registry: {error}");
        }

        // Step 8.2: Let origin be incumbentSettings's origin.
        let origin = request.source_origin.clone();

        // Step 8.3: Let source be the WindowProxy object corresponding to
        //           incumbentSettings's global object (a Window object).
        // The WindowProxy is created in the target realm so its methods
        // (postMessage) run in a realm that can reach the user agent; when
        // the source navigable's document lives in this content process, the
        // WindowProxy is backed by the source Window.
        let source_window = self
            .active_documents_by_traversable
            .get(&request.source_navigable_id)
            .and_then(|document_id| self.documents.get(document_id))
            .and_then(|document| {
                let global_object = document
                    .settings
                    .realm_execution_context
                    .realm_global_object();
                document
                    .settings
                    .realm_execution_context
                    .with_object_any(&global_object)
                    .and_then(|data| data.downcast_ref::<Window>().cloned())
                    .map(|window| (window, global_object))
            });
        let source = {
            let ec = &mut self
                .documents
                .get_mut(&target_document_id)
                .ok_or_else(|| {
                    format!("postMessage: unknown target document {target_document_id}")
                })?
                .settings
                .realm_execution_context;
            crate::html::windowproxy::window_proxy_object(
                request.source_navigable_id,
                source_window,
                ec,
            )
            .map_err(|error| {
                ec.to_rust_string(error).unwrap_or_else(|_| {
                    String::from("postMessage: failed to create source WindowProxy")
                })
            })?
        };

        let document = self
            .documents
            .get_mut(&target_document_id)
            .ok_or_else(|| format!("postMessage: unknown target document {target_document_id}"))?;

        // Step 8.4: Let deserializeRecord be
        //           StructuredDeserializeWithTransfer(serializeWithTransferResult,
        //           targetRealm).
        // Note: The deserialization runs in the target window's realm (the
        // target document's execution context), so `targetRealm` is the
        // current realm; the targetRealm argument is unused by the
        // deserializer, which constructs objects in the current realm.
        let serialize_result = SerializeWithTransferResult {
            serialized: request.serialized,
            transfer_data_holders: request.transfer_data_holders,
        };
        let deserialize_outcome = {
            let ec = &mut document.settings.realm_execution_context;
            structured_deserialize_with_transfer(&serialize_result, &ec.value_undefined(), ec)
        };
        let deserialize_result = match deserialize_outcome {
            Ok(result) => result,
            Err(_) => {
                // If this throws an exception, catch it, fire an event named
                // messageerror at targetWindow, using MessageEvent, with its
                // origin initialized to origin and the source attribute
                // initialized to source, and then return.
                let data = {
                    let ec = &mut document.settings.realm_execution_context;
                    ec.value_null()
                };
                let message_event = build_message_event(
                    &mut document.settings,
                    "messageerror",
                    origin,
                    Some(source),
                    data,
                    Vec::new(),
                )?;
                fire_event_at_window(&mut document.settings, &message_event.event)?;
                return Ok(());
            }
        };

        // Step 8.5: Let messageClone be deserializeRecord.[[Deserialized]].
        let message_clone = deserialize_result.deserialized;

        // Step 8.6: Let newPorts be a new frozen array consisting of all
        //           MessagePort objects in deserializeRecord.[[TransferredValues]],
        //           if any, maintaining their relative order.
        let new_ports: Vec<JsObject> = deserialize_result
            .transferred_values
            .iter()
            .filter_map(crate::js::Types::value_as_object)
            .collect();

        // Step 8.7: Fire an event named message at targetWindow, using
        //           MessageEvent, with its origin initialized to origin, the
        //           source attribute initialized to source, the data attribute
        //           initialized to messageClone, and the ports attribute
        //           initialized to newPorts.
        let message_event = build_message_event(
            &mut document.settings,
            "message",
            origin,
            Some(source),
            message_clone,
            new_ports,
        )?;
        fire_event_at_window(&mut document.settings, &message_event.event)?;

        if let Some(traversable_id) = traversable_id {
            if let Err(error) = self.tear_down_new_document_registry(traversable_id) {
                warn!("failed to tear down new document registry: {error}");
            }
            if let Err(error) = self.drain_new_traversable_documents() {
                warn!("failed to drain new traversable documents: {error}");
            }
        }

        Ok(())
    }

    /// Find the document whose realm manages a port record.
    fn find_port_document(&mut self, port_id: PortId) -> Option<DocumentId> {
        let mut found = None;
        for (document_id, document) in self.documents.iter_mut() {
            let result = with_global_scope(document.settings.ec(), |global_scope, ec| {
                Ok(global_scope
                    .channel_messaging(ec)
                    .map(|messaging| messaging.has_port(port_id, ec))
                    .unwrap_or(false))
            });
            if matches!(result, Ok(true)) {
                found = Some(*document_id);
                break;
            }
        }
        found
    }

    /// The channel messaging of the first realm in this process, used to
    /// return tasks to the user agent's routing queue when the port is no
    /// longer managed by this event loop.
    fn any_channel_messaging(&mut self) -> Option<crate::html::ChannelMessaging> {
        let (_, document) = self.documents.iter_mut().next()?;
        let messaging = with_global_scope(document.settings.ec(), |global_scope, ec| {
            Ok(global_scope.channel_messaging(ec))
        })
        .ok()
        .flatten()?;
        Some(messaging)
    }

    /// Run a task queued on this event loop by the user agent's routing
    /// (`MessagePortExtraFG.tla`'s `RunTask`).  The task is appended to the port's queue or
    /// returned to the routing queue when the port left this event loop.
    /// When the port is enabled, the message task (the substeps 7.1-7.7 of
    /// the message port post message steps) runs here, within the delivering
    /// task's slot: the message event fires without a further round-trip to
    /// request a task.
    fn handle_port_task(&mut self, port_id: PortId, task: PortTaskKind) -> Result<(), String> {
        let event_sender = self.event_sender.clone();
        let Some(document_id) = self.find_port_document(port_id) else {
            // The port is no longer managed by this event loop; return the
            // task to the user agent's routing queue.
            let messaging = self.any_channel_messaging().ok_or_else(|| {
                format!("port task: no realm to return the task for port {port_id}")
            })?;
            messaging
                .return_task_to_ua(port_id, task, &event_sender, &mut self.realm_parent)
                .map_err(|error| format!("port task return failed: {error}"))?;
            return Ok(());
        };
        let fire = {
            let content_document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("port task: unknown document {document_id}"))?;
            with_global_scope(content_document.settings.ec(), |global_scope, ec| {
                let Some(messaging) = global_scope.channel_messaging(ec) else {
                    return Ok(false);
                };
                messaging
                    .handle_port_task(port_id, task, &event_sender, ec)
                    .map_err(|error| ec.new_type_error(&format!("port task: {error}")))
            })
            .map_err(|error| format!("port task failed: {}", error.display()))?
        };
        if fire {
            // The delivery task runs the message task itself (the message
            // event fires within this task's slot).
            self.handle_run_port_message_task(port_id)?;
        }
        Ok(())
    }

    /// Fire one queued message event on a port (the message task of the
    /// message port post message steps).
    fn handle_run_port_message_task(&mut self, port_id: PortId) -> Result<(), String> {
        let Some(document_id) = self.find_port_document(port_id) else {
            return Ok(());
        };
        let traversable_id = self
            .documents
            .get(&document_id)
            .map(|document| document.traversable_id);
        let time_millis = self
            .documents
            .get(&document_id)
            .map(|document| document.settings.current_time_millis())
            .unwrap_or(0.0);
        let content_document = self
            .documents
            .get_mut(&document_id)
            .ok_or_else(|| format!("port message task: unknown document {document_id}"))?;
        with_global_scope(content_document.settings.ec(), |global_scope, ec| {
            let Some(messaging) = global_scope.channel_messaging(ec) else {
                return Ok(());
            };
            let Some(port) = messaging.port_object(port_id, ec) else {
                return Ok(());
            };
            port.run_message_task(time_millis, ec)
        })
        .map_err(|error| format!("port message task failed: {}", error.display()))?;

        // The message task's handler may have mutated the target document; mark
        // it dirty and request a rendering opportunity from the UA so the
        // target's render cycle is driven, instead of the change waiting for an
        // unrelated input event to come through.
        self.mark_document_dirty(document_id);
        if let Some(traversable_id) = traversable_id
            && let Err(error) = self
                .event_sender
                .send(ContentEvent::RenderingOpRequested(traversable_id))
        {
            error!("failed to request rendering op for port message task: {error}");
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
            let canceled = !crate::html::dispatch::steps_to_fire_beforeunload(
                &mut document.settings.realm_execution_context,
                "beforeunload",
                true,
                time_millis,
            )
            .map_err(|error| format!("steps_to_fire_beforeunload failed: {error:?}"))?;
            (Some(navigable_id), canceled)
        } else {
            (None, false)
        };
        if let Some(navigable_id) = navigable_id {
            let outcome = if canceled { "Aborted" } else { "Approved" };
            info!(
                "[nav] beforeunload result document={} check={} navigable={} outcome={}",
                document_id, check_id, navigable_id, outcome
            );
            verification::tla_log!(
                self.tla_tracer,
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

    /// <https://html.spec.whatwg.org/#update-the-rendering>
    fn update_the_rendering(
        &mut self,
        navigable_id: NavigableId,
        document_id: DocumentId,
        frame_timestamp_epoch_ms: f64,
    ) -> Result<(), String> {
        // This is where the update-the-rendering steps run: the rendering
        // opportunity arrives from the user agent as an UpdateTheRendering
        // command carrying the event loop's last render opportunity time.
        log_render_state_debug(format!(
            "process update-the-rendering navigable={} document={}",
            navigable_id, document_id,
        ));
        let video_paint_registry = Rc::clone(&self.video_paint_registry);
        let (paint_frame, shmem_map) = {
            let document = self
                .documents
                .get_mut(&document_id)
                .ok_or_else(|| format!("unknown document id: {document_id}"))?;

            document.document.borrow_mut().handle_messages();

            // Step 1: "Let `frameTimestamp` be `eventLoop`'s last render opportunity time."
            // The user agent stamps the opportunity time on the browser-wide monotonic
            // clock when it notes the opportunity and passes it in the UpdateTheRendering
            // command; convert it to a duration from this document's time origin (HR Time
            // "relative high resolution time") for the animation frame callbacks below.
            let frame_timestamp_ms = frame_timestamp_epoch_ms
                - epoch_millis(
                    self.epoch_anchor,
                    self.epoch_anchor_wall_ms,
                    document.settings.time_origin,
                );

            // Step 14: "For each `doc` of `docs`, run the animation frame callbacks for `doc`, passing in the relative high resolution time given `frameTimestamp` and `doc`'s relevant global object as the timestamp."
            // Note: The content process collapses `docs` to the single active document for this content process.
            let had_pending_r_af = document.settings.has_pending_animation_frame_callbacks();
            document
                .settings
                .run_animation_frame_callbacks(frame_timestamp_ms)?;

            let animation_time = frame_timestamp_ms / 1000.0;

            // Decide whether to run blitz. A static document that is asked to
            // render only because an external animation (a video frame arriving
            // at the graphics process) keeps the render cycle alive skips the
            // blitz resolve + paint and reuses its last scene, so the graphics
            // process keeps the content layer clean instead of re-rasterizing an
            // unchanged scene every cycle.
            let should_render = {
                let needs_paint = document.needs_paint.load(Ordering::Relaxed);
                let document_guard = document.document.borrow();
                needs_paint
                    || had_pending_r_af
                    || document_guard.is_animating()
                    // A fresh document has never painted; fail open so the first
                    // frame always runs blitz and populates the cache.
                    || document.last_scene.is_none()
            };

            if !should_render {
                // Clean render: reuse the last recorded scene and composition.
                // No blitz work, no font re-send, and the graphics process
                // keeps this frame's layer clean (the scene is byte-identical).
                // The render cycle still completes, so a new video frame is
                // composed against the unchanged content layer.
                info!(
                    "[render-pipe] Content clean render navigable={} document={} (skipping blitz)",
                    navigable_id, document_id
                );
                let recorded_scene = document.last_scene.clone().ok_or_else(|| {
                    format!(
                        "content render: no cached scene for clean cycle document={document_id}"
                    )
                })?;
                let composition = document.last_composition.clone().ok_or_else(|| {
                    format!("content render: no cached composition for clean cycle document={document_id}")
                })?;
                let document_guard = document.document.borrow();
                let viewport = document_guard.viewport().clone();
                let (width, height) = viewport.window_size;
                let has_video = video_paint_registry
                    .borrow()
                    .keys()
                    .any(|(doc_id, node_id)| {
                        let ended = self.ended_video_nodes.contains(&(*doc_id, *node_id));
                        *doc_id == document_id && !ended
                    });
                let animating = has_video || document_guard.is_animating();
                // Re-send the recorded scene without font data: the fonts are
                // already registered on the graphics side, and re-sending them
                // would reuse stale shared-memory keys.
                let scene = PreparedScene {
                    scene: recorded_scene,
                    registered_fonts: Vec::new(),
                    font_shmem: HashMap::new(),
                };
                let mut next_shmem_key = 0usize;
                let (paint_frame, shmem_data) = PaintFrame::new(
                    WebviewId(navigable_id),
                    document.frame_id,
                    width,
                    height,
                    composition,
                    scene,
                    &mut next_shmem_key,
                    animating,
                )?;
                (paint_frame, shmem_data)
            } else {
                {
                    let mut document_guard = document.document.borrow_mut();

                    // Step 16.2.1: "Recalculate styles and update layout for `doc`."
                    // `resolve` advances style, layout, and resource-driven document updates.
                    document_guard.resolve(animation_time);
                }

                info!(
                    "[render-pipe] Content render navigable={} document={} iframes={} video_registry_entries={}",
                    navigable_id,
                    document_id,
                    document.navigable_container_states.len(),
                    video_paint_registry.borrow().len()
                );
                let (paint_frame, shmem_data, recorded_scene, composition) = {
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
                    let prepared = self.font_sender.prepare_scene(
                        self.font_namespace,
                        scene,
                        &mut next_shmem_key,
                    );
                    log_render_state_debug(format!(
                        "emit paint navigable={} document={} size=({}, {})",
                        navigable_id, document_id, width, height,
                    ));
                    // Set animating=true when this document has animated content:
                    // video elements still producing frames, or blitz is animating
                    // (CSS animations/transitions, same-origin subdocuments,
                    // scroll animations). Graphics forwards this to the UA which
                    // keeps re-noting rendering opportunities to drive animation.
                    let has_video =
                        video_paint_registry
                            .borrow()
                            .keys()
                            .any(|(doc_id, node_id)| {
                                let ended = self.ended_video_nodes.contains(&(*doc_id, *node_id));
                                *doc_id == document_id && !ended
                            });
                    let animating = has_video || document_guard.is_animating();
                    info!(
                        "[render-pipe] Content paint_frame ready navigable={} frame={} size=({},{}) has_video={} animating={} embed_sites={}",
                        navigable_id,
                        document.frame_id.0,
                        width,
                        height,
                        has_video,
                        animating,
                        composition.embed_sites.len()
                    );
                    let recorded_scene = prepared.scene.clone();
                    let (paint_frame, shmem_data) = PaintFrame::new(
                        WebviewId(navigable_id),
                        document.frame_id,
                        width,
                        height,
                        composition.clone(),
                        prepared,
                        &mut next_shmem_key,
                        animating,
                    )?;
                    (paint_frame, shmem_data, recorded_scene, composition)
                };
                document.last_scene = Some(recorded_scene);
                document.last_composition = Some(composition);
                document.needs_paint.store(false, Ordering::Relaxed);
                (paint_frame, shmem_data)
            }
        };

        verification::tla_log!(
            self.tla_tracer,
            -> "RenderingOpportunity",
            "UpdateTheRendering",
            navigable_id
        );

        // Send the PaintFrame directly to the graphics process for composition.
        if let Some(graphics_sender) = &self.graphics_sender {
            let command = ipc_messages::graphics::GraphicsCommand::PaintFrame {
                frame: paint_frame.clone(),
            };
            if let Err(error) = graphics_sender.send_with_shmem_map(command, shmem_map.clone()) {
                error!("failed to send paint frame to graphics process: {error}");
            }
        }

        Ok(())
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

        let viewport_scroll = document.viewport_scroll();
        for (paint_order, (iframe_node_id, state)) in iframe_node_ids.into_iter().enumerate() {
            let (x, y, width, height) =
                match Self::content_box_for_node(document, iframe_node_id, scale) {
                    Some(box_) => box_,
                    None => continue,
                };
            debug!(
                "[layout] iframe node {} embed site: pos=({:.0},{:.0}) size=({:.0},{:.0}) viewport_scroll=({:.0},{:.0})",
                iframe_node_id, x, y, width, height, viewport_scroll.x, viewport_scroll.y
            );
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
                "[layout] video node {} embed site: pos=({:.0},{:.0}) size=({:.0},{:.0}) viewport_scroll=({:.0},{:.0})",
                video_node_id, x, y, width, height, viewport_scroll.x, viewport_scroll.y
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
                self.event_sender
                    .send(ContentEvent::RenderingOpRequested(traversable_id))
                    .map_err(|error| {
                        format!("failed to request rendering op for resource fetch: {error}")
                    })?;
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
                self.event_sender
                    .send(ContentEvent::RenderingOpRequested(traversable_id))
                    .map_err(|error| {
                        format!("failed to request rendering op for deferred script fetch: {error}")
                    })?;
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
                self.event_sender
                    .send(ContentEvent::RenderingOpRequested(traversable_id))
                    .map_err(|error| {
                        format!(
                            "failed to request rendering op for resource fetch failure: {error}"
                        )
                    })?;
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
                self.event_sender
                    .send(ContentEvent::RenderingOpRequested(traversable_id))
                    .map_err(|error| format!("failed to request rendering op for deferred script fetch failure: {error}"))?;
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
        with_global_scope(content_document.settings.ec(), |global_scope, _ec| {
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

    fn event_loop_task_sources(&self) -> EventLoopTaskSources {
        EventLoopTaskSources::new(self.task_sender.clone(), Rc::clone(&self.active_timers))
    }

    fn earliest_timer_expiry_wait(&self) -> Option<Duration> {
        self.active_timers.borrow().earliest_expiry_wait()
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    fn run_steps_after_a_timeout(&mut self) {
        // Step 1: "Let timerKey be a new unique internal value."
        // Step 2: "Let startTime be the current high resolution time given global."
        // Step 3: "Set global's map of active timers[timerKey] to startTime plus milliseconds."
        // Note: Steps 1-3 and 5 ran in `MapOfActiveTimers::run_steps_after_a_timeout`
        // when the timer was scheduled; only the in-parallel steps remain.

        // Step 4: "Run the following steps in parallel:"
        // Note: Runs on the content process main loop once the `select!` wait on
        // `earliest_timer_expiry_wait` fires.  One iteration per expired timer.
        //
        // Step 4.1: "If global is a Window object, wait until global's associated
        // Document has been fully active for a further milliseconds milliseconds
        // (not necessarily consecutively)."
        // Note: Realized by the `select!` wait on `earliest_timer_expiry_wait`.
        //
        // Step 4.2: "Wait until any invocations of this algorithm that had the
        // same global and orderingIdentifier, that started before this one, and
        // whose milliseconds is less than or equal to this one's, have completed."
        // Note: `take_expired_timers` returns the expired timers ordered by
        // expiry time, then start order, so completion steps queue in that order.
        //
        // Step 4.3: "Optionally, wait a further implementation-defined length of time."
        // Note: Not implemented: expiry times are exact.
        let expired = self.active_timers.borrow_mut().take_expired_timers();

        // Step 4.4: "Perform completionSteps."
        // Note: The completion step of the timer initialization steps queues a
        // global task on the timer task source to run the timer's task, so an
        // expired timer becomes a queued task rather than running here.
        let task_sources = self.event_loop_task_sources();
        for timer in expired {
            if let Err(error) = task_sources.queue_a_task(Command::RunWindowTimer {
                document_id: timer.document_id,
                timer_id: timer.timer_id,
                timer_key: timer.timer_key,
                nesting_level: timer.nesting_level,
            }) {
                error!("expired timer {}: {error}", timer.timer_id);
            }
        }

        // Step 4.5: "Remove global's map of active timers[timerKey]."
        // Note: `take_expired_timers` removed each expired entry as it collected
        // the timers.
        //
        // Step 5: "Return timerKey."
        // Note: Nothing to return: the id was already handed back when the timer
        // was scheduled.
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

    /// Mark a document as needing a render. Called whenever content runs a
    /// script or dispatches an event that may mutate the document, so
    /// `update_the_rendering` re-runs blitz. A render cycle driven solely by
    /// external animation (a video frame arriving at the graphics process)
    /// does not call this, so a static document that keeps being asked to
    /// render skips the blitz resolve + paint on an unchanged scene.
    fn mark_document_dirty(&self, document_id: DocumentId) {
        if let Some(document) = self.documents.get(&document_id) {
            document.needs_paint.store(true, Ordering::Relaxed);
        }
    }

    /// Mark the active document for a traversable as needing a render.
    fn mark_traversable_dirty(&self, traversable_id: NavigableId) {
        if let Some(document_id) = self.active_documents_by_traversable.get(&traversable_id) {
            self.mark_document_dirty(*document_id);
        }
    }

    fn handle_command_inner(&mut self, command: Command) -> Result<bool, String> {
        match command {
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
            Command::PostMessage(request) => {
                self.dispatch_post_message(request)?;
                Ok(true)
            }
            Command::PortTask { port, task } => {
                self.handle_port_task(port, task)?;
                Ok(true)
            }
            Command::RunPortMessageTask { port } => {
                self.handle_run_port_message_task(port)?;
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
                frame_timestamp_epoch_ms,
            } => {
                self.update_the_rendering(traversable_id, document_id, frame_timestamp_epoch_ms)?;
                Ok(true)
            }
            RunWindowTimer {
                document_id,
                timer_id,
                timer_key,
                nesting_level,
            } => {
                // The timer callback may mutate the document.
                self.mark_document_dirty(document_id);
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
            NotifyVideoEnded { video_paint_id } => {
                // Keep the paint ID in the registry so the last video frame is
                // still rendered as part of the composition. Just mark the node
                // as ended so the animating flag is set to false.
                let ended_key: Vec<(DocumentId, usize)> = self
                    .video_paint_registry
                    .borrow()
                    .iter()
                    .filter(|(_, existing_id)| **existing_id == video_paint_id)
                    .map(|((doc_id, node_id), _)| (*doc_id, *node_id))
                    .collect();
                for key in ended_key {
                    self.ended_video_nodes.insert(key);
                }
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

        let (
            event_loop_id,
            network_extension_sender,
            graphics_sender,
            content_command_sender,
            trace_sender,
        ) = {
            match cmd_rx.recv() {
                Ok(incoming) => match incoming.payload {
                    ContentBootstrap {
                        event_loop_id,
                        net_sender,
                        graphics_sender,
                        content_command_sender,
                        trace_sender,
                        ..
                    } => (
                        event_loop_id,
                        net_sender,
                        graphics_sender,
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

        let mut process = {
            ContentProcess::new(
                event_sender.clone(),
                wasm_signal_sender,
                event_loop_id,
                network_extension_sender,
                graphics_sender,
                content_command_sender,
                trace_sender,
            )
        };

        run_content_message_loop(&cmd_rx, &wasm_rx, &mut process)
    })
}

/// <https://html.spec.whatwg.org/#event-loop-processing-model>
fn run_content_message_loop(
    cmd_rx: &crossbeam_channel::Receiver<ipc::IpcIncoming<Command>>,
    wasm_rx: &crossbeam_channel::Receiver<()>,
    process: &mut ContentProcess,
) -> Result<(), String> {
    loop {
        // Step 1: "Let oldestTask and taskStartTime be null."
        // Step 2: "If the event loop has a task queue with at least one runnable task:"
        // Step 2.1: "Let taskQueue be one such task queue, chosen in an
        // implementation-defined manner."
        // Note: Tasks from every task source share one queue — the channel
        // `queue_a_task` appends to — so there is nothing to choose between.
        // Step 2.2: "Set taskStartTime to the unsafe shared current time."
        // Note: Not implemented: task start time is not recorded.
        // Step 2.3: "Set oldestTask to the first runnable task in taskQueue, and
        // remove it from taskQueue."
        // Note: Receiving from the task queue removes the task from it, and
        // runnability is not tracked, so the oldest task is the oldest queued
        // one.  A command from the user agent is a task only when it comes from
        // a task source (`command_is_event_loop_task`); the rest are control
        // messages driving the content process (viewport, document lifecycle,
        // fetch completion, shutdown), which are handled where they arrive
        // because no task source queued them.
        // Step 3: "Let taskEndTime be the unsafe shared current time."
        // Step 4: "If oldestTask is not null:"
        // Step 5: "If this is a window event loop that has no runnable task in
        // this event loop's task queues:"
        // Step 6: "If this is a worker event loop:"
        // Note: Steps 3-4 (long task reporting) and step 5 (idle periods) are
        // not implemented, and step 6 does not apply: this event loop is a
        // window event loop.  With no queued task the receive below is where the
        // loop waits for the next task-bearing input: a task on the task queue,
        // a command from the user agent, a WebAssembly result, or the earliest
        // expiry time in the map of active timers.
        let timer_expiry = match process.earliest_timer_expiry_wait() {
            Some(wait) => crossbeam_channel::after(wait),
            None => crossbeam_channel::never(),
        };
        let oldest_task = crossbeam_channel::select! {
            recv(&process.task_receiver) -> task => match task {
                Ok(task) => Some(task),
                Err(_) => return Ok(()),
            },
            recv(cmd_rx) -> cmd => {
                match cmd {
                    Ok(incoming) => {
                        if command_is_event_loop_task(&incoming.payload) {
                            Some(incoming.payload)
                        } else {
                            // These handlers run their own script-cleanup
                            // microtask checkpoints where they run script, so no
                            // task-path checkpoint (step 2.8) follows.
                            match process.handle_command(incoming.payload) {
                                Ok(true) => None,
                                Ok(false) => return Ok(()),
                                Err(error) => {
                                    error!("content error: {error}");
                                    None
                                }
                            }
                        }
                    }
                    Err(_) => return Ok(()),
                }
            },
            recv(wasm_rx) -> _ => {
                process.drain_all_pending_wasm_requests();
                process.drain_wasm_results();

                // <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
                // Wasm compilation results resolve promises, so run a microtask
                // checkpoint after they are processed.
                if let Err(error) = process.perform_microtask_checkpoint() {
                    error!("microtask checkpoint after wasm failed: {error}");
                }
                None
            },
            recv(timer_expiry) -> _ => {
                process.run_steps_after_a_timeout();
                None
            },
        };
        let Some(oldest_task) = oldest_task else {
            continue;
        };

        // Step 2.4: "If oldestTask's document is not null, then record task
        // start time given taskStartTime and oldestTask's document."
        // Step 2.5: "Set the event loop's currently running task to oldestTask."
        // Note: Not implemented: task timing is not recorded and the currently
        // running task is not tracked.
        // Step 2.6: "Perform oldestTask's steps."
        match process.handle_command(oldest_task) {
            Ok(true) => {
                // Step 2.7: "Set the event loop's currently running task back to null."
                // Note: Not implemented, as for step 2.5.
                // Step 2.8: "Perform a microtask checkpoint."
                if let Err(error) = process.perform_microtask_checkpoint() {
                    error!("microtask checkpoint after task failed: {error}");
                }
            }
            Ok(false) => return Ok(()),
            Err(error) => {
                error!("content error: {error}");
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
/// Build the MessageEvent for the `message`/`messageerror` event fired by
/// the window post message steps (steps 8.4 and 8.7), with the message
/// attributes and the trusted flag + timestamp of a user-agent-fired event.
/// The caller fires it via <https://dom.spec.whatwg.org/#concept-event-fire>.
fn build_message_event(
    settings: &mut EnvironmentSettingsObject,
    event_type: &str,
    origin: String,
    source: Option<JsObject>,
    data: JsValue,
    ports: Vec<JsObject>,
) -> Result<MessageEvent, String> {
    let time_millis = settings.current_time_millis();
    let ec = &mut settings.realm_execution_context;
    let message_event = crate::html::MessageEvent::new(
        event_type.to_owned(),
        crate::html::MessageEventInit {
            bubbles: false,
            cancelable: false,
            composed: false,
            data,
            origin,
            last_event_id: String::new(),
            source,
            ports,
        },
        ec,
    );
    let event_object =
        create_interface_instance::<crate::js::Types, MessageEvent>(message_event, ec)
            .map_err(|error| format!("failed to create MessageEvent: {error:?}"))?;
    let message_event: MessageEvent = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<MessageEvent>().cloned())
        .ok_or_else(|| String::from("event_object is not a MessageEvent"))?;
    // <https://dom.spec.whatwg.org/#concept-event-fire>
    // Events fired by the user agent are trusted and carry the current time.
    *message_event.event.is_trusted.borrow_mut(ec) = true;
    *message_event.event.time_stamp.borrow_mut(ec) = time_millis;
    Ok(message_event)
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
/// Fire a pre-built event at the realm's Window: build the event path and
/// dispatch (the fire-event algorithm with the event already created and
/// initialized).
fn fire_event_at_window(
    settings: &mut EnvironmentSettingsObject,
    event: &crate::dom::Event,
) -> Result<(), String> {
    let ec = &mut settings.realm_execution_context;
    let window_target = ec
        .with_object_any(&ec.realm_global_object())
        .and_then(|data| data.downcast_ref::<crate::html::Window>().cloned())
        .map(|window| window.get_event_target(ec))
        .ok_or_else(|| String::from("target window not found"))?;
    let path = simple_path(&window_target, ec);
    dispatch_with_path(ec, &path, event)
        .map(|_| ())
        .map_err(|error| format!("failed to dispatch event: {error:?}"))
}
