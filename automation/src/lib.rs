mod cdp;

use base64::Engine as _;
use clap::Args;
use ipc_messages::content::{NavigableId, WebviewId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex, mpsc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use cdp::{CdpArgs, CdpServerHandle};

pub(crate) const HTTP_BODY_LIMIT: usize = 2 * 1024 * 1024;
pub(crate) const AUTOMATION_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationSnapshot {
    pub webview_id: Option<WebviewId>,
    pub current_url: Option<String>,
    pub displayed_url: String,
    pub navigable_id: Option<NavigableId>,
    pub has_top_level_traversable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AutomationVisibleFrameViewport {
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CdpEvent {
    NavigationCommitted {
        url: String,
        frame_id: String,
        timestamp: f64,
    },
}

pub enum AutomationCommand {
    Snapshot {
        reply: mpsc::Sender<Result<AutomationSnapshot, String>>,
    },
    VisibleFrameViewports {
        reply: mpsc::Sender<Result<Vec<AutomationVisibleFrameViewport>, String>>,
    },
    Screenshot {
        reply: mpsc::Sender<Result<Vec<u8>, String>>,
    },
    Navigate {
        url: String,
        reply: mpsc::Sender<Result<AutomationSnapshot, String>>,
    },
    Click {
        x: f32,
        y: f32,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ClickElement {
        selector: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Scroll {
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        reply: mpsc::Sender<Result<(), String>>,
    },
    EvaluateScript {
        source: String,
        timeout: Duration,
        reply: mpsc::Sender<Result<Value, String>>,
    },
    SetCdpEventSink {
        sink: Option<mpsc::Sender<CdpEvent>>,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

pub trait AutomationHost {
    fn automation_snapshot(&mut self) -> AutomationSnapshot;
    fn automation_visible_frame_viewports(
        &mut self,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String>;
    fn automation_screenshot(&mut self) -> Result<Vec<u8>, String>;
    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String>;
    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String>;
    fn automation_click_element(&mut self, selector: String) -> Result<(), String>;
    fn automation_scroll(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String>;
    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String>;
}

#[derive(Default)]
pub struct AutomationController {
    pending_navigation: Option<PendingAutomationNavigation>,
    cdp_event_sink: Option<mpsc::Sender<CdpEvent>>,
}

struct PendingAutomationNavigation {
    reply: mpsc::Sender<Result<AutomationSnapshot, String>>,
}

impl AutomationController {
    pub fn handle_command<H: AutomationHost>(&mut self, host: &mut H, command: AutomationCommand) {
        match command {
            AutomationCommand::Snapshot { reply } => {
                let _ = reply.send(Ok(host.automation_snapshot()));
            }
            AutomationCommand::VisibleFrameViewports { reply } => {
                let _ = reply.send(host.automation_visible_frame_viewports());
            }
            AutomationCommand::Screenshot { reply } => {
                let _ = reply.send(host.automation_screenshot());
            }
            AutomationCommand::Navigate { url, reply } => {
                if self.pending_navigation.is_some() {
                    let _ = reply.send(Err(String::from(
                        "an automation navigation is already pending",
                    )));
                    return;
                }

                match host.begin_automation_navigation(url) {
                    Ok(()) => {
                        self.pending_navigation = Some(PendingAutomationNavigation { reply });
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            AutomationCommand::Click { x, y, reply } => {
                let _ = reply.send(host.automation_click(x, y));
            }
            AutomationCommand::ClickElement { selector, reply } => {
                let _ = reply.send(host.automation_click_element(selector));
            }
            AutomationCommand::Scroll {
                x,
                y,
                delta_x,
                delta_y,
                reply,
            } => {
                let _ = reply.send(host.automation_scroll(x, y, delta_x, delta_y));
            }
            AutomationCommand::EvaluateScript {
                source,
                timeout,
                reply,
            } => {
                let _ = reply.send(host.automation_evaluate_script(source, timeout));
            }
            AutomationCommand::SetCdpEventSink { sink, reply } => {
                self.cdp_event_sink = sink;
                let _ = reply.send(Ok(()));
            }
        }
    }

    pub fn note_navigation_committed<H: AutomationHost>(&mut self, host: &mut H) {
        let Some(snapshot) = self.automation_snapshot_if_ready(host) else {
            return;
        };
        self.emit_navigation_committed(&snapshot);
        self.complete_pending_navigation(snapshot);
    }

    pub fn note_rendering_update<H: AutomationHost>(&mut self, host: &mut H) {
        let Some(snapshot) = self.automation_snapshot_if_ready(host) else {
            return;
        };
        self.complete_pending_navigation(snapshot);
    }

    pub fn abort_pending_navigation(&mut self, message: String) {
        if let Some(pending_navigation) = self.pending_navigation.take() {
            let _ = pending_navigation.reply.send(Err(message));
        }
    }

    fn automation_snapshot_if_ready<H: AutomationHost>(
        &mut self,
        host: &mut H,
    ) -> Option<AutomationSnapshot> {
        let snapshot = host.automation_snapshot();
        if snapshot.navigable_id.is_none() {
            return None;
        }
        Some(snapshot)
    }

    fn emit_navigation_committed(&self, snapshot: &AutomationSnapshot) {
        let Some(sink) = self.cdp_event_sink.as_ref() else {
            return;
        };
        let Some(url) = snapshot.current_url.clone() else {
            return;
        };
        let Some(frame_id) = snapshot.navigable_id.map(|id| id.to_string()) else {
            return;
        };

        let _ = sink.send(CdpEvent::NavigationCommitted {
            url,
            frame_id,
            timestamp: cdp_timestamp_now(),
        });
    }

    fn complete_pending_navigation(&mut self, snapshot: AutomationSnapshot) {
        if let Some(pending_navigation) = self.pending_navigation.take() {
            let _ = pending_navigation.reply.send(Ok(snapshot));
        }
    }
}

fn cdp_timestamp_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Clone)]
pub struct AutomationRuntime {
    send_command: Arc<dyn Fn(AutomationCommand) -> Result<(), String> + Send + Sync>,
    request_exit: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    is_ready: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl AutomationRuntime {
    pub fn new<SendCommand, RequestExit, IsReady>(
        send_command: SendCommand,
        request_exit: RequestExit,
        is_ready: IsReady,
    ) -> Self
    where
        SendCommand: Fn(AutomationCommand) -> Result<(), String> + Send + Sync + 'static,
        RequestExit: Fn() -> Result<(), String> + Send + Sync + 'static,
        IsReady: Fn() -> bool + Send + Sync + 'static,
    {
        Self {
            send_command: Arc::new(send_command),
            request_exit: Arc::new(request_exit),
            is_ready: Arc::new(is_ready),
        }
    }

    pub fn is_ready(&self) -> bool {
        (self.is_ready)()
    }

    pub fn snapshot(&self, timeout: Duration) -> Result<AutomationSnapshot, String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::Snapshot { reply })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for automation snapshot: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn navigate(&self, url: &str, timeout: Duration) -> Result<AutomationSnapshot, String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::Navigate {
            url: url.to_owned(),
            reply,
        })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for navigation to complete: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn screenshot(&self, timeout: Duration) -> Result<Vec<u8>, String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::Screenshot { reply })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for automation screenshot: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn visible_frame_viewports(
        &self,
        timeout: Duration,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::VisibleFrameViewports { reply })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for visible frame viewports: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn click(&self, x: f32, y: f32, timeout: Duration) -> Result<(), String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::Click { x, y, reply })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for automation click delivery: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn click_element(&self, selector: &str, timeout: Duration) -> Result<(), String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::ClickElement {
            selector: selector.to_owned(),
            reply,
        })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for selector click delivery: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn scroll(
        &self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        timeout: Duration,
    ) -> Result<(), String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::Scroll {
            x,
            y,
            delta_x,
            delta_y,
            reply,
        })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for automation scroll delivery: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn evaluate_script(&self, source: String, timeout: Duration) -> Result<Value, String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::EvaluateScript {
            source,
            timeout,
            reply,
        })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for script evaluation: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn set_cdp_event_sink(
        &self,
        sink: Option<mpsc::Sender<CdpEvent>>,
        timeout: Duration,
    ) -> Result<(), String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::SetCdpEventSink { sink, reply })?;
        receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting to update the CDP event sink: {error}",
                timeout.as_millis()
            )
        })?
    }

    pub fn request_exit(&self) -> Result<(), String> {
        (self.request_exit)()
    }
}

