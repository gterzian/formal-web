use crate::{
    AUTOMATION_TIMEOUT, CdpEvent, HttpRequest, SCRIPT_TIMEOUT, find_header_terminator,
    read_http_request, write_http_response,
};
use crate::AutomationRuntime;
use base64::Engine as _;
use clap::Args;
use serde_json::{Value, json};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex, mpsc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tungstenite::{Error as WebSocketError, Message, WebSocket, accept};
use uuid::Uuid;

const CDP_BROWSER_PRODUCT: &str = "formal-web/0.1";
const CDP_PROTOCOL_VERSION: &str = "1.3";
const CDP_SOCKET_IO_TIMEOUT: Duration = Duration::from_millis(100);
const CDP_PEEK_LIMIT: usize = 16 * 1024;
const CDP_EXECUTION_CONTEXT_ID: u64 = 1;

#[derive(Args, Debug, Clone)]
pub struct CdpArgs {
    #[arg(long, default_value_t = 9222)]
    pub port: u16,

    #[arg(long)]
    pub headless: bool,

    #[arg(long, value_name = "URL")]
    pub startup_url: Option<String>,
}

pub struct CdpServer {
    stop: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
    connection_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

struct CdpState {
    port: u16,
    runtime: AutomationRuntime,
    browser_id: String,
    page_target_id: String,
}

struct CdpPageSnapshot {
    url: String,
    frame_id: String,
}

#[derive(Clone, Copy)]
enum CdpRoute {
    Browser,
    Page,
}

struct PeekedRequest {
    method: String,
    target: String,
    websocket_upgrade: bool,
}

struct CdpRequest {
    id: Option<Value>,
    method: String,
    params: Value,
    session_id: Option<String>,
}

struct CdpConnectionState {
    route: CdpRoute,
    session_id: Option<String>,
    page_enabled: bool,
    runtime_enabled: bool,
}

impl CdpServer {
    pub fn start(port: u16, runtime: AutomationRuntime) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("failed to bind CDP server on port {port}: {error}"))?;
        let actual_port = listener
            .local_addr()
            .map_err(|error| format!("failed to read CDP listener address: {error}"))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure CDP server listener: {error}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let connection_threads = Arc::new(Mutex::new(Vec::new()));
        let thread_connection_threads = Arc::clone(&connection_threads);
        let state = Arc::new(CdpState {
            port: actual_port,
            runtime,
            browser_id: new_cdp_id(),
            page_target_id: new_cdp_id(),
        });
        let thread_state = Arc::clone(&state);
        let listener_thread = thread::Builder::new()
            .name(String::from("formal-web-cdp"))
            .spawn(move || {
                run_cdp_server(
                    listener,
                    thread_state,
                    thread_stop,
                    thread_connection_threads,
                )
            })
            .map_err(|error| format!("failed to spawn CDP server thread: {error}"))?;

        Ok(Self {
            stop,
            listener_thread: Some(listener_thread),
            connection_threads,
        })
    }
}

impl Drop for CdpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(listener_thread) = self.listener_thread.take() {
            let _ = listener_thread.join();
        }

        let connection_threads = {
            let mut guard = self
                .connection_threads
                .lock()
                .expect("cdp connection thread mutex poisoned");
            std::mem::take(&mut *guard)
        };
        for connection_thread in connection_threads {
            let _ = connection_thread.join();
        }
    }
}

impl CdpState {
    fn browser_ws_url(&self) -> String {
        format!(
            "ws://localhost:{}/devtools/browser/{}",
            self.port, self.browser_id
        )
    }

    fn page_ws_url(&self) -> String {
        format!(
            "ws://localhost:{}/devtools/page/{}",
            self.port, self.page_target_id
        )
    }

    fn page_snapshot(&self) -> CdpPageSnapshot {
        let snapshot = self.runtime.snapshot(Duration::from_millis(250)).ok();
        let url = snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot.current_url.clone().or_else(|| {
                    (!snapshot.displayed_url.is_empty()).then(|| snapshot.displayed_url.clone())
                })
            })
            .unwrap_or_else(|| String::from("about:blank"));
        let frame_id = snapshot
            .and_then(|snapshot| snapshot.navigable_id.map(|id| id.to_string()))
            .unwrap_or_else(|| self.page_target_id.clone());

        CdpPageSnapshot { url, frame_id }
    }

    fn target_info(&self, attached: bool) -> Value {
        let snapshot = self.page_snapshot();
        target_info_payload(self, &snapshot.url, attached)
    }
}

