use base64::Engine as _;
use clap::Parser;
use serde_json::{Value, json};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{Error as WebSocketError, Message, WebSocket, client::client};

const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_millis(50);
const CDP_HANDSHAKE_RETRY_TIMEOUT: Duration = Duration::from_secs(10);
const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Parser, Debug)]
#[command(name = "cdp-smoke-check")]
#[command(about = "Launch formal-web and verify a Rust-native external CDP client flow")]
struct Cli {
    #[arg(long, value_name = "PATH")]
    browser: Option<PathBuf>,

    #[arg(long)]
    port: Option<u16>,

    #[arg(long, default_value_t = false)]
    rebuild_browser: bool,

    #[arg(long, default_value_t = false)]
    headless: bool,
}

struct BrowserProcess {
    child: Child,
    stderr_log_path: PathBuf,
    pid: u32,
}

struct CdpClient {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cdp-smoke-check: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let repo_root = repo_root()?;
    let navigation_url = startup_artifact_url(&repo_root)?;
    let browser_path = cli
        .browser
        .unwrap_or_else(|| repo_root.join("embedder/target/release/formal-web-embedder"));

    if cli.rebuild_browser || !browser_path.exists() {
        build_browser(&repo_root)?;
    }

    let port = cli.port.unwrap_or_else(free_local_port);
    let mut browser = BrowserProcess::spawn(
        &browser_path,
        &repo_root,
        port,
        &navigation_url,
        cli.headless,
    )?;
    wait_for_cdp_server(&mut browser, port)?;

    let version = http_get_json(port, "/json/version")?;
    let targets = http_get_json(port, "/json")?;
    let browser_ws_url = json_str_field(&version, &["webSocketDebuggerUrl"])?;
    let page_target_id = json_str_field(&targets, &["0", "id"])?;

    let (mut client, browser_version, browser_events) = connect_ready_client(
        &mut browser,
        browser_ws_url,
        CDP_HANDSHAKE_RETRY_TIMEOUT,
    )?;
    ensure_no_error("Browser.getVersion", &browser_version)?;
    if !browser_events.is_empty() {
        return Err(String::from(
            "Browser.getVersion unexpectedly produced CDP events",
        ));
    }

    wait_for_startup_target_url(&mut client, &navigation_url)?;

    let (attach_response, attach_events) = client.send(
        "Target.attachToTarget",
        json!({
            "targetId": page_target_id,
            "flatten": true,
        }),
        None,
    )?;
    ensure_no_error("Target.attachToTarget", &attach_response)?;
    if !attach_events.is_empty() {
        return Err(String::from(
            "Target.attachToTarget unexpectedly produced CDP events",
        ));
    }
    let session_id = json_str_field(&attach_response, &["result", "sessionId"])?;

    let _ = client.send("Page.enable", json!({}), Some(session_id))?;
    let _ = client.send("Runtime.enable", json!({}), Some(session_id))?;

    let (navigate_response, _) = client.send(
        "Page.navigate",
        json!({ "url": navigation_url }),
        Some(session_id),
    )?;
    if let Some(error) = navigate_response.get("error") {
        let message = error.to_string();
        if !(message.contains("timed out") || message.contains("no active top-level traversable")) {
            return Err(format!("Page.navigate returned CDP error: {error}"));
        }
    }

    wait_for_document_ready(&mut client, session_id)?;

    let (document_response, document_events) =
        client.send("DOM.getDocument", json!({}), Some(session_id))?;
    ensure_no_error("DOM.getDocument", &document_response)?;
    if !document_events.is_empty() {
        return Err(String::from("DOM.getDocument unexpectedly produced CDP events"));
    }
    let root_node_id = json_u64_field(&document_response, &["result", "root", "nodeId"])?;

    let (query_response, query_events) = client.send(
        "DOM.querySelector",
        json!({
            "nodeId": root_node_id,
            "selector": "#click-counter-button",
        }),
        Some(session_id),
    )?;
    ensure_no_error("DOM.querySelector", &query_response)?;
    if !query_events.is_empty() {
        return Err(String::from("DOM.querySelector unexpectedly produced CDP events"));
    }
    let button_node_id = json_u64_field(&query_response, &["result", "nodeId"])?;
    if button_node_id <= root_node_id {
        return Err(String::from("CDP button query returned an invalid node id"));
    }