pub fn automation_bridge<SendCommand, RequestExit, IsReady>(
    send_command: SendCommand,
    request_exit: RequestExit,
    is_ready: IsReady,
) -> AutomationRuntime
where
    SendCommand: Fn(AutomationCommand) -> Result<(), String> + Send + Sync + 'static,
    RequestExit: Fn() -> Result<(), String> + Send + Sync + 'static,
    IsReady: Fn() -> bool + Send + Sync + 'static,
{
    AutomationRuntime::new(send_command, request_exit, is_ready)
}

#[derive(Args, Debug)]
pub struct WebDriverArgs {
    #[arg(long, default_value_t = 4444)]
    pub port: u16,

    #[arg(long)]
    pub cdp_port: Option<u16>,

    #[arg(long)]
    pub headless: bool,

    #[arg(long, value_name = "URL")]
    pub startup_url: Option<String>,

    #[arg(long, default_value_t = true)]
    pub exit_on_session_delete: bool,
}

#[derive(Debug)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) body: Vec<u8>,
}

pub struct WebDriverServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct WebDriverState {
    session_id: Mutex<Option<String>>,
    exit_on_session_delete: bool,
    runtime: AutomationRuntime,
}

#[derive(Deserialize)]
struct NavigateRequest {
    url: String,
}