impl CdpRequest {
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|error| format!("invalid CDP JSON: {error}"))?;
        let Some(object) = value.as_object() else {
            return Err(String::from("CDP message must be a JSON object"));
        };
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| String::from("CDP message is missing a string `method`"))?
            .to_owned();

        Ok(Self {
            id: object.get("id").cloned(),
            method,
            params: object
                .get("params")
                .cloned()
                .unwrap_or_else(|| json!({})),
            session_id: object
                .get("sessionId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })
    }
}

impl CdpConnectionState {
    fn new(route: CdpRoute) -> Self {
        Self {
            route,
            session_id: None,
            page_enabled: false,
            runtime_enabled: false,
        }
    }

    fn handle_text(&mut self, state: &CdpState, text: &str) -> Result<Vec<Value>, String> {
        let request = CdpRequest::parse(text)?;
        let mut events = Vec::new();
        let response_session_id = self.response_session_id(&request.method, request.session_id.clone());

        let result = match request.method.as_str() {
            "Browser.getVersion" => Ok(json!({
                "protocolVersion": CDP_PROTOCOL_VERSION,
                "product": CDP_BROWSER_PRODUCT,
                "revision": "0",
                "userAgent": CDP_BROWSER_PRODUCT,
                "jsVersion": "0.0"
            })),
            "Target.getTargets" => Ok(json!({
                "targetInfos": [state.target_info(self.session_id.is_some())]
            })),
            "Target.attachToTarget" => {
                let requested_target = request
                    .params
                    .get("targetId")
                    .and_then(Value::as_str)
                    .unwrap_or(state.page_target_id.as_str());
                if requested_target != state.page_target_id {
                    Err(format!("unknown target id `{requested_target}`"))
                } else {
                    let session_id = self.session_id.get_or_insert_with(new_cdp_id).clone();
                    Ok(json!({ "sessionId": session_id }))
                }
            }
            "Target.setDiscoverTargets" => {
                if request
                    .params
                    .get("discover")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    events.push(target_created_event(state, self.session_id.is_some()));
                }
                Ok(json!({}))
            }
            "Target.setAutoAttach" => {
                if request
                    .params
                    .get("autoAttach")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let session_id = self.session_id.get_or_insert_with(new_cdp_id).clone();
                    events.push(attached_to_target_event(state, &session_id));
                }
                Ok(json!({}))
            }
            "Page.enable" => {
                self.page_enabled = true;
                if let Some(event) = self.current_frame_navigated_event(state) {
                    events.push(event);
                }
                Ok(json!({}))
            }
            "Page.navigate" => {
                let Some(url) = request.params.get("url").and_then(Value::as_str) else {
                    return Ok(vec![cdp_error_response(
                        request.id,
                        response_session_id.as_deref(),
                        String::from("`Page.navigate` requires a string `url` parameter"),
                    )]);
                };

                let snapshot = state
                    .runtime
                    .navigate(url, AUTOMATION_TIMEOUT)
                    .map_err(|error| format!("navigation failed: {error}"))?;
                let frame_id = snapshot
                    .navigable_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| state.page_target_id.clone());
                Ok(json!({
                    "frameId": frame_id,
                    "loaderId": frame_id
                }))
            }
            "Page.captureScreenshot" => {
                let png = state
                    .runtime
                    .screenshot(AUTOMATION_TIMEOUT)
                    .map_err(|error| format!("screenshot failed: {error}"))?;
                Ok(json!({
                    "data": base64::engine::general_purpose::STANDARD.encode(png)
                }))
            }
            "Page.getFrameTree" => {
                let snapshot = state.page_snapshot();
                Ok(json!({
                    "frameTree": {
                        "frame": frame_payload(&snapshot.url, &snapshot.frame_id)
                    }
                }))
            }
            "Page.addScriptToEvaluateOnNewDocument" => {
                Ok(json!({ "identifier": new_cdp_id() }))
            }
            "Page.createIsolatedWorld" => {
                Ok(json!({ "executionContextId": CDP_EXECUTION_CONTEXT_ID }))
            }
            "Runtime.enable" => {
                self.runtime_enabled = true;
                if let Some(event) = self.current_execution_context_created_event(state) {
                    events.push(event);
                }
                Ok(json!({}))
            }
            "Runtime.evaluate" => {
                let Some(expression) = request.params.get("expression").and_then(Value::as_str)
                else {
                    return Ok(vec![cdp_error_response(
                        request.id,
                        response_session_id.as_deref(),
                        String::from("`Runtime.evaluate` requires a string `expression` parameter"),
                    )]);
                };
                let value = state
                    .runtime
                    .evaluate_script(expression.to_owned(), SCRIPT_TIMEOUT)
                    .map_err(|error| format!("script evaluation failed: {error}"))?;
                Ok(json!({
                    "result": remote_object_payload(value)
                }))
            }
            "Input.dispatchMouseEvent" => {
                let event_type = request
                    .params
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if event_type == "mouseReleased" {
                    let x = request
                        .params
                        .get("x")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| String::from("`Input.dispatchMouseEvent` requires `x`"))?;
                    let y = request
                        .params
                        .get("y")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| String::from("`Input.dispatchMouseEvent` requires `y`"))?;
                    state
                        .runtime
                        .click(x as f32, y as f32, AUTOMATION_TIMEOUT)
                        .map_err(|error| format!("mouse dispatch failed: {error}"))?;
                }
                Ok(json!({}))
            }
            "Input.dispatchKeyEvent" => Ok(json!({})),
            "DOM.getDocument" => {
                let snapshot = state.page_snapshot();
                Ok(json!({
                    "root": document_root_payload(&snapshot.url)
                }))
            }
            _ => Ok(json!({})),
        };

        let mut outgoing = Vec::new();
        if let Some(id) = request.id {
            match result {
                Ok(result) => outgoing.push(cdp_success_response(
                    id,
                    response_session_id.as_deref(),
                    result,
                )),
                Err(message) => outgoing.push(cdp_error_response(
                    Some(id),
                    response_session_id.as_deref(),
                    message,
                )),
            }
        }
        outgoing.extend(events);
        Ok(outgoing)
    }

    fn translate_event(&self, state: &CdpState, event: CdpEvent) -> Vec<Value> {
        match event {
            CdpEvent::NavigationCommitted {
                url,
                frame_id,
                timestamp,
            } => {
                let mut outgoing = vec![target_info_changed_event(state, &url, self.session_id.is_some())];
                let session_id = self.event_session_id();
                if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
                    return outgoing;
                }
                if self.runtime_enabled {
                    outgoing.push(event_message(
                        "Runtime.executionContextsCleared",
                        json!({}),
                        session_id,
                    ));
                    outgoing.push(event_message(
                        "Runtime.executionContextCreated",
                        json!({
                            "context": execution_context_payload(&url, &frame_id)
                        }),
                        session_id,
                    ));
                }
                if self.page_enabled {
                    outgoing.push(event_message(
                        "Page.frameNavigated",
                        json!({
                            "frame": frame_payload(&url, &frame_id)
                        }),
                        session_id,
                    ));
                    outgoing.push(event_message(
                        "Page.loadEventFired",
                        json!({
                            "timestamp": timestamp
                        }),
                        session_id,
                    ));
                }
                outgoing
            }
        }
    }

    fn response_session_id(
        &self,
        method: &str,
        request_session_id: Option<String>,
    ) -> Option<String> {
        request_session_id.or_else(|| {
            if matches!(self.route, CdpRoute::Browser) && session_scoped_method(method) {
                self.session_id.clone()
            } else {
                None
            }
        })
    }

    fn event_session_id(&self) -> Option<&str> {
        match self.route {
            CdpRoute::Browser => self.session_id.as_deref(),
            CdpRoute::Page => None,
        }
    }

    fn current_frame_navigated_event(&self, state: &CdpState) -> Option<Value> {
        let snapshot = state.page_snapshot();
        let session_id = self.event_session_id();
        if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
            return None;
        }
        Some(event_message(
            "Page.frameNavigated",
            json!({
                "frame": frame_payload(&snapshot.url, &snapshot.frame_id)
            }),
            session_id,
        ))
    }

    fn current_execution_context_created_event(&self, state: &CdpState) -> Option<Value> {
        let snapshot = state.page_snapshot();
        let session_id = self.event_session_id();
        if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
            return None;
        }
        Some(event_message(
            "Runtime.executionContextCreated",
            json!({
                "context": execution_context_payload(&snapshot.url, &snapshot.frame_id)
            }),
            session_id,
        ))
    }
}