    let (query_all_response, _) = client.send(
        "DOM.querySelectorAll",
        json!({
            "nodeId": root_node_id,
            "selector": "button",
        }),
        Some(session_id),
    )?;
    ensure_no_error("DOM.querySelectorAll", &query_all_response)?;
    let button_nodes = json_array_field(&query_all_response, &["result", "nodeIds"])?;
    if button_nodes.len() < 2 || button_nodes[0].as_u64() != Some(button_node_id) {
        return Err(String::from(
            "DOM.querySelectorAll did not return the expected button node ordering",
        ));
    }

    let (search_response, _) = client.send(
        "DOM.performSearch",
        json!({ "query": "button" }),
        Some(session_id),
    )?;
    ensure_no_error("DOM.performSearch", &search_response)?;
    let search_id = json_str_field(&search_response, &["result", "searchId"])?;
    let (search_results_response, _) = client.send(
        "DOM.getSearchResults",
        json!({
            "searchId": search_id,
            "fromIndex": 0,
            "toIndex": 8,
        }),
        Some(session_id),
    )?;
    ensure_no_error("DOM.getSearchResults", &search_results_response)?;
    let search_results = json_array_field(&search_results_response, &["result", "nodeIds"])?;
    if search_results.first().and_then(Value::as_u64) != Some(button_node_id) {
        return Err(String::from(
            "DOM.performSearch did not preserve button node identity",
        ));
    }

    let (describe_response, _) = client.send(
        "DOM.describeNode",
        json!({ "nodeId": button_node_id }),
        Some(session_id),
    )?;
    ensure_no_error("DOM.describeNode", &describe_response)?;
    if json_str_field(&describe_response, &["result", "node", "nodeName"])? != "BUTTON" {
        return Err(String::from("DOM.describeNode did not describe a button element"));
    }

    let (startup_probe_response, _) = client.send(
        "Runtime.evaluate",
        json!({
            "expression": "(() => { const image = document.querySelector('img'); const frame = document.querySelector('iframe.cross-origin-frame'); const link = document.querySelector('a.article-link'); const accent = document.getElementById('accent-toggle-button'); const signalCard = document.getElementById('signal-card'); return { imageLoaded: !!(image && image.complete && image.naturalWidth > 0), iframePresent: !!frame, iframeSrc: frame ? frame.getAttribute('src') || '' : '', linkHref: link ? link.getAttribute('href') || '' : '', accentPressed: accent ? accent.getAttribute('aria-pressed') || '' : '', signalState: signalCard ? signalCard.getAttribute('data-active') || '' : '' }; })()",
        }),
        Some(session_id),
    )?;
    ensure_no_error("Runtime.evaluate", &startup_probe_response)?;
    let startup_probe = json_field(&startup_probe_response, &["result", "result", "value"])?;
    if !json_bool_field(startup_probe, &["imageLoaded"])? {
        return Err(String::from(
            "startup artifact image did not report as loaded via Runtime.evaluate",
        ));
    }
    if !json_bool_field(startup_probe, &["iframePresent"])? {
        return Err(String::from(
            "startup artifact cross-origin iframe was not present",
        ));
    }
    if !json_str_field(startup_probe, &["iframeSrc"])?
        .contains("gterzian.github.io/lsp_agent")
    {
        return Err(String::from(
            "startup artifact iframe src did not match the expected cross-origin target",
        ));
    }
    if json_str_field(startup_probe, &["linkHref"])? != "navigated.html" {
        return Err(String::from(
            "startup artifact article link did not target navigated.html",
        ));
    }
    if json_str_field(startup_probe, &["accentPressed"])? != "false" {
        return Err(String::from(
            "accent toggle did not start with aria-pressed=false",
        ));
    }
    if json_str_field(startup_probe, &["signalState"])? != "false" {
        return Err(String::from(
            "signal card did not start in the inactive state",
        ));
    }

    let _ = client.send(
        "DOM.scrollIntoViewIfNeeded",
        json!({ "nodeId": button_node_id }),
        Some(session_id),
    )?;