#[derive(Deserialize)]
struct ExecuteScriptRequest {
    script: String,
    #[serde(default)]
    args: Vec<Value>,
}

#[derive(Deserialize)]
struct ClickRequest {
    x: f32,
    y: f32,
}

#[derive(Deserialize)]
struct ClickElementRequest {
    selector: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScrollRequest {
    x: f32,
    y: f32,
    #[serde(default)]
    delta_x: f32,
    delta_y: f32,
}

#[derive(Serialize)]
struct SuccessResponse<T> {
    value: T,
}

#[derive(Serialize)]
struct ErrorResponse {
    value: ErrorValue,
}

#[derive(Serialize)]
struct ErrorValue {
    error: &'static str,
    message: String,
    stacktrace: String,
}

impl WebDriverServer {
    pub fn start(
        port: u16,
        exit_on_session_delete: bool,
        runtime: AutomationRuntime,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("failed to bind WebDriver server on port {port}: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure WebDriver server listener: {error}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let state = Arc::new(WebDriverState {
            session_id: Mutex::new(None),
            exit_on_session_delete,
            runtime,
        });
        let thread_state = Arc::clone(&state);
        let thread = thread::Builder::new()
            .name(String::from("formal-web-webdriver"))
            .spawn(move || run_server(listener, thread_state, thread_stop))
            .map_err(|error| format!("failed to spawn WebDriver server thread: {error}"))?;

        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for WebDriverServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_server(listener: TcpListener, state: Arc<WebDriverState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _address)) => {
                if let Err(error) = handle_connection(stream, &state) {
                    eprintln!("formal-web webdriver server error: {error}");
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                eprintln!("formal-web webdriver accept error: {error}");
                break;
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream, state: &WebDriverState) -> Result<(), String> {
    let Some(request) = read_http_request(&mut stream)? else {
        return Ok(());
    };
    let (status, status_text, body) = dispatch_request(&request, state);
    write_http_response(&mut stream, &request.method, status, status_text, &body)
}

fn dispatch_request(request: &HttpRequest, state: &WebDriverState) -> (u16, &'static str, Vec<u8>) {
    match dispatch_request_inner(request, state) {
        Ok(value) => (200, "OK", json_success(value)),
        Err(WebDriverError::Http {
            status,
            status_text,
            error,
            message,
        }) => (status, status_text, json_error(error, &message)),
    }
}

fn dispatch_request_inner(
    request: &HttpRequest,
    state: &WebDriverState,
) -> Result<Value, WebDriverError> {
    let path = request
        .target
        .split('?')
        .next()
        .unwrap_or(request.target.as_str());
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match (request.method.as_str(), segments.as_slice()) {
        ("GET", ["status"]) => Ok(status_payload(&state.runtime)),
        ("POST", ["session"]) => create_session(state),
        (method, ["session", session_id, rest @ ..]) => {
            ensure_session(state, session_id)?;
            dispatch_session_request(method, rest, &request.body, state)
        }
        _ => Err(WebDriverError::http(
            404,
            "Not Found",
            "unknown command",
            format!(
                "unsupported WebDriver route {} {}",
                request.method, request.target
            ),
        )),
    }
}

fn dispatch_session_request(
    method: &str,
    rest: &[&str],
    body: &[u8],
    state: &WebDriverState,
) -> Result<Value, WebDriverError> {
    match (method, rest) {
        ("DELETE", []) => {
            clear_session(state)?;
            if state.exit_on_session_delete {
                state.runtime.request_exit().map_err(WebDriverError::unsupported)?;
            }
            Ok(Value::Null)
        }
        ("GET", ["formal-web", "frame-viewports"]) => Ok(json!(state
            .runtime
            .visible_frame_viewports(AUTOMATION_TIMEOUT)
            .map_err(WebDriverError::timeout)?)),
        ("GET", ["screenshot"]) => {
            let png = state
                .runtime
                .screenshot(AUTOMATION_TIMEOUT)
                .map_err(WebDriverError::timeout)?;
            Ok(json!(base64::engine::general_purpose::STANDARD.encode(png)))
        }
        ("GET", ["url"]) => {
            let snapshot = current_snapshot(state)?;
            Ok(json!(snapshot.current_url.unwrap_or_default()))
        }
        ("GET", ["title"]) => execute_script_value(state, "return document.title;", &[]),
        ("POST", ["url"]) => {
            let request: NavigateRequest =
                serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            state
                .runtime
                .navigate(&request.url, AUTOMATION_TIMEOUT)
                .map_err(WebDriverError::timeout)?;
            Ok(Value::Null)
        }
        ("POST", ["formal-web", "click"]) => {
            let request: ClickRequest =
                serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            state
                .runtime
                .click(request.x, request.y, AUTOMATION_TIMEOUT)
                .map_err(WebDriverError::timeout)?;
            Ok(Value::Null)
        }
        ("POST", ["formal-web", "element", "click"]) => {
            let request: ClickElementRequest =
                serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            state
                .runtime
                .click_element(&request.selector, AUTOMATION_TIMEOUT)
                .map_err(element_click_error)?;
            Ok(Value::Null)
        }
        ("POST", ["formal-web", "scroll"]) => {
            let request: ScrollRequest =
                serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            state
                .runtime
                .scroll(
                    request.x,
                    request.y,
                    request.delta_x,
                    request.delta_y,
                    AUTOMATION_TIMEOUT,
                )
                .map_err(WebDriverError::timeout)?;
            Ok(Value::Null)
        }
        ("POST", ["execute", "sync"]) => {
            let request: ExecuteScriptRequest =
                serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            execute_script_value(state, &request.script, &request.args)
        }
        _ => Err(WebDriverError::http(
            404,
            "Not Found",
            "unknown command",
            format!(
                "unsupported WebDriver session route {method} /{}",
                rest.join("/")
            ),
        )),
    }
}

fn status_payload(runtime: &AutomationRuntime) -> Value {
    let ready = runtime.is_ready()
        && runtime
            .snapshot(Duration::from_millis(250))
            .map(|snapshot| snapshot.has_top_level_traversable && snapshot.webview_id.is_some())
            .unwrap_or(false);
    json!({
        "ready": ready,
        "message": "formal-web webdriver"
    })
}

fn create_session(state: &WebDriverState) -> Result<Value, WebDriverError> {
    if !state.runtime.is_ready() {
        return Err(WebDriverError::http(
            503,
            "Service Unavailable",
            "session not created",
            String::from("embedder event loop is not ready yet"),
        ));
    }

    let mut guard = state
        .session_id
        .lock()
        .expect("webdriver session mutex poisoned");
    if guard.is_some() {
        return Err(WebDriverError::http(
            500,
            "Internal Server Error",
            "session not created",
            String::from("a WebDriver session is already active"),
        ));
    }

    let session_id = format!(
        "formal-web-{}",
        NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
    );
    *guard = Some(session_id.clone());

    Ok(json!({
        "sessionId": session_id,
        "capabilities": {
            "browserName": "formal-web",
            "platformName": std::env::consts::OS,
            "acceptInsecureCerts": true
        }
    }))
}

fn ensure_session(state: &WebDriverState, session_id: &str) -> Result<(), WebDriverError> {
    let guard = state
        .session_id
        .lock()
        .expect("webdriver session mutex poisoned");
    match guard.as_deref() {
        Some(active) if active == session_id => Ok(()),
        _ => Err(WebDriverError::http(
            404,
            "Not Found",
            "invalid session id",
            format!("unknown WebDriver session `{session_id}`"),
        )),
    }
}

fn clear_session(state: &WebDriverState) -> Result<(), WebDriverError> {
    let mut guard = state
        .session_id
        .lock()
        .expect("webdriver session mutex poisoned");
    if guard.take().is_none() {
        return Err(WebDriverError::http(
            404,
            "Not Found",
            "invalid session id",
            String::from("no WebDriver session is active"),
        ));
    }
    Ok(())
}

fn current_snapshot(state: &WebDriverState) -> Result<AutomationSnapshot, WebDriverError> {
    state
        .runtime
        .snapshot(Duration::from_secs(1))
        .map_err(WebDriverError::unsupported)
}

fn execute_script_value(
    state: &WebDriverState,
    script: &str,
    args: &[Value],
) -> Result<Value, WebDriverError> {
    let wrapped = wrap_execute_script(script, args).map_err(WebDriverError::invalid_argument)?;
    state
        .runtime
        .evaluate_script(wrapped, SCRIPT_TIMEOUT)
        .map_err(WebDriverError::javascript)
}

fn element_click_error(error: String) -> WebDriverError {
    if error.starts_with("invalid selector ") {
        return WebDriverError::http(400, "Bad Request", "invalid selector", error);
    }
    if error.starts_with("no element matched selector ") {
        return WebDriverError::http(404, "Not Found", "no such element", error);
    }
    WebDriverError::unsupported(error)
}

fn wrap_execute_script(script: &str, args: &[Value]) -> Result<String, serde_json::Error> {
    let args_json = serde_json::to_string(args)?;
    Ok(format!(
        "(() => {{\nconst __formalWebArgs = {args_json};\nconst __formalWebKey = \"__formalWebExecuteScript\";\nwindow[__formalWebKey] = function() {{\n{script}\n}};\ntry {{\nreturn Function.prototype.apply.call(window[__formalWebKey], window, __formalWebArgs);\n}} finally {{\ndelete window[__formalWebKey];\n}}\n}})()"
    ))
}

fn json_success(value: Value) -> Vec<u8> {
    serde_json::to_vec(&SuccessResponse { value })
        .expect("webdriver success response serialization failed")
}

fn json_error(error: &'static str, message: &str) -> Vec<u8> {
    serde_json::to_vec(&ErrorResponse {
        value: ErrorValue {
            error,
            message: message.to_owned(),
            stacktrace: String::new(),
        },
    })
    .expect("webdriver error response serialization failed")
}

pub(crate) fn find_header_terminator(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

pub(crate) fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| format!("failed to set WebDriver read timeout: {error}"))?;

    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let bytes_read = match stream.read(&mut chunk) {
            Ok(bytes_read) => bytes_read,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => return Err(format!("failed to read WebDriver request: {error}")),
        };
        if bytes_read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if buffer.len() > HTTP_BODY_LIMIT {
            return Err(String::from(
                "incoming WebDriver request exceeded the size limit",
            ));
        }
        if let Some(header_end) = find_header_terminator(&buffer) {
            break header_end;
        }
    };

    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = header_text.lines();
    let Some(request_line) = lines.next() else {
        return Ok(None);
    };
    let mut request_line_parts = request_line.split_whitespace();
    let method = request_line_parts.next().unwrap_or("GET").to_owned();
    let target = request_line_parts.next().unwrap_or("/").to_owned();