fn run_cdp_server(
    listener: TcpListener,
    state: Arc<CdpState>,
    stop: Arc<AtomicBool>,
    connection_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _address)) => {
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                let spawn_result = thread::Builder::new()
                    .name(String::from("formal-web-cdp-connection"))
                    .spawn(move || {
                        if let Err(error) = handle_cdp_stream(stream, state, stop) {
                            eprintln!("formal-web cdp connection error: {error}");
                        }
                    });
                match spawn_result {
                    Ok(connection_thread) => {
                        let mut guard = connection_threads
                            .lock()
                            .expect("cdp connection thread mutex poisoned");
                        guard.push(connection_thread);
                    }
                    Err(error) => {
                        eprintln!("formal-web cdp thread spawn error: {error}");
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                eprintln!("formal-web cdp accept error: {error}");
                break;
            }
        }
    }
}

fn handle_cdp_stream(
    mut stream: TcpStream,
    state: Arc<CdpState>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let Some(peeked_request) = peek_request(&stream)? else {
        return Ok(());
    };

    if peeked_request.websocket_upgrade {
        match parse_route(&peeked_request.target, &state) {
            Some(route) => handle_websocket_connection(stream, state, stop, route),
            None => write_json_response(
                &mut stream,
                &peeked_request.method,
                404,
                "Not Found",
                json!({ "error": "unknown CDP websocket route" }),
            ),
        }
    } else {
        handle_http_connection(stream, &state)
    }
}