    let (partial_ax_response, _) = client.send(
        "Accessibility.getPartialAXTree",
        json!({ "nodeId": button_node_id }),
        Some(session_id),
    )?;
    ensure_no_error("Accessibility.getPartialAXTree", &partial_ax_response)?;
    let partial_ax_nodes = json_array_field(&partial_ax_response, &["result", "nodes"])?;
    let partial_ax_first = partial_ax_nodes
        .first()
        .ok_or_else(|| String::from("Accessibility.getPartialAXTree returned no nodes"))?;
    if json_str_field(partial_ax_first, &["role", "value"])? != "button" {
        return Err(String::from(
            "Accessibility.getPartialAXTree did not identify the button role",
        ));
    }
    if json_str_field(partial_ax_first, &["name", "value"])? != "Increment click counter" {
        return Err(String::from(
            "Accessibility.getPartialAXTree did not return the expected button name",
        ));
    }

    let (full_ax_response, _) =
        client.send("Accessibility.getFullAXTree", json!({}), Some(session_id))?;
    ensure_no_error("Accessibility.getFullAXTree", &full_ax_response)?;
    let full_ax_nodes = json_array_field(&full_ax_response, &["result", "nodes"])?;
    if !full_ax_nodes.iter().any(|node| {
        json_str_field(node, &["role", "value"]).is_ok_and(|role| role == "button")
            && json_str_field(node, &["name", "value"])
                .is_ok_and(|name| name == "Increment click counter")
    }) {
        return Err(String::from(
            "Accessibility.getFullAXTree did not contain the startup button",
        ));
    }

    let (screenshot_response, _) =
        client.send("Page.captureScreenshot", json!({}), Some(session_id))?;
    ensure_no_error("Page.captureScreenshot", &screenshot_response)?;
    let screenshot_b64 = json_str_field(&screenshot_response, &["result", "data"])?;
    let screenshot_png = base64::engine::general_purpose::STANDARD
        .decode(screenshot_b64)
        .map_err(|error| format!("failed to decode CDP screenshot: {error}"))?;
    let screenshot = image::load_from_memory(&screenshot_png)
        .map_err(|error| format!("failed to decode screenshot image: {error}"))?;
    if screenshot.width() == 0 || screenshot.height() == 0 {
        return Err(String::from("CDP screenshot decoded to an empty image"));
    }

    let (click_count_response, _) = client.send(
        "Runtime.evaluate",
        json!({
            "expression": "(() => { const button = document.querySelector('#click-counter-button'); if (!button) return 'missing-button'; button.click(); const counter = document.getElementById('click-count'); return counter ? counter.textContent.trim() : 'missing-counter'; })()",
        }),
        Some(session_id),
    )?;
    ensure_no_error("Runtime.evaluate", &click_count_response)?;
    if json_str_field(&click_count_response, &["result", "result", "value"])? != "1" {
        return Err(String::from(
            "runtime-evaluated CDP click did not increment the startup click counter",
        ));
    }

    let (accent_toggle_response, _) = client.send(
        "Runtime.evaluate",
        json!({
            "expression": "(() => { const button = document.getElementById('accent-toggle-button'); const signalCard = document.getElementById('signal-card'); const signalState = document.getElementById('signal-state'); if (!button || !signalCard || !signalState) return { ok: false, reason: 'missing-elements' }; button.click(); return { ok: true, pressed: button.getAttribute('aria-pressed') || '', active: button.getAttribute('data-active') || '', card: signalCard.getAttribute('data-active') || '', state: signalState.textContent ? signalState.textContent.trim() : '' }; })()",
        }),
        Some(session_id),
    )?;
    ensure_no_error("Runtime.evaluate", &accent_toggle_response)?;
    let accent_result = json_field(&accent_toggle_response, &["result", "result", "value"])?;
    if !json_bool_field(accent_result, &["ok"])? {
        return Err(String::from(
            "accent toggle probe did not execute successfully",
        ));
    }
    if json_str_field(accent_result, &["pressed"])? != "true"
        || json_str_field(accent_result, &["active"])? != "true"
        || json_str_field(accent_result, &["card"])? != "true"
        || json_str_field(accent_result, &["state"])? != "Signal armed"
    {
        return Err(String::from(
            "accent toggle probe did not produce the expected active UI state",
        ));
    }

