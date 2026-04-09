use crate::AppRunOptions;
use clap::Args;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_WPT_ROOT: &str = "vendor/wpt";
const FORMAL_WEB_WINDOW_TEST_PATH: &str = "__formal_web__/window-test.html";

#[derive(Args, Debug)]
pub struct TestWptArgs {
    #[arg(value_name = "PATH")]
    path: String,
}

struct WptServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn normalize_rel_path(value: &str) -> String {
    value.trim().trim_matches('/').replace('\\', "/")
}

fn path_is_html_file(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|extension| extension.to_str()),
        Some("htm" | "html" | "svg" | "xht" | "xhtml")
    )
}

fn path_is_javascript_test(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("js")
}

fn resolve_wpt_test_path(raw_path: &str, wpt_root: &Path) -> Result<(String, PathBuf), String> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return Err(String::from("WPT path must not be empty"));
    }

    let normalized = normalize_rel_path(raw_path);
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized).to_owned();
    let relative_path = normalized
        .strip_prefix("vendor/wpt/")
        .unwrap_or(&normalized)
        .to_owned();
    let absolute_path = wpt_root.join(&relative_path);
    if !absolute_path.exists() {
        return Err(format!(
            "{} does not exist under {}",
            relative_path,
            wpt_root.display()
        ));
    }
    if absolute_path.is_dir() {
        return Err(String::from(
            "test-wpt expects a single WPT file path for now",
        ));
    }
    Ok((relative_path, absolute_path))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .map_err(|error| format!("invalid percent-encoded byte in URL: {error}"))?;
                let byte = u8::from_str_radix(hex, 16)
                    .map_err(|error| format!("invalid percent-encoded byte `{hex}`: {error}"))?;
                decoded.push(byte);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(decoded).map_err(|error| format!("invalid UTF-8 in URL component: {error}"))
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn mime_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("gif") => "image/gif",
        Some("htm" | "html" | "xht" | "xhtml") => "text/html; charset=utf-8",
        Some("jpeg" | "jpg") => "image/jpeg",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("webp") => "image/webp",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

fn sanitized_relative_path(request_path: &str) -> Result<PathBuf, String> {
    let mut sanitized = PathBuf::new();
    for component in Path::new(request_path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => sanitized.push(part),
            _ => return Err(format!("unsupported path component in `{request_path}`")),
        }
    }
    Ok(sanitized)
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (candidate_key, candidate_value) = pair.split_once('=')?;
        (candidate_key == key)
            .then(|| percent_decode(candidate_value).ok())
            .flatten()
    })
}

fn window_test_wrapper_html(test_path: &str) -> String {
    let escaped_path = escape_html(test_path);
    format!(
        "<!DOCTYPE html>\n<meta charset=\"utf-8\">\n<title>{escaped_path}</title>\n<div id=\"log\"></div>\n<script src=\"/resources/testharness.js\"></script>\n<script src=\"/resources/testharnessreport.js\"></script>\n<script src=\"/{escaped_path}\"></script>\n"
    )
}

fn request_target_parts(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

fn serve_request(
    method: &str,
    target: &str,
    wpt_root: &Path,
) -> Result<(u16, &'static str, &'static str, Vec<u8>), String> {
    if method != "GET" && method != "HEAD" {
        return Ok((
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed".to_vec(),
        ));
    }

    let (path_part, query_part) = request_target_parts(target);
    let decoded_path = percent_decode(path_part.trim_start_matches('/'))?;
    if decoded_path == FORMAL_WEB_WINDOW_TEST_PATH {
        let Some(test_path) = query_value(query_part, "path") else {
            return Ok((
                400,
                "Bad Request",
                "text/plain; charset=utf-8",
                b"missing path query parameter".to_vec(),
            ));
        };
        return Ok((
            200,
            "OK",
            "text/html; charset=utf-8",
            window_test_wrapper_html(&test_path).into_bytes(),
        ));
    }

    let relative_path = sanitized_relative_path(&decoded_path)?;
    let mut absolute_path = wpt_root.join(relative_path);
    if absolute_path.is_dir() {
        absolute_path = absolute_path.join("index.html");
    }

    match fs::read(&absolute_path) {
        Ok(body) => Ok((200, "OK", mime_type_for_path(&absolute_path), body)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok((
            404,
            "Not Found",
            "text/plain; charset=utf-8",
            b"not found".to_vec(),
        )),
        Err(error) => Err(format!("failed to read {}: {error}", absolute_path.display())),
    }
}

fn write_http_response(
    stream: &mut TcpStream,
    method: &str,
    status: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let headers = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .map_err(|error| format!("failed to write HTTP headers: {error}"))?;
    if method != "HEAD" {
        stream
            .write_all(body)
            .map_err(|error| format!("failed to write HTTP body: {error}"))?;
    }
    stream
        .flush()
        .map_err(|error| format!("failed to flush HTTP response: {error}"))
}

fn handle_connection(mut stream: TcpStream, wpt_root: &Path) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| format!("failed to set HTTP read timeout: {error}"))?;

    let mut request = [0_u8; 8192];
    let bytes_read = stream
        .read(&mut request)
        .map_err(|error| format!("failed to read HTTP request: {error}"))?;
    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&request[..bytes_read]);
    let Some(request_line) = request.lines().next() else {
        return Ok(());
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let target = parts.next().unwrap_or("/");

    let (status, status_text, content_type, body) = serve_request(method, target, wpt_root)?;
    write_http_response(&mut stream, method, status, status_text, content_type, &body)
}

fn run_wpt_server(listener: TcpListener, wpt_root: PathBuf, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _address)) => {
                if let Err(error) = handle_connection(stream, &wpt_root) {
                    eprintln!("formal-web WPT server error: {error}");
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                eprintln!("formal-web WPT server accept error: {error}");
                break;
            }
        }
    }
}

impl WptServer {
    fn start(wpt_root: &Path) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("failed to bind local WPT server: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure local WPT server: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("failed to read local WPT server address: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_root = wpt_root.to_path_buf();
        let thread = thread::spawn(move || run_wpt_server(listener, thread_root, thread_stop));

        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for WptServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn visible_test_url(server: &WptServer, path: &str) -> Result<String, String> {
    if path_is_html_file(path) {
        return Ok(format!("{}/{path}", server.base_url()));
    }
    if path_is_javascript_test(path) {
        return Ok(format!(
            "{}/{}?path={}",
            server.base_url(),
            FORMAL_WEB_WINDOW_TEST_PATH,
            percent_encode(path)
        ));
    }

    Err(format!(
        "{} is not a supported visible WPT file; use an HTML file or a JavaScript test file",
        path
    ))
}

pub fn run(args: TestWptArgs) -> Result<(), String> {
    let wpt_root = repo_root()
        .join(DEFAULT_WPT_ROOT)
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", DEFAULT_WPT_ROOT))?;
    let (path, _absolute_path) = resolve_wpt_test_path(&args.path, &wpt_root)?;
    let server = WptServer::start(&wpt_root)?;
    let startup_url = visible_test_url(&server, &path)?;

    println!("WPT root: {}", wpt_root.display());
    println!("Mode: visible single-test window");
    println!("  {path}");

    crate::run_app_with_options(AppRunOptions {
        startup_url: Some(startup_url),
        window_title: Some(format!("formal-web WPT: {path}")),
    })
}