use log::error;
use crate::{
    AUTOMATION_TIMEOUT, AutomationRuntime, AutomationSnapshot, HttpRequest, NEXT_SESSION_ID,
    SCRIPT_TIMEOUT, read_http_request, write_http_response,
};
use base64::Engine as _;
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
            if let Err(error) = thread.join() {
                error!("[webdriver] failed to join server thread: {error:?}");
            }
        }
    }
}

fn run_server(listener: TcpListener, state: Arc<WebDriverState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _address)) => {
                if let Err(error) = handle_connection(stream, &state) {
                    error!("formal-web webdriver server error: {error}");
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                error!("formal-web webdriver accept error: {error}");
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
                state
                    .runtime
                    .request_exit()
                    .map_err(WebDriverError::unsupported)?;
            }
            Ok(Value::Null)
        }
        ("GET", ["formal-web", "frame-viewports"]) => Ok(json!(
            state
                .runtime
                .visible_frame_viewports(AUTOMATION_TIMEOUT)
                .map_err(WebDriverError::timeout)?
        )),
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