    let (scroll_probe_response, _) = client.send(
        "Runtime.evaluate",
        json!({
            "expression": "(() => { const before = Math.max(window.scrollY || 0, document.documentElement ? document.documentElement.scrollTop || 0 : 0); const target = Math.max(0, (document.documentElement ? document.documentElement.scrollHeight || 0 : 0) - (window.innerHeight || 0)); window.scrollTo(0, target); const after = Math.max(window.scrollY || 0, document.documentElement ? document.documentElement.scrollTop || 0 : 0); window.scrollTo(0, 0); return { before, after, scrolled: after > before }; })()",
        }),
        Some(session_id),
    )?;
    ensure_no_error("Runtime.evaluate", &scroll_probe_response)?;
    let scroll_probe = json_field(&scroll_probe_response, &["result", "result", "value"])?;
    if !json_bool_field(scroll_probe, &["scrolled"])? {
        return Err(String::from(
            "startup artifact scroll probe did not move the document scroll offset",
        ));
    }

    let (iframe_probe_response, _) = client.send(
        "Runtime.evaluate",
        json!({
            "expression": "(() => { const frame = document.querySelector('iframe.cross-origin-frame'); if (!frame) return { ok: false, reason: 'missing-iframe' }; frame.focus(); const focused = document.activeElement === frame; frame.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window })); return { ok: true, focused }; })()",
        }),
        Some(session_id),
    )?;
    ensure_no_error("Runtime.evaluate", &iframe_probe_response)?;
    let iframe_probe = json_field(&iframe_probe_response, &["result", "result", "value"])?;
    if !json_bool_field(iframe_probe, &["ok"])? || !json_bool_field(iframe_probe, &["focused"])? {
        return Err(String::from(
            "startup artifact iframe interaction probe did not focus the iframe",
        ));
    }

    let (hover_probe_response, _) = client.send(
        "Runtime.evaluate",
        json!({
            "expression": "(() => { const selectors = ['body:hover::before', '.hover-probe:hover', '.counter-button:hover', '.accent-button:hover']; let matched = 0; for (const sheet of Array.from(document.styleSheets || [])) { let rules = []; try { rules = Array.from(sheet.cssRules || []); } catch (_) { continue; } for (const rule of rules) { const text = String(rule.cssText || ''); for (const selector of selectors) { if (text.includes(selector)) { matched += 1; } } } } return { expected: selectors.length, matched, ok: matched >= selectors.length }; })()",
        }),
        Some(session_id),
    )?;
    ensure_no_error("Runtime.evaluate", &hover_probe_response)?;
    let hover_probe = json_field(&hover_probe_response, &["result", "result", "value"])?;
    if !json_bool_field(hover_probe, &["ok"])? {
        return Err(String::from(
            "startup artifact hover probe did not find the expected hover selectors in active stylesheets",
        ));
    }

