mod cdp;
mod webdriver;

use ipc_messages::content::{NavigableId, WebviewId};
use serde::Serialize;
use serde_json::Value;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::AtomicU64, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use cdp::{CdpArgs, CdpServerHandle};
pub use webdriver::{WebDriverArgs, WebDriverServer};

pub(crate) const HTTP_BODY_LIMIT: usize = 2 * 1024 * 1024;
pub(crate) const AUTOMATION_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

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
    /// Abort any currently pending navigation.  No-op if none is pending.
    AbortNavigation {
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
            AutomationCommand::AbortNavigation { reply } => {
                self.abort_pending_navigation(String::from("aborted by CDP client"));
                let _ = reply.send(Ok(()));
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
        if snapshot.webview_id.is_none() {
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
        let frame_id = snapshot
            .navigable_id
            .map(|id| id.to_string())
            .or_else(|| snapshot.webview_id.map(|id| id.0.to_string()));
        let Some(frame_id) = frame_id else {
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

    /// Abort any stale pending navigation.  No-op if none is pending.
    pub fn reset_navigation(&self) -> Result<(), String> {
        let (reply, receiver) = mpsc::channel();
        (self.send_command)(AutomationCommand::AbortNavigation { reply })?;
        receiver.recv().map_err(|error| {
            format!("failed to reset navigation state: {error}")
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

#[derive(Debug)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) body: Vec<u8>,
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