fn peek_request(stream: &TcpStream) -> Result<Option<PeekedRequest>, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| format!("failed to set CDP request peek timeout: {error}"))?;

    let mut buffer = vec![0_u8; CDP_PEEK_LIMIT];
    loop {
        let bytes_peeked = match stream.peek(&mut buffer) {
            Ok(bytes_peeked) => bytes_peeked,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => return Err(format!("failed to peek CDP request: {error}")),
        };
        if bytes_peeked == 0 {
            return Ok(None);
        }

        let header_bytes = &buffer[..bytes_peeked];
        if let Some(header_end) = find_header_terminator(header_bytes) {
            let header_text = String::from_utf8_lossy(&header_bytes[..header_end]);
            let mut lines = header_text.lines();
            let request_line = lines.next().unwrap_or_default();
            let mut request_parts = request_line.split_whitespace();
            let method = request_parts.next().unwrap_or("GET").to_owned();
            let target = request_parts.next().unwrap_or("/").to_owned();

            let mut upgrade_websocket = false;
            let mut connection_upgrade = false;
            for line in lines {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some((name, value)) = trimmed.split_once(':') {
                    let name = name.trim();
                    let value = value.trim();
                    if name.eq_ignore_ascii_case("upgrade")
                        && value.eq_ignore_ascii_case("websocket")
                    {
                        upgrade_websocket = true;
                    }
                    if name.eq_ignore_ascii_case("connection")
                        && value.to_ascii_lowercase().contains("upgrade")
                    {
                        connection_upgrade = true;
                    }
                }
            }

            return Ok(Some(PeekedRequest {
                method,
                target,
                websocket_upgrade: upgrade_websocket && connection_upgrade,
            }));
        }

        if bytes_peeked == buffer.len() {
            return Err(String::from(
                "incoming CDP request headers exceeded the size limit",
            ));
        }
    }
}

fn parse_route(target: &str, state: &CdpState) -> Option<CdpRoute> {
    let path = target.split('?').next().unwrap_or(target);
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match segments.as_slice() {
        ["devtools", "browser", browser_id] if *browser_id == state.browser_id => {
            Some(CdpRoute::Browser)
        }
        ["devtools", "page", page_target_id] if *page_target_id == state.page_target_id => {
            Some(CdpRoute::Page)
        }
        _ => None,
    }
}

fn handle_http_connection(mut stream: TcpStream, state: &CdpState) -> Result<(), String> {
    let Some(request) = read_http_request(&mut stream)? else {
        return Ok(());
    };

    let (status, status_text, body) = dispatch_http_request(&request, state);
    write_http_response(&mut stream, &request.method, status, status_text, &body)
}

fn dispatch_http_request(request: &HttpRequest, state: &CdpState) -> (u16, &'static str, Vec<u8>) {
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
        ("GET", ["json", "version"]) => (
            200,
            "OK",
            json_bytes(json!({
                "Browser": CDP_BROWSER_PRODUCT,
                "Protocol-Version": CDP_PROTOCOL_VERSION,
                "User-Agent": CDP_BROWSER_PRODUCT,
                "webSocketDebuggerUrl": state.browser_ws_url()
            })),
        ),
        ("GET", ["json"]) | ("GET", ["json", "list"]) => (
            200,
            "OK",
            json_bytes(json!([{
                "description": "",
                "devtoolsFrontendUrl": "",
                "id": state.page_target_id,
                "title": "formal-web",
                "type": "page",
                "url": state.page_snapshot().url,
                "webSocketDebuggerUrl": state.page_ws_url()
            }])),
        ),
        _ => (
            404,
            "Not Found",
            json_bytes(json!({
                "error": format!("unsupported CDP route {} {}", request.method, request.target)
            })),
        ),
    }
}

fn handle_websocket_connection(
    stream: TcpStream,
    state: Arc<CdpState>,
    stop: Arc<AtomicBool>,
    route: CdpRoute,
) -> Result<(), String> {
    let mut websocket =
        accept(stream).map_err(|error| format!("failed to accept CDP websocket: {error}"))?;
    websocket
        .get_mut()
        .set_read_timeout(Some(CDP_SOCKET_IO_TIMEOUT))
        .map_err(|error| format!("failed to configure CDP websocket timeout: {error}"))?;

    let (event_sender, event_receiver) = mpsc::channel();
    state
        .runtime
        .set_cdp_event_sink(Some(event_sender), AUTOMATION_TIMEOUT)?;

    let mut connection = CdpConnectionState::new(route);
    let result = (|| {
        loop {
            drain_cdp_events(&mut websocket, &connection, &event_receiver, &state)?;
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }

            match websocket.read() {
                Ok(message) => match message {
                    Message::Text(text) => {
                        let outgoing = connection.handle_text(&state, text.as_ref())?;
                        for message in outgoing {
                            send_cdp_message(&mut websocket, &message)?;
                        }
                        drain_cdp_events(&mut websocket, &connection, &event_receiver, &state)?;
                    }
                    Message::Ping(payload) => websocket
                        .send(Message::Pong(payload))
                        .map_err(|error| format!("failed to send CDP pong: {error}"))?,
                    Message::Close(_frame) => return Ok(()),
                    _ => {}
                },
                Err(WebSocketError::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    continue;
                }
                Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => {
                    return Ok(());
                }
                Err(error) => return Err(format!("failed to read CDP websocket message: {error}")),
            }
        }
    })();

    let _ = state
        .runtime
        .set_cdp_event_sink(None, Duration::from_millis(250));
    result
}