    let fps_deadline = Instant::now() + Duration::from_secs(4);
    let mut fps_value = None;
    while Instant::now() < fps_deadline {
        let (fps_response, _) = client.send(
            "Runtime.evaluate",
            json!({
                "expression": "(() => { const el = document.getElementById('fps-value'); return el ? el.textContent.trim() : ''; })()",
            }),
            Some(session_id),
        )?;
        ensure_no_error("Runtime.evaluate", &fps_response)?;
        let fps_text = json_str_field(&fps_response, &["result", "result", "value"])?;
        if let Ok(parsed) = fps_text.parse::<f64>() {
            if parsed > 0.0 {
                fps_value = Some(parsed);
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if fps_value.is_none() {
        return Err(String::from(
            "startup artifact FPS probe did not advance above 0.0",
        ));
    }

    println!(
        "cdp smoke passed: target={} session={} screenshot={}x{} fps={:.1}",
        page_target_id,
        session_id,
        screenshot.width(),
        screenshot.height(),
        fps_value.unwrap_or(0.0),
    );

    let _ = client.send("Browser.close", json!({}), None);
    browser.wait_for_exit(Duration::from_secs(5))?;
    Ok(())
}

impl BrowserProcess {
    fn spawn(
        browser_path: &Path,
        repo_root: &Path,
        port: u16,
        startup_url: &str,
        headless: bool,
    ) -> Result<Self, String> {
        let stderr_log_path = std::env::temp_dir().join(format!(
            "formal-web-cdp-smoke-stderr-{}.log",
            std::process::id()
        ));
        let stderr_log_file = File::create(&stderr_log_path)
            .map_err(|error| format!("failed to create browser stderr log file: {error}"))?;
        let mut command = Command::new(browser_path);
        command
            .current_dir(repo_root)
            .arg("cdp")
            .arg("--port")
            .arg(port.to_string())
            .arg("--startup-url")
            .arg(startup_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_log_file));
        if headless {
            command.arg("--headless");
        }
        let child = command
            .spawn()
            .map_err(|error| {
                format!(
                    "failed to launch {}: {error}",
                    browser_path.display()
                )
            })?;
        let pid = child.id();
        eprintln!(
            "cdp-smoke-check: browser stderr log: {}",
            stderr_log_path.display()
        );
        Ok(Self {
            child,
            stderr_log_path,
            pid,
        })
    }

    fn ensure_running(&mut self) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Err(format!(
                "formal-web exited unexpectedly with status {}\n{}",
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| String::from("unknown")),
                tail_file(&self.stderr_log_path, 4096)
                    .unwrap_or_else(|error| format!("failed to read browser stderr log: {error}"))
            )),
            Ok(None) => Ok(()),
            Err(error) => Err(format!("failed to poll formal-web process: {error}")),
        }
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(format!(
                        "formal-web exited with status {}\n{}",
                        status
                            .code()
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| String::from("unknown")),
                        tail_file(&self.stderr_log_path, 4096).unwrap_or_else(|error| {
                            format!("failed to read browser stderr log: {error}")
                        })
                    ))
                }
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Ok(None) => {
                    return Err(format!(
                        "timed out waiting for formal-web to exit cleanly\n{}",
                        tail_file(&self.stderr_log_path, 4096).unwrap_or_else(|error| {
                            format!("failed to read browser stderr log: {error}")
                        })
                    ))
                }
                Err(error) => {
                    return Err(format!("failed to wait for formal-web process exit: {error}"))
                }
            }
        }
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = Command::new("pkill")
            .arg("-TERM")
            .arg("-P")
            .arg(self.pid.to_string())
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = Command::new("pkill")
            .arg("-KILL")
            .arg("-P")
            .arg(self.pid.to_string())
            .status();
    }
}

impl CdpClient {
    fn connect(ws_url: &str) -> Result<Self, String> {
        let (host, port) = parse_ws_host_port(ws_url)?;
        let stream = TcpStream::connect((host.as_str(), port))
            .map_err(|error| format!("failed to connect to {ws_url}: {error}"))?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("failed to set websocket read timeout: {error}"))?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("failed to set websocket write timeout: {error}"))?;
        let socket = client(ws_url, stream)
            .map_err(|error| format!("failed to complete websocket handshake: {error}"))?
            .0;
        Ok(Self { socket, next_id: 1 })
    }

    fn send(
        &mut self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<(Value, Vec<Value>), String> {
        let request_id = self.next_id;
        self.next_id += 1;
        let mut request = json!({
            "id": request_id,
            "method": method,
            "params": params,
        });
        if let Some(session_id) = session_id {
            request["sessionId"] = json!(session_id);
        }

        self.socket
            .send(Message::Text(request.to_string().into()))
            .map_err(|error| format!("failed to send {method} over CDP: {error}"))?;

        let mut events = Vec::new();
        let deadline = Instant::now() + CDP_COMMAND_TIMEOUT;
        let response = loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    let message: Value = serde_json::from_str(text.as_ref())
                        .map_err(|error| format!("invalid CDP JSON payload: {error}"))?;
                    if message["id"].as_u64() == Some(request_id) {
                        break message;
                    }
                    events.push(message);
                }
                Ok(Message::Ping(payload)) => self
                    .socket
                    .send(Message::Pong(payload))
                    .map_err(|error| format!("failed to send CDP pong: {error}"))?,
                Ok(Message::Close(frame)) => {
                    return Err(format!(
                        "CDP websocket closed while waiting for {method}: {frame:?}"
                    ));
                }
                Ok(_) => {}
                Err(WebSocketError::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    if Instant::now() >= deadline {
                        return Err(format!("timed out waiting for CDP response to {method}"));
                    }
                }
                Err(error) => {
                    return Err(format!("failed to read CDP response for {method}: {error}"));
                }
            }
        };

        events.extend(self.drain_events()?);
        Ok((response, events))
    }

    fn drain_events(&mut self) -> Result<Vec<Value>, String> {
        let mut events = Vec::new();
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    let message: Value = serde_json::from_str(text.as_ref())
                        .map_err(|error| format!("invalid CDP JSON payload: {error}"))?;
                    events.push(message);
                }
                Ok(Message::Ping(payload)) => self
                    .socket
                    .send(Message::Pong(payload))
                    .map_err(|error| format!("failed to send CDP pong: {error}"))?,
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(WebSocketError::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break;
                }
                Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => break,
                Err(error) => return Err(format!("failed to drain CDP events: {error}")),
            }
        }
        Ok(events)
    }
}

