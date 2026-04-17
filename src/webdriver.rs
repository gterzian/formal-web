use crate::AppRunOptions;
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const HTTP_BODY_LIMIT: usize = 2 * 1024 * 1024;
const AUTOMATION_TIMEOUT: Duration = Duration::from_secs(30);
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Args, Debug)]
pub struct WebDriverArgs {
    #[arg(long, default_value_t = 4444)]
    pub port: u16,
    #[arg(long, value_name = "URL")]
    pub startup_url: Option<String>,
    #[arg(long, default_value_t = true)]
    pub exit_on_session_delete: bool,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    body: Vec<u8>,
}

#[derive(Debug)]
struct WebDriverServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct WebDriverState {
    session_id: Mutex<Option<String>>,
    exit_on_session_delete: bool,
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

pub fn run(args: WebDriverArgs) -> Result<(), String> {
    let server = WebDriverServer::start(args.port, args.exit_on_session_delete)?;
    let result = crate::run_app_with_options(AppRunOptions {
        startup_url: args.startup_url,
        window_title: Some(format!("formal-web WebDriver :{}", args.port)),
    });
    drop(server);
    result
}

impl WebDriverServer {
    fn start(port: u16, exit_on_session_delete: bool) -> Result<Self, String> {
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
        });
        let thread_state = Arc::clone(&state);
        let thread = thread::spawn(move || run_server(listener, thread_state, thread_stop));

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

fn dispatch_request_inner(request: &HttpRequest, state: &WebDriverState) -> Result<Value, WebDriverError> {
    let path = request.target.split('?').next().unwrap_or(request.target.as_str());
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match (request.method.as_str(), segments.as_slice()) {
        ("GET", ["status"]) => Ok(status_payload()),
        ("POST", ["session"]) => create_session(state),
        (method, ["session", session_id, rest @ ..]) => {
            ensure_session(state, session_id)?;
            dispatch_session_request(method, rest, &request.body, state)
        }
        _ => Err(WebDriverError::http(
            404,
            "Not Found",
            "unknown command",
            format!("unsupported WebDriver route {} {}", request.method, request.target),
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
                embedder::request_exit().map_err(WebDriverError::unsupported)?;
            }
            Ok(Value::Null)
        }
        ("GET", ["url"]) => {
            let snapshot = current_snapshot()?;
            Ok(json!(snapshot.current_url.unwrap_or_default()))
        }
        ("GET", ["title"]) => execute_script_value("return document.title;", &[]),
        ("POST", ["url"]) => {
            let request: NavigateRequest = serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            embedder::automation_navigate(&request.url, AUTOMATION_TIMEOUT)
                .map_err(WebDriverError::timeout)?;
            Ok(Value::Null)
        }
        ("POST", ["execute", "sync"]) => {
            let request: ExecuteScriptRequest =
                serde_json::from_slice(body).map_err(WebDriverError::invalid_argument)?;
            execute_script_value(&request.script, &request.args)
        }
        _ => Err(WebDriverError::http(
            404,
            "Not Found",
            "unknown command",
            format!("unsupported WebDriver session route {method} /{}", rest.join("/")),
        )),
    }
}

fn status_payload() -> Value {
    let ready = embedder::automation_is_ready()
        && embedder::automation_snapshot(Duration::from_millis(250))
            .map(|snapshot| snapshot.document_id.is_some())
            .unwrap_or(false);
    json!({
        "ready": ready,
        "message": "formal-web webdriver"
    })
}

fn create_session(state: &WebDriverState) -> Result<Value, WebDriverError> {
    if !embedder::automation_is_ready() {
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

    let session_id = format!("formal-web-{}", NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed));
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

fn current_snapshot() -> Result<embedder::AutomationSnapshot, WebDriverError> {
    embedder::automation_snapshot(Duration::from_secs(1)).map_err(WebDriverError::unsupported)
}

fn execute_script_value(script: &str, args: &[Value]) -> Result<Value, WebDriverError> {
    let snapshot = current_snapshot()?;
    let document_id = snapshot.document_id.ok_or_else(|| {
        WebDriverError::http(
            500,
            "Internal Server Error",
            "javascript error",
            String::from("no active document is available for script execution"),
        )
    })?;

    let wrapped = wrap_execute_script(script, args).map_err(WebDriverError::invalid_argument)?;
    ffi::evaluate_script(document_id, wrapped, SCRIPT_TIMEOUT).map_err(WebDriverError::javascript)
}

fn wrap_execute_script(script: &str, args: &[Value]) -> Result<String, serde_json::Error> {
    let args_json = serde_json::to_string(args)?;
    Ok(format!(
        "(() => {{\nconst __formalWebArgs = {args_json};\nconst __formalWebKey = \"__formalWebExecuteScript\";\nwindow[__formalWebKey] = function() {{\n{script}\n}};\ntry {{\nreturn window[__formalWebKey](...__formalWebArgs);\n}} finally {{\ndelete window[__formalWebKey];\n}}\n}})()"
    ))
}

fn json_success(value: Value) -> Vec<u8> {
    serde_json::to_vec(&SuccessResponse { value }).expect("webdriver success response serialization failed")
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

fn find_header_terminator(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| format!("failed to set WebDriver read timeout: {error}"))?;

    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let bytes_read = match stream.read(&mut chunk) {
            Ok(bytes_read) => bytes_read,
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(error) => return Err(format!("failed to read WebDriver request: {error}")),
        };
        if bytes_read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if buffer.len() > HTTP_BODY_LIMIT {
            return Err(String::from("incoming WebDriver request exceeded the size limit"));
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
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
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
            return Err(String::from("incoming WebDriver request body exceeded the size limit"));
        }
    }
    body.truncate(content_length);

    Ok(Some(HttpRequest { method, target, body }))
}

fn write_http_response(
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
        Self::http(
            400,
            "Bad Request",
            "invalid argument",
            error.to_string(),
        )
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
        Self::http(
            500,
            "Internal Server Error",
            "timeout",
            error.to_string(),
        )
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
        let wrapped = wrap_execute_script("return arguments[0] + arguments[1];", &[json!(1), json!(2)])
            .expect("script wrapping should succeed");
        assert!(wrapped.contains("__formalWebArgs = [1,2]"));
        assert!(wrapped.contains("return window[__formalWebKey](...__formalWebArgs);"));
        assert!(wrapped.contains("delete window[__formalWebKey];"));
    }
}