    let mut content_length = 0;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let bytes_read = match stream.read(&mut chunk) {
            Ok(bytes_read) => bytes_read,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => {
                return Err(format!("failed to read WebDriver request body: {error}"));
            }
        };
        if bytes_read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..bytes_read]);
        if body.len() > HTTP_BODY_LIMIT {
            return Err(String::from(
                "incoming WebDriver request body exceeded the size limit",
            ));
        }
    }
    body.truncate(content_length);

    Ok(Some(HttpRequest {
        method,
        target,
        body,
    }))
}

pub(crate) fn write_http_response(
    stream: &mut TcpStream,
    method: &str,
    status: u16,
    status_text: &str,
    body: &[u8],
) -> Result<(), String> {
    let headers = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .map_err(|error| format!("failed to write WebDriver headers: {error}"))?;
    if method != "HEAD" {
        stream
            .write_all(body)
            .map_err(|error| format!("failed to write WebDriver body: {error}"))?;
    }
    stream
        .flush()
        .map_err(|error| format!("failed to flush WebDriver response: {error}"))
}

#[derive(Debug)]
enum WebDriverError {
    Http {
        status: u16,
        status_text: &'static str,
        error: &'static str,
        message: String,
    },
}

impl WebDriverError {
    fn http(status: u16, status_text: &'static str, error: &'static str, message: String) -> Self {
        Self::Http {
            status,
            status_text,
            error,
            message,
        }
    }

    fn invalid_argument(error: impl ToString) -> Self {
        Self::http(400, "Bad Request", "invalid argument", error.to_string())
    }

    fn unsupported(error: impl ToString) -> Self {
        Self::http(
            500,
            "Internal Server Error",
            "unsupported operation",
            error.to_string(),
        )
    }

    fn timeout(error: impl ToString) -> Self {
        Self::http(500, "Internal Server Error", "timeout", error.to_string())
    }

    fn javascript(error: impl ToString) -> Self {
        Self::http(
            500,
            "Internal Server Error",
            "javascript error",
            error.to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_execute_script;
    use serde_json::json;

    #[test]
    fn wraps_execute_script_with_arguments() {
        let wrapped =
            wrap_execute_script("return arguments[0] + arguments[1];", &[json!(1), json!(2)])
                .expect("script wrapping should succeed");
        assert!(wrapped.contains("__formalWebArgs = [1,2]"));
        assert!(wrapped.contains(
            "return Function.prototype.apply.call(window[__formalWebKey], window, __formalWebArgs);"
        ));
        assert!(wrapped.contains("delete window[__formalWebKey];"));
    }
}