fn repo_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| String::from("failed to locate the repository root from verification/"))
}

fn build_browser(repo_root: &Path) -> Result<(), String> {
    let status = Command::new("rustup")
        .current_dir(repo_root)
        .arg("run")
        .arg("1.92.0")
        .arg("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg("embedder/Cargo.toml")
        .arg("--bin")
        .arg("formal-web-embedder")
        .status()
        .map_err(|error| format!("failed to spawn browser build: {error}"))?;
    if !status.success() {
        Err(format!(
            "browser build exited with status {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| String::from("unknown"))
        ))
    } else {
        build_sidecar(repo_root, "content/Cargo.toml", "formal-web-content")?;
        build_sidecar(repo_root, "net/Cargo.toml", "formal-web-net")?;
        Ok(())
    }
}

fn build_sidecar(repo_root: &Path, manifest_path: &str, binary_name: &str) -> Result<(), String> {
    let status = Command::new("rustup")
        .current_dir(repo_root)
        .arg("run")
        .arg("1.92.0")
        .arg("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--target-dir")
        .arg("target/sidecar-prebuild")
        .arg("--bin")
        .arg(binary_name)
        .status()
        .map_err(|error| format!("failed to spawn {binary_name} build: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{binary_name} build exited with status {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| String::from("unknown"))
        ))
    }
}

fn wait_for_cdp_server(browser: &mut BrowserProcess, port: u16) -> Result<(), String> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        browser.ensure_running()?;
        match http_get_json(port, "/json/version") {
            Ok(_) => return Ok(()),
            Err(_error) if Instant::now() < deadline => thread::sleep(RETRY_DELAY),
            Err(error) => return Err(format!("timed out waiting for CDP readiness: {error}")),
        }
    }
}

fn connect_ready_client(
    browser: &mut BrowserProcess,
    browser_ws_url: &str,
    timeout: Duration,
) -> Result<(CdpClient, Value, Vec<Value>), String> {
    let deadline = Instant::now() + timeout;

    loop {
        browser.ensure_running()?;

        match CdpClient::connect(browser_ws_url)
            .and_then(|mut client| match client.send("Browser.getVersion", json!({}), None) {
                Ok((response, events)) => Ok((client, response, events)),
                Err(error) => Err(error),
            })
        {
            Ok(result) => return Ok(result),
            Err(_error) => {}
        }

        if Instant::now() >= deadline {
            return Err(String::from(
                "timed out waiting for stable CDP websocket handshake",
            ));
        }

        thread::sleep(RETRY_DELAY);
    }
}