fn drain_cdp_events(
    websocket: &mut WebSocket<TcpStream>,
    connection: &CdpConnectionState,
    event_receiver: &mpsc::Receiver<CdpEvent>,
    state: &CdpState,
) -> Result<(), String> {
    while let Ok(event) = event_receiver.try_recv() {
        for message in connection.translate_event(state, event) {
            send_cdp_message(websocket, &message)?;
        }
    }
    Ok(())
}

fn send_cdp_message(websocket: &mut WebSocket<TcpStream>, message: &Value) -> Result<(), String> {
    let payload = serde_json::to_string(message)
        .map_err(|error| format!("failed to serialize CDP message: {error}"))?;
    websocket
        .send(Message::Text(payload.into()))
        .map_err(|error| format!("failed to send CDP websocket message: {error}"))
}

fn write_json_response(
    stream: &mut TcpStream,
    method: &str,
    status: u16,
    status_text: &str,
    body: Value,
) -> Result<(), String> {
    let body = json_bytes(body);
    write_http_response(stream, method, status, status_text, &body)
}

fn json_bytes(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).expect("cdp json response serialization should succeed")
}

fn new_cdp_id() -> String {
    Uuid::new_v4().to_string()
}

fn cdp_success_response(id: Value, session_id: Option<&str>, result: Value) -> Value {
    let mut response = json!({
        "id": id,
        "result": result,
    });
    if let Some(session_id) = session_id {
        response["sessionId"] = json!(session_id);
    }
    response
}

fn cdp_error_response(id: Option<Value>, session_id: Option<&str>, message: String) -> Value {
    let mut response = json!({
        "error": {
            "code": -32000,
            "message": message,
        }
    });
    if let Some(id) = id {
        response["id"] = id;
    }
    if let Some(session_id) = session_id {
        response["sessionId"] = json!(session_id);
    }
    response
}

fn event_message(method: &str, params: Value, session_id: Option<&str>) -> Value {
    let mut event = json!({
        "method": method,
        "params": params,
    });
    if let Some(session_id) = session_id {
        event["sessionId"] = json!(session_id);
    }
    event
}

fn target_created_event(state: &CdpState, attached: bool) -> Value {
    json!({
        "method": "Target.targetCreated",
        "params": {
            "targetInfo": state.target_info(attached)
        }
    })
}

fn attached_to_target_event(state: &CdpState, session_id: &str) -> Value {
    json!({
        "method": "Target.attachedToTarget",
        "params": {
            "sessionId": session_id,
            "targetInfo": state.target_info(true),
            "waitingForDebugger": false
        }
    })
}

fn target_info_changed_event(state: &CdpState, url: &str, attached: bool) -> Value {
    json!({
        "method": "Target.targetInfoChanged",
        "params": {
            "targetInfo": target_info_payload(state, url, attached)
        }
    })
}

fn target_info_payload(state: &CdpState, url: &str, attached: bool) -> Value {
    json!({
        "targetId": state.page_target_id,
        "type": "page",
        "title": "formal-web",
        "url": url,
        "attached": attached,
        "canAccessOpener": false,
    })
}

fn frame_payload(url: &str, frame_id: &str) -> Value {
    json!({
        "id": frame_id,
        "loaderId": frame_id,
        "url": url,
        "securityOrigin": url_origin(url),
        "mimeType": "text/html",
    })
}

fn execution_context_payload(url: &str, frame_id: &str) -> Value {
    json!({
        "id": CDP_EXECUTION_CONTEXT_ID,
        "origin": url_origin(url),
        "name": "",
        "auxData": {
            "isDefault": true,
            "type": "default",
            "frameId": frame_id,
        }
    })
}

fn document_root_payload(url: &str) -> Value {
    json!({
        "nodeId": 1,
        "backendNodeId": 1,
        "nodeType": 9,
        "nodeName": "#document",
        "localName": "",
        "nodeValue": "",
        "childNodeCount": 1,
        "documentURL": url,
        "baseURL": url,
        "xmlVersion": "",
    })
}

fn url_origin(url: &str) -> String {
    if url == "about:blank" {
        return String::from("null");
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return String::from("null");
    };
    let authority = rest.split('/').next().unwrap_or_default();
    format!("{scheme}://{authority}")
}