fn wait_for_document_ready(client: &mut CdpClient, session_id: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last_observation = String::from("no readiness observations collected");
    while Instant::now() < deadline {
        let (response, _) = client.send(
            "Runtime.evaluate",
            json!({
                "expression": "(() => { const state = document.readyState || ''; const button = !!document.getElementById('click-counter-button'); const frame = !!document.querySelector('iframe.cross-origin-frame'); return { state, button, frame }; })()",
            }),
            Some(session_id),
        )?;
        if let Some(error_value) = response.get("error") {
            let error_text = error_value.to_string();
            last_observation = format!("runtime-error: {error_text}");
            if error_text.contains("no active top-level traversable") {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            return Err(format!("Runtime.evaluate returned CDP error: {error_text}"));
        }
        let ready_probe = json_field(&response, &["result", "result", "value"])?;
        let state = json_str_field(ready_probe, &["state"])?;
        let has_button = json_bool_field(ready_probe, &["button"])?;
        let has_frame = json_bool_field(ready_probe, &["frame"])?;
        last_observation = format!(
            "state={state} button={has_button} frame={has_frame}"
        );
        if (state == "interactive" || state == "complete") && has_button && has_frame {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "timed out waiting for startup artifact document readiness ({last_observation})"
    ))
}

fn wait_for_startup_target_url(client: &mut CdpClient, expected_url: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let (response, _) = client.send("Target.getTargets", json!({}), None)?;
        ensure_no_error("Target.getTargets", &response)?;
        let target_infos = json_array_field(&response, &["result", "targetInfos"])?;
        if target_infos.iter().any(|info| {
            json_str_field(info, &["type"]).is_ok_and(|kind| kind == "page")
                && json_str_field(info, &["url"]).is_ok_and(|url| url == expected_url)
        }) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(String::from(
        "timed out waiting for startup target URL to become active",
    ))
}

fn http_get_json(port: u16, path: &str) -> Result<Value, String> {
    let body = http_get(port, path)?;
    serde_json::from_slice(&body).map_err(|error| format!("invalid JSON from {path}: {error}"))
}

fn http_get(port: u16, path: &str) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|error| format!("failed to connect to http://127.0.0.1:{port}{path}: {error}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| format!("failed to set HTTP read timeout: {error}"))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("failed to write HTTP request: {error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("failed to read HTTP response: {error}"))?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| String::from("HTTP response did not include a header terminator"))?;
    let status_line = String::from_utf8_lossy(&response[..header_end]);
    if !status_line.starts_with("HTTP/1.1 200") {
        return Err(format!("unexpected HTTP response for {path}: {status_line}"));
    }
    Ok(response[header_end..].to_vec())
}

fn parse_ws_host_port(ws_url: &str) -> Result<(String, u16), String> {
    let without_scheme = ws_url
        .strip_prefix("ws://")
        .ok_or_else(|| format!("unsupported websocket url `{ws_url}`"))?;
    let host_port = without_scheme.split('/').next().unwrap_or_default();
    let (host, port) = host_port
        .rsplit_once(':')
        .ok_or_else(|| format!("websocket url `{ws_url}` is missing a port"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| format!("invalid websocket port in `{ws_url}`: {error}"))?;
    Ok((host.to_owned(), port))
}

fn startup_artifact_url(repo_root: &Path) -> Result<String, String> {
    let artifact_path = repo_root.join(STARTUP_ARTIFACT_RELATIVE_PATH);
    let artifact_path = artifact_path
        .canonicalize()
        .map_err(|error| format!("failed to resolve startup artifact path: {error}"))?;
    Ok(format!("file://{}", artifact_path.display()))
}

fn free_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("ephemeral port bind should succeed")
        .local_addr()
        .expect("ephemeral port address should be available")
        .port()
}

fn tail_file(path: &Path, max_bytes: usize) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let start = data.len().saturating_sub(max_bytes);
    let excerpt = String::from_utf8_lossy(&data[start..]);
    Ok(format!("browser stderr tail ({}):\n{}", path.display(), excerpt))
}

fn ensure_no_error(method: &str, response: &Value) -> Result<(), String> {
    if let Some(error) = response.get("error") {
        return Err(format!("{method} returned CDP error: {error}"));
    }
    Ok(())
}

fn json_str_field<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, String> {
    json_field(value, path)?
        .as_str()
        .ok_or_else(|| format!("expected string at JSON path {}", path.join(".")))
}

fn json_u64_field(value: &Value, path: &[&str]) -> Result<u64, String> {
    json_field(value, path)?
        .as_u64()
        .ok_or_else(|| format!("expected u64 at JSON path {}", path.join(".")))
}

fn json_bool_field(value: &Value, path: &[&str]) -> Result<bool, String> {
    json_field(value, path)?
        .as_bool()
        .ok_or_else(|| format!("expected bool at JSON path {}", path.join(".")))
}

fn json_array_field<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Vec<Value>, String> {
    json_field(value, path)?
        .as_array()
        .ok_or_else(|| format!("expected array at JSON path {}", path.join(".")))
}

fn json_field<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value, String> {
    let mut current = value;
    for segment in path {
        current = if let Ok(index) = segment.parse::<usize>() {
            current
                .as_array()
                .and_then(|items| items.get(index))
                .ok_or_else(|| format!("missing array index {index} at JSON path {}", path.join(".")))?
        } else {
            current
                .get(*segment)
                .ok_or_else(|| format!("missing key `{segment}` at JSON path {}", path.join(".")))?
        };
    }
    Ok(current)
}