fn remote_object_payload(value: Value) -> Value {
    match value {
        Value::Null => json!({
            "type": "object",
            "subtype": "null",
            "value": Value::Null,
        }),
        Value::Bool(value) => json!({
            "type": "boolean",
            "value": value,
        }),
        Value::Number(value) => json!({
            "type": "number",
            "value": value,
        }),
        Value::String(value) => json!({
            "type": "string",
            "value": value,
        }),
        Value::Array(value) => json!({
            "type": "object",
            "subtype": "array",
            "value": value,
            "description": "Array",
        }),
        Value::Object(value) => json!({
            "type": "object",
            "value": value,
            "description": "Object",
        }),
    }
}

fn session_scoped_method(method: &str) -> bool {
    method.starts_with("Page.")
        || method.starts_with("Runtime.")
        || method.starts_with("Input.")
        || method.starts_with("DOM.")
        || method.starts_with("Accessibility.")
}

#[cfg(test)]
mod tests {
    use super::{
        CDP_BROWSER_PRODUCT, CDP_PROTOCOL_VERSION, CdpEvent, CdpServer,
        find_header_terminator,
    };
    use crate::{
        AutomationCommand, AutomationRuntime, AutomationSnapshot, AutomationVisibleFrameViewport,
    };
    use base64::Engine as _;
    use ipc_messages::content::{NavigableId, WebviewId};
    use serde_json::{Value, json};
    use std::io::{ErrorKind, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;
    use tungstenite::{Error as WebSocketError, Message, WebSocket, client::client};

    const TEST_TIMEOUT: Duration = Duration::from_secs(2);
    const RETRY_DELAY: Duration = Duration::from_millis(10);

    struct MockRuntimeState {
        snapshot: AutomationSnapshot,
        screenshot: Vec<u8>,
        evaluation_result: Value,
        last_script: Option<String>,
        event_sink: Option<mpsc::Sender<CdpEvent>>,
    }

    #[test]
    fn discovery_endpoints_expose_browser_and_page_targets() {
        let (_server, _state, port) = start_test_server();

        let version = http_get_json(port, "/json/version");
        assert_eq!(version["Browser"], CDP_BROWSER_PRODUCT);
        assert_eq!(version["Protocol-Version"], CDP_PROTOCOL_VERSION);
        let browser_ws_url = version["webSocketDebuggerUrl"]
            .as_str()
            .expect("browser websocket url should be a string");
        assert!(browser_ws_url.starts_with(&format!(
            "ws://localhost:{port}/devtools/browser/"
        )));

        let targets = http_get_json(port, "/json")
            .as_array()
            .cloned()
            .expect("CDP target list should be an array");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0]["type"], "page");
        assert_eq!(targets[0]["title"], "formal-web");
        assert_eq!(targets[0]["url"], "about:blank");
        let page_ws_url = targets[0]["webSocketDebuggerUrl"]
            .as_str()
            .expect("page websocket url should be a string");
        assert!(page_ws_url.starts_with(&format!("ws://localhost:{port}/devtools/page/")));
    }

    #[test]
    fn browser_websocket_supports_minimum_session_flow() {
        let (_server, state, port) = start_test_server();
        let version = http_get_json(port, "/json/version");
        let targets = http_get_json(port, "/json")
            .as_array()
            .cloned()
            .expect("CDP target list should be an array");
        let browser_ws_url = version["webSocketDebuggerUrl"]
            .as_str()
            .expect("browser websocket url should be a string")
            .to_owned();
        let page_target_id = targets[0]["id"]
            .as_str()
            .expect("page target id should be a string")
            .to_owned();
        let mut socket = connect_websocket(&browser_ws_url, port);
        let mut next_id = 1_u64;

        let (browser_version, browser_events) =
            send_cdp_request(&mut socket, &mut next_id, "Browser.getVersion", json!({}), None);
        assert!(browser_events.is_empty());
        assert_eq!(browser_version["result"]["product"], CDP_BROWSER_PRODUCT);
        assert_eq!(browser_version["result"]["protocolVersion"], CDP_PROTOCOL_VERSION);

        let (targets_response, target_events) =
            send_cdp_request(&mut socket, &mut next_id, "Target.getTargets", json!({}), None);
        assert!(target_events.is_empty());
        assert_eq!(
            targets_response["result"]["targetInfos"][0]["targetId"],
            page_target_id
        );
        assert_eq!(targets_response["result"]["targetInfos"][0]["attached"], false);

        let (attach_response, attach_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Target.attachToTarget",
            json!({ "targetId": page_target_id, "flatten": true }),
            None,
        );
        assert!(attach_events.is_empty());
        let session_id = attach_response["result"]["sessionId"]
            .as_str()
            .expect("session id should be a string")
            .to_owned();

        let (page_enable, page_enable_events) =
            send_cdp_request(&mut socket, &mut next_id, "Page.enable", json!({}), Some(&session_id));
        assert_eq!(page_enable["sessionId"], session_id);
        assert_event_methods(&page_enable_events, &["Page.frameNavigated"]);
        assert_eq!(page_enable_events[0]["sessionId"], session_id);

        let (runtime_enable, runtime_enable_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Runtime.enable",
            json!({}),
            Some(&session_id),
        );
        assert_eq!(runtime_enable["sessionId"], session_id);
        assert_event_methods(&runtime_enable_events, &["Runtime.executionContextCreated"]);
        assert_eq!(runtime_enable_events[0]["sessionId"], session_id);

        let navigation_url = "https://example.com/formal-web";
        let (navigate_response, navigate_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.navigate",
            json!({ "url": navigation_url }),
            Some(&session_id),
        );
        assert_eq!(navigate_response["sessionId"], session_id);
        assert!(navigate_response["result"]["frameId"]
            .as_str()
            .is_some_and(|frame_id| !frame_id.is_empty()));
        assert_event_methods(
            &navigate_events,
            &[
                "Target.targetInfoChanged",
                "Runtime.executionContextsCleared",
                "Runtime.executionContextCreated",
                "Page.frameNavigated",
                "Page.loadEventFired",
            ],
        );
        assert!(navigate_events.iter().any(|event| {
            event["method"] == "Page.frameNavigated"
                && event["params"]["frame"]["url"] == navigation_url
                && event["sessionId"] == session_id
        }));
        assert!(navigate_events.iter().any(|event| {
            event["method"] == "Page.loadEventFired"
                && event["params"]["timestamp"] == json!(1234.5)
                && event["sessionId"] == session_id
        }));

        let (frame_tree, frame_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.getFrameTree",
            json!({}),
            Some(&session_id),
        );
        assert!(frame_events.is_empty());
        assert_eq!(frame_tree["result"]["frameTree"]["frame"]["url"], navigation_url);

        let (document_response, document_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.getDocument",
            json!({}),
            Some(&session_id),
        );
        assert!(document_events.is_empty());
        assert_eq!(document_response["result"]["root"]["documentURL"], navigation_url);

        let (evaluate_response, evaluate_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Runtime.evaluate",
            json!({ "expression": "1 + 1" }),
            Some(&session_id),
        );
        assert!(evaluate_events.is_empty());
        assert_eq!(evaluate_response["result"]["result"]["value"], 2);
        assert_eq!(
            state
                .lock()
                .expect("mock runtime mutex poisoned")
                .last_script
                .as_deref(),
            Some("1 + 1")
        );

        let (screenshot_response, screenshot_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.captureScreenshot",
            json!({}),
            Some(&session_id),
        );
        assert!(screenshot_events.is_empty());
        assert_eq!(
            screenshot_response["result"]["data"],
            base64::engine::general_purpose::STANDARD.encode([0_u8, 1, 2, 3])
        );

        let (unknown_response, unknown_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "FormalWeb.unknownMethod",
            json!({}),
            Some(&session_id),
        );
        assert!(unknown_events.is_empty());
        assert_eq!(unknown_response["result"], json!({}));

        let _ = socket.close(None);
    }

    fn start_test_server() -> (CdpServer, Arc<Mutex<MockRuntimeState>>, u16) {
        let port = free_local_port();
        let state = Arc::new(Mutex::new(MockRuntimeState {
            snapshot: automation_snapshot("about:blank", NavigableId::from_u128(1)),
            screenshot: vec![0_u8, 1, 2, 3],
            evaluation_result: json!(2),
            last_script: None,
            event_sink: None,
        }));
        let runtime = mock_runtime(Arc::clone(&state));
        let server = CdpServer::start(port, runtime).expect("test CDP server should start");
        (server, state, port)
    }

    fn mock_runtime(state: Arc<Mutex<MockRuntimeState>>) -> AutomationRuntime {
        let send_state = Arc::clone(&state);
        AutomationRuntime::new(
            move |command| {
                let mut state = send_state.lock().expect("mock runtime mutex poisoned");
                match command {
                    AutomationCommand::Snapshot { reply } => {
                        let _ = reply.send(Ok(state.snapshot.clone()));
                    }
                    AutomationCommand::VisibleFrameViewports { reply } => {
                        let _ = reply.send(Ok(Vec::<AutomationVisibleFrameViewport>::new()));
                    }
                    AutomationCommand::Screenshot { reply } => {
                        let _ = reply.send(Ok(state.screenshot.clone()));
                    }
                    AutomationCommand::Navigate { url, reply } => {
                        let frame_id = state
                            .snapshot
                            .navigable_id
                            .expect("mock snapshot should have a frame id");
                        state.snapshot.current_url = Some(url.clone());
                        state.snapshot.displayed_url = url.clone();
                        if let Some(sink) = state.event_sink.as_ref() {
                            let _ = sink.send(CdpEvent::NavigationCommitted {
                                url,
                                frame_id: frame_id.to_string(),
                                timestamp: 1234.5,
                            });
                        }
                        let _ = reply.send(Ok(state.snapshot.clone()));
                    }
                    AutomationCommand::Click { reply, .. }
                    | AutomationCommand::ClickElement { reply, .. }
                    | AutomationCommand::Scroll { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    AutomationCommand::EvaluateScript { source, reply, .. } => {
                        state.last_script = Some(source);
                        let _ = reply.send(Ok(state.evaluation_result.clone()));
                    }
                    AutomationCommand::SetCdpEventSink { sink, reply } => {
                        state.event_sink = sink;
                        let _ = reply.send(Ok(()));
                    }
                }
                Ok(())
            },
            || Ok(()),
            || true,
        )
    }

    fn automation_snapshot(url: &str, navigable_id: NavigableId) -> AutomationSnapshot {
        AutomationSnapshot {
            webview_id: Some(WebviewId(navigable_id)),
            current_url: Some(url.to_owned()),
            displayed_url: url.to_owned(),
            navigable_id: Some(navigable_id),
            has_top_level_traversable: true,
        }
    }

    fn free_local_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0))
            .expect("ephemeral port bind should succeed")
            .local_addr()
            .expect("ephemeral port address should be available")
            .port()
    }

    fn http_get_json(port: u16, path: &str) -> Value {
        let body = http_get(port, path);
        serde_json::from_slice(&body).expect("HTTP response body should be valid JSON")
    }

    fn http_get(port: u16, path: &str) -> Vec<u8> {
        let mut last_error = None;
        for _attempt in 0..50 {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(mut stream) => {
                    stream
                        .set_read_timeout(Some(TEST_TIMEOUT))
                        .expect("HTTP read timeout should be configurable");
                    let request = format!(
                        "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
                    );
                    stream
                        .write_all(request.as_bytes())
                        .expect("HTTP request should write successfully");
                    let mut response = Vec::new();
                    stream
                        .read_to_end(&mut response)
                        .expect("HTTP response should be readable");
                    let header_end = find_header_terminator(&response)
                        .expect("HTTP response should contain a header terminator");
                    let header_text = String::from_utf8_lossy(&response[..header_end]);
                    assert!(header_text.starts_with("HTTP/1.1 200 OK"));
                    return response[header_end..].to_vec();
                }
                Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                    last_error = Some(error);
                    thread::sleep(RETRY_DELAY);
                }
                Err(error) => panic!("unexpected HTTP connect failure: {error}"),
            }
        }

        panic!(
            "timed out connecting to test CDP server: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| String::from("no connection attempts were made"))
        );
    }

    fn connect_websocket(url: &str, port: u16) -> WebSocket<TcpStream> {
        let mut last_error = None;
        for _attempt in 0..50 {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(TEST_TIMEOUT))
                        .expect("websocket read timeout should be configurable");
                    stream
                        .set_write_timeout(Some(TEST_TIMEOUT))
                        .expect("websocket write timeout should be configurable");
                    return client(url, stream)
                        .expect("websocket handshake should succeed")
                        .0;
                }
                Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                    last_error = Some(error);
                    thread::sleep(RETRY_DELAY);
                }
                Err(error) => panic!("unexpected websocket connect failure: {error}"),
            }
        }

        panic!(
            "timed out connecting websocket to test CDP server: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| String::from("no connection attempts were made"))
        );
    }

    fn send_cdp_request(
        socket: &mut WebSocket<TcpStream>,
        next_id: &mut u64,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> (Value, Vec<Value>) {
        let id = *next_id;
        *next_id += 1;
        let mut request = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        if let Some(session_id) = session_id {
            request["sessionId"] = json!(session_id);
        }
        socket
            .send(Message::Text(request.to_string().into()))
            .expect("CDP request should send successfully");

        let mut events = Vec::new();
        let response = loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let message: Value = serde_json::from_str(text.as_ref())
                        .expect("CDP websocket payload should be valid JSON");
                    if message["id"].as_u64() == Some(id) {
                        break message;
                    }
                    events.push(message);
                }
                Ok(Message::Ping(payload)) => {
                    socket
                        .send(Message::Pong(payload))
                        .expect("CDP pong should send successfully");
                }
                Ok(Message::Close(_)) => panic!("CDP websocket closed while waiting for {method}"),
                Ok(_) => {}
                Err(error) => panic!("failed to read CDP response for {method}: {error}"),
            }
        };
        events.extend(drain_websocket_messages(socket));
        (response, events)
    }

    fn drain_websocket_messages(socket: &mut WebSocket<TcpStream>) -> Vec<Value> {
        let mut messages = Vec::new();
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => messages.push(
                    serde_json::from_str(text.as_ref())
                        .expect("CDP websocket payload should be valid JSON"),
                ),
                Ok(Message::Ping(payload)) => {
                    socket
                        .send(Message::Pong(payload))
                        .expect("CDP pong should send successfully");
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(WebSocketError::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break;
                }
                Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => break,
                Err(error) => panic!("failed to drain CDP websocket messages: {error}"),
            }
        }
        messages
    }

    fn assert_event_methods(events: &[Value], expected_methods: &[&str]) {
        let methods = events
            .iter()
            .filter_map(|event| event["method"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(methods, expected_methods);
    }
}